#![deny(unsafe_code)]
//! WASM 沙箱 (services 层占位)
//!
//! ## 当前状态: ⏳ 未迁移 (Phase 7 未完成)
//!
//! 实际实现仍在 `kernel/wasm/` 老位置:
//! - [kernel/wasm/interpreter.rs](file:///home/anfer/Code/AntX/src/kernel/wasm/interpreter.rs) — 解释器
//! - [kernel/wasm/runtime.rs](file:///home/anfer/Code/AntX/src/kernel/wasm/runtime.rs) — 运行时
//! - [kernel/wasm/module.rs](file:///home/anfer/Code/AntX/src/kernel/wasm/module.rs) — 模块加载
//! - [kernel/wasm/types.rs](file:///home/anfer/Code/AntX/src/kernel/wasm/types.rs) — 类型
//! - [kernel/wasm/leb128.rs](file:///home/anfer/Code/AntX/src/kernel/wasm/leb128.rs) — LEB128 解码
//!
//! ## 迁移路径
//!
//! 1. 引入 `wasmi` (no_std 兼容) 或自己实现
//! 2. WASM 线性内存 = `framework::VmSpace` 实例
//! 3. WASM 实例 = `framework::sched::Task` 子类
//! 4. 在 services/wasm/ 暴露 `pub fn instantiate`, `pub fn invoke` 等纯 safe API
//!
//! ## 估算: 1 人月
//!
//! 评估日期: 2026-06-03
//! 阻塞点: 依赖 Phase 2.3 进程管理迁移完成
