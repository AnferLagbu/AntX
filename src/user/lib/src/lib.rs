#![no_std]

pub mod sys;
pub mod io;
pub mod str;
pub mod fs;

// TODO(deprecation): Replace `pub use sys::*` with selective re-exports.
// Currently exports ~50 constants, types, and functions into the global
// `userlib::` namespace. Future crates should import from `userlib::sys`
// directly (e.g., `use userlib::sys::{self, UserDiskInfo}`).
pub use sys::*;
pub use io::{print, println, print_char, print_hex, print_dec, read_line};
pub use str::{cmp, parse_args};
pub use fs::{file_open, file_copy};

pub fn delay_loop(count: u64) {
    for _ in 0..count { core::hint::spin_loop(); }
}
