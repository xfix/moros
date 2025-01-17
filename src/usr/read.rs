use crate::{api, sys, usr};
use crate::api::fs;
use crate::api::syscall;
use crate::sys::cmos::CMOS;
use alloc::borrow::ToOwned;
use alloc::vec::Vec;

pub fn main(args: &[&str]) -> usr::shell::ExitCode {
    if args.len() != 2 {
        return usr::shell::ExitCode::CommandError;
    }

    let pathname = args[1];

    match pathname {
        "/dev/rtc" => {
            let rtc = CMOS::new().rtc();
            println!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                rtc.year, rtc.month, rtc.day,
                rtc.hour, rtc.minute, rtc.second
            );
            usr::shell::ExitCode::CommandSuccessful
        },
        "/dev/clk/realtime" => {
            println!("{:.6}", syscall::realtime());
            usr::shell::ExitCode::CommandSuccessful
        },
        "/dev/clk/uptime" => {
            println!("{:.6}", syscall::uptime());
            usr::shell::ExitCode::CommandSuccessful
        },
        _ => {
            if pathname.starts_with("/net/") {
                // Examples:
                // > read /net/http/example.com/articles
                // > read /net/http/example.com:8080/articles/index.html
                // > read /net/daytime/time.nist.gov
                // > read /net/tcp/time.nist.gov:13
                let parts: Vec<_> = pathname.split('/').collect();
                if parts.len() < 4 {
                    println!("Usage: read /net/http/<host>/<path>");
                    usr::shell::ExitCode::CommandError
                } else {
                    match parts[2] {
                        "tcp" => {
                            let host = parts[3];
                            usr::tcp::main(&["tcp", host])
                        }
                        "daytime" => {
                            let host = parts[3];
                            let port = "13";
                            usr::tcp::main(&["tcp", host, port])
                        }
                        "http" => {
                            let host = parts[3];
                            let path = "/".to_owned() + &parts[4..].join("/");
                            usr::http::main(&["http", host, &path])
                        }
                        _ => {
                            println!("Error: unknown protocol '{}'", parts[2]);
                            usr::shell::ExitCode::CommandError
                        }
                    }
                }
            } else if let Some(stat) = syscall::stat(pathname) {
                if stat.is_file() {
                    if let Ok(contents) = api::fs::read_to_string(pathname) {
                        print!("{}", contents);
                        usr::shell::ExitCode::CommandSuccessful
                    } else {
                        println!("Could not read '{}'", pathname);
                        usr::shell::ExitCode::CommandError
                    }
                } else if stat.is_dir() {
                    usr::list::main(args)
                } else if stat.is_device() {
                    loop {
                        if let Ok(bytes) = fs::read(pathname) {
                            print!("{}", bytes[0] as char);
                        }
                        if sys::console::end_of_text() {
                            println!();
                            return usr::shell::ExitCode::CommandSuccessful;
                        }
                    }
                } else {
                    println!("Could not read type of '{}'", pathname);
                    usr::shell::ExitCode::CommandError
                }
            } else {
                println!("File not found '{}'", pathname);
                usr::shell::ExitCode::CommandError
            }
        }
    }
}
