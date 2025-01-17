use crate::{sys, usr};
use crate::api::syscall;
use time::{OffsetDateTime, Duration, UtcOffset};

pub fn main(args: &[&str]) -> usr::shell::ExitCode {
    let seconds = syscall::realtime(); // Since Unix Epoch
    let nanoseconds = libm::floor(1e9 * (seconds - libm::floor(seconds))) as i64;
    let date = OffsetDateTime::from_unix_timestamp(seconds as i64).to_offset(offset())
             + Duration::nanoseconds(nanoseconds);

    let format = if args.len() > 1 { args[1] } else { "%FT%H:%M:%S" };
    match time::util::validate_format_string(format) {
        Ok(()) => {
            println!("{}", date.format(format));
            usr::shell::ExitCode::CommandSuccessful
        }
        Err(e) => {
            println!("Error: {}", e);
            usr::shell::ExitCode::CommandError
        }
    }
}

fn offset() -> UtcOffset {
    if let Some(tz) = sys::process::env("TZ") {
        if let Ok(offset) = tz.parse::<i32>() {
            return UtcOffset::seconds(offset);
        }
    }
    UtcOffset::seconds(0)
}
