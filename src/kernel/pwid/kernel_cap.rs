//! PWID v5 Kernel Capability Definitions
//!
//! Kernel privilege layer, independent from user capabilities.
//! Kernel caps are never exposed to user space.

pub const KERNEL_PRIVILEGE: u8 = 0xFF;

pub const KERNEL_CAP_MEMORY: u64    = 1 << 0;
pub const KERNEL_CAP_INTERRUPT: u64 = 1 << 1;
pub const KERNEL_CAP_SCHEDULER: u64 = 1 << 2;
pub const KERNEL_CAP_DEVICE: u64    = 1 << 3;
pub const KERNEL_CAP_IPC: u64       = 1 << 4;
pub const KERNEL_CAP_BARRIER: u64   = 1 << 5;

pub const KERNEL_CAP_ALL: u64 =
    KERNEL_CAP_MEMORY | KERNEL_CAP_INTERRUPT |
    KERNEL_CAP_SCHEDULER | KERNEL_CAP_DEVICE |
    KERNEL_CAP_IPC | KERNEL_CAP_BARRIER;

#[repr(u8)]
pub enum KernelCapDomain {
    MemoryMgmt = 0,
    Interrupt = 1,
    Scheduler = 2,
    Device = 3,
    Ipc = 4,
    Barrier = 5,
}
