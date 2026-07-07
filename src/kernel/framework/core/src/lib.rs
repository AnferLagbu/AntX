#![no_std]
//! QueenX Core — 共享类型和基础功能

extern crate alloc;

// 共享类型
pub mod types {
    /// 进程 ID
    pub type Pid = u32;
    /// 线程 ID
    pub type Tid = u32;
}

// 共享常量
pub mod constants {
    /// 页大小
    pub const PAGE_SIZE: u64 = 4096;
    /// 页大小 shift
    pub const PAGE_SHIFT: u64 = 12;
}