//! 文件系统 — services 层 (Phase 2.2 完成 ✓)
//!
//! ## 真实状态 (v2.5, 2026-06-04)
//!
//! 已完成 4/4 子系统迁移:
//! - [ramfs]  — RamFS 内存文件系统安全代理 (100% safe API, 0 unsafe)
//! - [devfs]  — DevFS 设备文件系统安全代理 (100% safe API, 0 unsafe)
//! - [procfs] — ProcFS 进程文件系统安全代理 (100% safe API, 0 unsafe)
//! - [hvfs]   — HvFS 磁盘文件系统安全代理 (100% safe API, 0 unsafe)
//!
//! ## 迁移方法
//!
//! 1. 把内核 `i32` 错误码 → `Result<_, FsError>` (services 层类型化)
//! 2. 把 `*const u8`/`*mut u8` 用户指针 → `&[u8]`/`&mut [u8]` 切片
//! 3. 把硬编码路径/标志 → 引入 `VfsOpenFlags`/`VfsSeekWhence` 等强类型
//! 4. 0 unsafe 出现在 services 层
//!
//! 评估日期: 2026-06-04

pub mod ramfs;
pub mod devfs;
pub mod procfs;
pub mod hvfs;
