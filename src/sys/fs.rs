use crate::{sys, log};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use bit_field::BitField;
use lazy_static::lazy_static;
use spin::Mutex;

lazy_static! {
    pub static ref BLOCK_DEVICE: Mutex<Option<BlockDevice>> = Mutex::new(None);
}

const MAGIC: &'static str = "MOROS FS";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Dir = 0,
    File = 1,
}

pub fn dirname(pathname: &str) -> &str {
    let n = pathname.len();
    let i = match pathname.rfind('/') {
        Some(0) => 1,
        Some(i) => i,
        None => n,
    };
    &pathname[0..i]
}

pub fn filename(pathname: &str) -> &str {
    let n = pathname.len();
    let i = match pathname.rfind('/') {
        Some(i) => i + 1,
        None => 0,
    };
    &pathname[i..n]
}

// Transform "foo.txt" into "/path/to/foo.txt"
pub fn realpath(pathname: &str) -> String {
    if pathname.starts_with("/") {
        pathname.into()
    } else {
        let dirname = sys::process::dir();
        let sep = if dirname.ends_with("/") { "" } else { "/" };
        format!("{}{}{}", dirname, sep, pathname)
    }
}

#[derive(Clone)]
pub struct File {
    name: String,
    addr: u32,
    size: u32,
    dir: Dir, // TODO: Replace with `parent: Some(Dir)` and also add it to `Dir`
    // TODO: add `offset: usize`
}

impl File {
    pub fn create(pathname: &str) -> Option<Self> {
        let pathname = realpath(pathname);
        let dirname = dirname(&pathname);
        let filename = filename(&pathname);
        if let Some(dir) = Dir::open(dirname) {
            if let Some(dir_entry) = dir.create_file(filename) {
                return Some(dir_entry.to_file());
            }
        }
        None
    }

    pub fn open(pathname: &str) -> Option<Self> {
        let pathname = realpath(pathname);
        let dirname = dirname(&pathname);
        let filename = filename(&pathname);
        if let Some(dir) = Dir::open(dirname) {
            if let Some(dir_entry) = dir.find(filename) {
                if dir_entry.is_file() {
                    return Some(dir_entry.to_file());
                }
            }
        }
        None
    }

    pub fn size(&self) -> usize {
        self.size as usize
    }

    // TODO: return `Result<usize>`
    // TODO: start `i` at `self.offset` instead of `0`
    // TODO: set `self.offset` to `i` at the end
    pub fn read(&self, buf: &mut [u8]) -> usize {
        let buf_len = buf.len();
        let mut addr = self.addr;
        let mut i = 0;
        loop {
            let block = Block::read(addr);
            let data = block.data();
            let data_len = data.len();
            for j in 0..data_len {
                if i == buf_len || i == self.size() {
                    return i;
                }
                buf[i] = data[j];
                i += 1;
            }
            match block.next() {
                Some(next_block) => addr = next_block.addr(),
                None => return i,
            }
        }
    }

    // TODO: add `seek(&mut self, pos: SeekFrom) -> Result<u32>`

    // TODO: add `read_to_end(&self, buf: &mut Vec<u8>) -> Result<u32>`

    // TODO: `return Result<String>`
    pub fn read_to_string(&self) -> String {
        let mut buf: Vec<u8> = Vec::with_capacity(self.size());
        buf.resize(self.size(), 0);
        let bytes = self.read(&mut buf);
        buf.resize(bytes, 0);
        String::from_utf8(buf).unwrap()
    }

