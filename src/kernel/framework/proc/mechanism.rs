//! 进程管理 — 机制 API 集中导出
//!
//! L-02: 将 framework/proc 的纯机制函数集中导出, 供 services 层策略实现调用.
//!
//! **机制 (Mechanism)**: 直接操作硬件或全局数据结构的底层操作:
//! - 页表切换 (CR3 切换)
//! - 内核栈复制
//! - 上下文切换
//! - 进程表 CRUD (PCB 分配/释放/插入/查找)
//! - 调度器队列操作 (add/block/unblock/schedule)
//! - 用户进程加载/进入
//!
//! **策略 (Policy)**: 决定"何时/如何"使用机制:
//! - fork 的 COW 决策
//! - execve 的地址空间替换策略
//! - 调度优先级计算
//! - 进程创建/退出的资源管理策略
//!
//! services 层通过 `use framework::proc::mechanism::*` 获取机制 API,
//! 在 services 层实现策略逻辑.

// ==================== 页表操作 ====================

pub use super::api::raw::switch_page_table;
pub use super::api::raw::clone_user_page_table_cow;
pub use super::api::raw::destroy_user_page_table;

// ==================== 进程分配/释放 ====================

pub use super::api::raw::alloc_process;
pub use super::api::raw::dealloc_process;
pub use super::api::raw::drop_boxed_process;
pub use super::api::raw::process_ref;
pub use super::api::raw::process_ref_mut;

// ==================== 内核栈操作 ====================

pub use super::api::raw::copy_kstack;

// ==================== 进程表操作 ====================

pub use super::api::process_exists;
pub use super::api::process_try_inc_ref;
pub use super::api::process_dec_ref;
pub use super::api::process_get_cr3;
pub use super::api::process_get_pwm;
pub use super::api::process_with;
pub use super::api::process_with_mut;
pub use super::api::process_for_each;
pub use super::api::process_get_raw;
pub use super::api::process_remove_and_free;
pub use super::api::process_insert;

// ==================== 调度器操作 ====================

pub use super::api::scheduler_add_to_run_queue;
pub use super::api::scheduler_unblock;
pub use super::api::scheduler_yield;
pub use super::api::scheduler_yield_ex;
pub use super::api::scheduler_schedule;
pub use super::api::scheduler_add;
pub use super::api::scheduler_current_cputime;

// ==================== 用户进程操作 ====================

pub use super::api::user_proc_load_elf;
pub use super::api::user_proc_load_elf_from_memory;
pub use super::api::user_proc_enter_by_pid;
pub use super::api::user_proc_setup_argv;
pub use super::api::proc_alloc_pid;

// ==================== 进程信息查询 ====================

pub use super::api::process_get_current;
pub use super::api::process_get_current_pid;
pub use super::api::process_get_current_pwm;
pub use super::api::process_get_by_pid;
pub use super::api::process_get_pwm_by_pid;
pub use super::api::process_find_by_pid;
pub use super::api::update_current_process_ptr;

// ==================== 信号操作 ====================

pub use super::api::process_signal_pending_set;

// ==================== 进程状态操作 ====================

pub use super::api::proc_set_in_kern;
pub use super::api::proc_get_in_kern;
pub use super::api::proc_get_state;
pub use super::api::proc_set_priority;
pub use super::api::proc_get_exit_code;
pub use super::api::proc_is_initialized;

// ==================== 初始化 ====================

pub use super::api::scheduler_init;
pub use super::api::process_init;
pub use super::api::thread_init;
pub use super::api::session_init;
pub use super::api::user_proc_init;
pub use super::api::scheduler_tick;
