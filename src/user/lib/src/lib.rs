#![no_std]

pub mod sys;
pub mod io;
pub mod str;
pub mod fs;
pub mod install_wizard;

pub use sys::*;
pub use io::{print, println, print_char, print_hex, print_dec, read_line};
pub use str::{cmp, parse_args};
pub use fs::{file_open, file_copy};

pub fn delay_loop(count: u64) {
    for _ in 0..count { core::hint::spin_loop(); }
}