    // TODO: `return Result<usize>`
    // TODO: start `i` at `self.offset` instead of `0`
    // TODO: set `self.offset` to `i` at the end
    pub fn write(&mut self, buf: &[u8]) -> Result<(), ()> {
        let buf_len = buf.len();
        let mut addr = self.addr;
        let mut i = 0;
        while i < buf_len {
            let mut block = Block::new(addr);
            let data = block.data_mut();
            let data_len = data.len();
            for j in 0..data_len {
                if i == buf_len {
                    break;
                }
                data[j] = buf[i];
                i += 1;
            }

            addr = match block.next() {
                Some(next_block) => {
                    if i < buf_len {
                        next_block.addr()
                    } else {
                        // TODO: Free the next block(s)
                        0
                    }
                }
                None => {
                    if i < buf_len {
                        match Block::alloc() {
                            Some(next_block) => next_block.addr(),
                            None => return Err(()),
                        }
                    } else {
                        0
                    }
                }
            };

            block.set_next(addr);
            block.write();
        }
        self.size = i as u32;
        self.dir.update_entry_size(&self.name, self.size);
        Ok(())
    }

    pub fn addr(&self) -> u32 {
        self.addr
    }

    pub fn delete(pathname: &str) -> Result<(), ()> {
        let pathname = realpath(pathname);
        let dirname = dirname(&pathname);
        let filename = filename(&pathname);
        if let Some(mut dir) = Dir::open(dirname) {
            dir.delete_entry(filename)
        } else {
            Err(())
        }
    }
}

#[derive(Clone)]
pub struct Block {
    addr: u32,
    buf: [u8; 512],
}

// Block structure:
// 0..4 => next block address
// 4..512 => block data
impl Block {
    pub fn new(addr: u32) -> Self {
        let buf = [0; 512];
        Self { addr, buf }
    }

    pub fn read(addr: u32) -> Self {
        let mut buf = [0; 512];
        if let Some(ref block_device) = *BLOCK_DEVICE.lock() {
            block_device.read(addr, &mut buf);
        }
        Self { addr, buf }
    }

    pub fn alloc() -> Option<Self> {
        match BlockBitmap::next_free_addr() {
            None => {
                return None;
            }
            Some(addr) => {
                BlockBitmap::alloc(addr);

                // Initialize block
                let mut block = Block::read(addr);
                for i in 0..512 {
                    block.buf[i] = 0;
                }
                block.write();

                Some(block)
            }
        }
    }

    pub fn write(&self) {
        if let Some(ref block_device) = *BLOCK_DEVICE.lock() {
            block_device.write(self.addr, &self.buf);
        }
    }

    pub fn addr(&self) -> u32 {
        self.addr
    }

    pub fn data(&self) -> &[u8] {
        &self.buf[4..512]
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.buf[4..512]
    }

    // TODO: Return addr instead of block?
    pub fn next(&self) -> Option<Self> {
        let addr = (self.buf[0] as u32) << 24
                 | (self.buf[1] as u32) << 16
                 | (self.buf[2] as u32) << 8
                 | (self.buf[3] as u32);

        if addr == 0 {
            None
        } else {
            Some(Self::read(addr))
        }
    }

    // FIXME: next() returns a Block, but set_next() takes a u32
    pub fn set_next(&mut self, addr: u32) {
        self.buf[0] = addr.get_bits(24..32) as u8;
        self.buf[1] = addr.get_bits(16..24) as u8;
        self.buf[2] = addr.get_bits(8..16) as u8;
        self.buf[3] = addr.get_bits(0..8) as u8;
    }
}

const BITMAP_SIZE: u32 = 512 - 4; // TODO: Bitmap should use the full block
const MAX_BLOCKS: u32 = 2 * 2048;

const DISK_OFFSET: u32 = (1 << 20) / 512;
const SUPERBLOCK_ADDR: u32 = DISK_OFFSET;
const BITMAP_ADDR_OFFSET: u32 = DISK_OFFSET + 2;
const DATA_ADDR_OFFSET: u32 = BITMAP_ADDR_OFFSET + MAX_BLOCKS / 8;

/* Disk Areas
 * 1 => Reserved
 * 2 => Bitmap (allocated blocks (1 bit per block)
 * 3 => Data (directories and files)
 */

