//! 文件系统 — VFS + ramfs + HvFS + devfs + procfs (services 层占位)
//!
//! ## 当前状态: ⏳ 未迁移
//!
//! 实际实现仍在 `kernel/fs/` 老位置:
//! - [kernel/fs/vfs/](file:///home/anfer/Code/AntX/src/kernel/fs/vfs/) — VFS trait + 统一接口
//! - [kernel/fs/ramfs/](file:///home/anfer/Code/AntX/src/kernel/fs/ramfs/) — 内存 FS
//! - [kernel/fs/hvfs/](file:///home/anfer/Code/AntX/src/kernel/fs/hvfs/) — HvFS v2 (SPA/DMU/ZAP/TXG/ZIL/ARC/RAIDZ)
//! - [kernel/fs/devfs/](file:///home/anfer/Code/AntX/src/kernel/fs/devfs/) — 设备 FS
//! - [kernel/fs/procfs/](file:///home/anfer/Code/AntX/src/kernel/fs/procfs/) — 进程 FS
//!
//! ## 迁移路径
//!
//! 1. 引入 `framework::vmspace::VmSpace` 处理 page cache 映射
//! 2. ramfs 33 unsafe 行 → 全部走 `Frame::from_raw` / `Frame::as_virt_ptr`
//! 3. HvFS 16 unsafe 行 → 走 `framework::dma::DmaStream`
//! 4. 在 services/fs/ 暴露 `pub fn mount`, `pub fn open` 等纯 safe API
//!
//! ## 估算: 1.5 人月
//!
//! 评估日期: 2026-06-03
//! 注意: HvFS 磁盘挂载端到端路径在 [KNOWN_ISSUES Issue #3](file:///home/anfer/Code/AntX/docs/development/KNOWN_ISSUES.md) 标记为未测试
