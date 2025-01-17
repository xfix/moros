use crate::{sys, usr};
use crate::api::syscall;
use crate::api::console::Style;

pub fn main(_args: &[&str]) -> usr::shell::ExitCode {
    let csi_color = Style::color("Yellow");
    let csi_reset = Style::reset();
    println!("{}MOROS has reached its fate, the system is now halting.{}", csi_color, csi_reset);
    sys::acpi::shutdown();
    loop { syscall::sleep(1.0) }
}