// A BlockBitmap store the allocation status of (512 - 4) * 8 data blocks
pub struct BlockBitmap {}

impl BlockBitmap {
    fn block_index(data_addr: u32) -> u32 {
        let i = data_addr - DATA_ADDR_OFFSET;
        BITMAP_ADDR_OFFSET + (i / BITMAP_SIZE / 8)
    }

    fn buffer_index(data_addr: u32) -> usize {
        let i = data_addr - DATA_ADDR_OFFSET;
        (i % BITMAP_SIZE) as usize
    }

    pub fn is_free(addr: u32) -> bool {
        let block = Block::read(BlockBitmap::block_index(addr));
        let bitmap = block.data(); // TODO: Add block.buffer()
        let i = BlockBitmap::buffer_index(addr);
        bitmap[i / 8].get_bit(i % 8)
    }

    pub fn alloc(addr: u32) {
        let mut block = Block::read(BlockBitmap::block_index(addr));
        let bitmap = block.data_mut();
        let i = BlockBitmap::buffer_index(addr);
        bitmap[i / 8].set_bit(i % 8, true);
        block.write();
    }

    pub fn free(addr: u32) {
        let mut block = Block::read(BlockBitmap::block_index(addr));
        let bitmap = block.data_mut();
        let i = BlockBitmap::buffer_index(addr);
        bitmap[i / 8].set_bit(i % 8, false);
        block.write();
    }

    pub fn next_free_addr() -> Option<u32> {
        let n = MAX_BLOCKS / BITMAP_SIZE / 8;
        for i in 0..n {
            let block = Block::read(BITMAP_ADDR_OFFSET + i);
            let bitmap = block.data();
            for j in 0..BITMAP_SIZE {
                for k in 0..8 {
                    if !bitmap[j as usize].get_bit(k) {
                        let addr = DATA_ADDR_OFFSET + i * 512 * 8 + j * 8 + k as u32;
                        return Some(addr);
                    }
                }
            }
        }
        None
    }
}

#[derive(Clone)]
pub struct DirEntry {
    dir: Dir,
    kind: FileType,
    addr: u32,
    size: u32,
    name: String,
}

impl DirEntry {
    pub fn new(dir: Dir, kind: FileType, addr: u32, size: u32, name: &str) -> Self {
        let name = String::from(name);
        Self { dir, kind, addr, size, name }
    }

    pub fn is_dir(&self) -> bool {
        self.kind == FileType::Dir
    }

    pub fn is_file(&self) -> bool {
        self.kind == FileType::File
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn to_dir(&self) -> Dir {
        assert!(self.kind == FileType::Dir);
        Dir { addr: self.addr }
    }

    pub fn to_file(&self) -> File {
        assert!(self.kind == FileType::File);
        File {
            name: self.name.clone(),
            addr: self.addr,
            size: self.size,
            dir: self.dir,
        }
    }

    pub fn len(&self) -> usize {
        1 + 4 + 4 + 1 + self.name.len()
    }
}

#[derive(Clone, Copy)]
pub struct Dir {
    addr: u32,
}

impl Dir {
    pub fn root() -> Self {
        Self { addr: DATA_ADDR_OFFSET }
    }

    pub fn create(pathname: &str) -> Option<Self> {
        let pathname = realpath(pathname);
        let dirname = dirname(&pathname);
        let filename = filename(&pathname);
        if let Some(dir) = Dir::open(dirname) {
            if let Some(dir_entry) = dir.create_dir(filename) {
                return Some(dir_entry.to_dir());
            }
        }
        None
    }

    pub fn open(pathname: &str) -> Option<Self> {
        let pathname = realpath(pathname);
        let mut dir = Dir::root();

        if !is_mounted() {
            return None;
        }

        if pathname == "/" {
            return Some(dir);
        }

        for name in pathname.trim_start_matches('/').split('/') {
            match dir.find(name) {
                Some(dir_entry) => {
                    if dir_entry.is_dir() {
                        dir = dir_entry.to_dir()
                    } else {
                        return None;
                    }
                },
                None => {
                    return None
                },
            }
        }
        Some(dir)
    }

