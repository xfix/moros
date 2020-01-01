use lazy_static::lazy_static;
use heapless::{String, LinearMap};
use heapless::consts::*;
use spin::Mutex;

lazy_static! {
    pub static ref FS: Mutex<LinearMap<String<U32>, File, U2048>> = Mutex::new(LinearMap::new());
}

#[derive(Clone)]
pub struct File {
    pathname: String<U32>,
    contents: String<U2048>,
}

impl File {
    pub fn create(pathname: &str) -> Option<Self> {
        Some(File {
            pathname: String::from(pathname),
            contents: String::new()
        })
    }

    pub fn open(pathname: &str) -> Option<Self> {
        let fs = FS.lock();
        if let Some(file) = fs.get(&String::from(pathname)) {
            Some(file.clone())
        } else {
            None
        }
    }

    pub fn read(&self) -> String<U2048> {
        self.contents.clone()
    }

    pub fn write(&mut self, chunk: &str) {
        let mut fs = FS.lock();
        self.contents.push_str(chunk).ok(); // TODO: File full
        fs.insert(String::from(self.pathname.clone()), self.clone()).ok(); // TODO: Disk full
    }
}