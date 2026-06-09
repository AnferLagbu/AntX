#![deny(unsafe_code)]
//! QueenX Services 层 — 100% safe Rust (去特权)
//!
//! **禁止** 包含任何 `unsafe` 代码。
//! 所有硬件交互通过 `kernel::framework` 的安全 API 进行。
//!
//! ## 架构 (框内核)
//!
//! ```text
//! framework/ (TCB, unsafe 允许)     ← 唯一 unsafe 位置
//!     ↓ 安全函数调用 (零开销)
//! services/ (本模块)               ← 100% safe Rust
//!     ↓ 系统调用
//! 用户态                             ← Ring 3
//! ```
//!
//! ## Safe Rust 契约
//!
//! 本模块中的每个文件必须在文件头部声明:
//! ```rust
//! //! @SAFE: 本文件不含 unsafe 代码。
//! //! 所有 unsafe 操作已委托至 framework API。
//! ```
//!
//! CI 检查: `grep -rn 'unsafe ' src/kernel/services/` 必须输出为空。
//!
//! ## 检查脚本
//!
//! ```bash
//! tools/check_tcb.sh  # 自动检查 services/ 中无 unsafe
//! ```

// ============================================================================
// 子系统声明
// ============================================================================

/// 系统调用 — POSIX + Credo 分发 (通过 framework::UserContext)
pub mod syscall;

/// 进程管理 — 调度 / 进程表 / ELF 加载
pub mod proc;

/// 文件系统 — VFS + ramfs + HvFS + devfs + procfs
pub mod fs;

/// 网络栈 — smoltcp + 驱动适配
pub mod net;

/// 进程间通信 — 管道 / 共享内存 / 消息队列 / 信号
pub mod ipc;

/// 设备驱动框架 — Chitin 协议族
pub mod chitin;

/// 设备驱动 — 网卡 / 存储 / 显示 / 输入
pub mod driver;

/// 身份与权限 — PWM / 能力矩阵 / 会话
pub mod credo;

/// 故障恢复 — 栏栈恢复
pub mod barrier;

/// 同步原语高级封装 — IrqSpinLock / scoped / Barrier / Once
///
/// 基础同步原语见 `framework::sync` (TCB); 本模块提供
/// services 层的安全抽象 (闭包 API / 一次性初始化等)。
pub mod sync;

/// 内存管理 — Page Cache / Swap / mmap safe 代理
///
/// 底层实现见 `framework::mm` (TCB); 本模块提供
/// services 层的安全抽象与参数验证。
pub mod mm;

/// 存储子系统 — 块设备列表/信息/格式化/分区 (Credo 私有 syscall safe 代理)
///
/// 底层实现见 `framework::driver::block` (TCB); 本模块提供
/// services 层的安全抽象与参数验证 (credo 鉴权 + 用户指针 + 容量检查)。
pub mod storage;

/// init 启动子系统 — PID 1 / initramfs / Ring 3 切换状态查询
///
/// 底层实现见 `framework::proc::api` (TCB); 本模块提供
/// services 层的安全状态查询 API, 不暴露 unsafe 入口。
pub mod init;

/// WASM 沙箱
pub mod wasm;

/// 内核调试 / 跟踪 — ftrace / KGDB 安全封装
///
/// 底层实现见 `framework::debug` (TCB) 与 `framework::syscall::ftrace_kgdb` (TCB);
/// 本模块提供 services 层的安全抽象与系统调用包装 (0 unsafe)。
pub mod debug;

// ============================================================================
// CI 自检 (编译时)
// ============================================================================

/// 编译时断言: services 层禁止 unsafe。
///
/// 实际检查由 `tools/check_tcb.sh` 在 CI 中完成。
/// 此常量仅用于文档目的。
pub const SERVICES_SAFE_RUST: bool = true;