    pub fn addr(&self) -> u32 {
        self.addr
    }

    pub fn find(&self, name: &str) -> Option<DirEntry> {
        for entry in self.read() {
            if entry.name == name {
                return Some(entry);
            }
        }
        None
    }

    pub fn create_file(&self, name: &str) -> Option<DirEntry> {
        self.create_entry(FileType::File, name)
    }

    pub fn create_dir(&self, name: &str) -> Option<DirEntry> {
        self.create_entry(FileType::Dir, name)
    }

    fn create_entry(&self, kind: FileType, name: &str) -> Option<DirEntry> {
        if self.find(name).is_some() {
            return None;
        }

        let mut read_dir = self.read();
        while read_dir.next().is_some() {}

        if read_dir.block.data().len() - read_dir.data_offset < name.len() + 10 {
            let new_block = Block::alloc().unwrap(); // TODO
            read_dir.block.set_next(new_block.addr);
            read_dir.block.write();
            read_dir.block = new_block;
            read_dir.data_offset = 0;
        }

        let new_block = Block::alloc().unwrap();

        let entry_kind = kind;
        let entry_size = 0;
        let entry_addr = new_block.addr();
        let entry_name = name.as_bytes();

        let n = entry_name.len();
        let i = read_dir.data_offset;
        let data = read_dir.block.data_mut();
        data[i + 0] = entry_kind as u8;
        data[i + 1] = entry_addr.get_bits(24..32) as u8;
        data[i + 2] = entry_addr.get_bits(16..24) as u8;
        data[i + 3] = entry_addr.get_bits(8..16) as u8;
        data[i + 4] = entry_addr.get_bits(0..8) as u8;
        data[i + 5] = entry_size.get_bits(24..32) as u8;
        data[i + 6] = entry_size.get_bits(16..24) as u8;
        data[i + 7] = entry_size.get_bits(8..16) as u8;
        data[i + 8] = entry_size.get_bits(0..8) as u8;
        data[i + 9] = n as u8;
        for j in 0..n {
            data[i + 10 + j] = entry_name[j];
        }
        read_dir.block.write();

        Some(DirEntry::new(self.clone(), kind, entry_addr, entry_size, name))
    }

    // Deleting an entry is done by setting the entry address to 0
    // TODO: If the entry is a directory, remove its entries recursively
    pub fn delete_entry(&mut self, name: &str) -> Result<(), ()> {
        let mut read_dir = self.read();
        for entry in &mut read_dir {
            if entry.name == name {
                // Zeroing entry addr
                let data = read_dir.block.data_mut();
                let i = read_dir.data_offset - entry.len();
                data[i + 1] = 0;
                data[i + 2] = 0;
                data[i + 3] = 0;
                data[i + 4] = 0;
                read_dir.block.write();

                // Freeing entry blocks
                let mut entry_block = Block::read(entry.addr);
                loop {
                    BlockBitmap::free(entry_block.addr);
                    match entry_block.next() {
                        Some(next_block) => entry_block = next_block,
                        None => break,
                    }
                }

                return Ok(());
            }
        }
        Err(())
    }

    fn update_entry_size(&mut self, name: &str, size: u32) {
        let mut read_dir = self.read();
        for entry in &mut read_dir {
            if entry.name == name {
                let data = read_dir.block.data_mut();
                let i = read_dir.data_offset - entry.len();
                data[i + 5] = size.get_bits(24..32) as u8;
                data[i + 6] = size.get_bits(16..24) as u8;
                data[i + 7] = size.get_bits(8..16) as u8;
                data[i + 8] = size.get_bits(0..8) as u8;
                read_dir.block.write();
                break;
            }
        }
    }

