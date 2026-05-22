pub mod vfs;
pub mod ramfs;
#[cfg(target_arch = "x86_64")]
pub mod hvfs;
pub mod devfs;
pub mod procfs;

pub use vfs::*;