    pub fn read(&self) -> ReadDir {
        ReadDir {
            dir: self.clone(),
            block: Block::read(self.addr),
            data_offset: 0,
        }
    }

    pub fn delete(pathname: &str) -> Result<(), ()> {
        let pathname = realpath(pathname);
        let dirname = dirname(&pathname);
        let filename = filename(&pathname);
        if let Some(mut dir) = Dir::open(dirname) {
            dir.delete_entry(filename)
        } else {
            Err(())
        }
    }
}

pub struct ReadDir {
    dir: Dir,
    block: Block,
    data_offset: usize,
}

impl Iterator for ReadDir {
    type Item = DirEntry;

    fn next(&mut self) -> Option<DirEntry> {
        loop {
            let data = self.block.data();
            let mut i = self.data_offset;

            loop {
                if i == data.len() - 10 { // No space left for another entry in the block
                    break;
                }

                let entry_kind = match data[i + 0] {
                    0 => FileType::Dir,
                    1 => FileType::File,
                    _ => break,
                };
                let entry_addr = (data[i + 1] as u32) << 24
                               | (data[i + 2] as u32) << 16
                               | (data[i + 3] as u32) << 8
                               | (data[i + 4] as u32);
                let entry_size = (data[i + 5] as u32) << 24
                               | (data[i + 6] as u32) << 16
                               | (data[i + 7] as u32) << 8
                               | (data[i + 8] as u32);
                i += 9;

                let mut n = data[i];
                if n == 0 || n as usize >= data.len() - i {
                    break;
                }
                i += 1;

                // The rest of the entry is the pathname string.
                let mut entry_name = String::new();
                loop {
                    if n == 0 {
                        break;
                    }
                    entry_name.push(data[i] as char);
                    n -= 1;
                    i += 1;
                }

                self.data_offset = i;

                // Skip deleted entries
                if entry_addr == 0 {
                    continue;
                }

                return Some(DirEntry::new(self.dir, entry_kind, entry_addr, entry_size, &entry_name));
            }

            match self.block.next() {
                Some(next_block) => {
                    self.block = next_block;
                    self.data_offset = 0;
                }
                None => break,
            }
        }

        None
    }
}

pub struct BlockDevice {
    bus: u8,
    dsk: u8,
}

impl BlockDevice {
    pub fn new(bus: u8, dsk: u8) -> Self {
        Self { bus, dsk }
    }

    pub fn read(&self, block: u32, mut buf: &mut [u8]) {
        sys::ata::read(self.bus, self.dsk, block, &mut buf);
    }

    pub fn write(&self, block: u32, buf: &[u8]) {
        sys::ata::write(self.bus, self.dsk, block, &buf);
    }
}

pub fn is_mounted() -> bool {
    BLOCK_DEVICE.lock().is_some()
}

pub fn mount(bus: u8, dsk: u8) {
    let block_device = BlockDevice::new(bus, dsk);
    *BLOCK_DEVICE.lock() = Some(block_device);
}

pub fn format(bus: u8, dsk: u8) {
    // Write superblock
    let mut buf = MAGIC.as_bytes().to_vec();
    buf.resize(512, 0);
    let block_device = BlockDevice::new(bus, dsk);
    block_device.write(SUPERBLOCK_ADDR, &buf);

    mount(bus, dsk);

    // Allocate root dir
    let root = Dir::root();
    BlockBitmap::alloc(root.addr());
}

pub fn init() {
    for bus in 0..2 {
        for dsk in 0..2 {
            let mut buf = [0u8; 512];
            sys::ata::read(bus, dsk, SUPERBLOCK_ADDR, &mut buf);
            if let Ok(header) = String::from_utf8(buf[0..8].to_vec()) {
                if header == MAGIC {
                    log!("MFS Superblock found in ATA {}:{}\n", bus, dsk);
                    mount(bus, dsk);
                }
            }
        }
    }
}