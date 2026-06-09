//! debug 模块公共 re-export

pub use super::ftrace::{
    fnv1a_32, FtraceState, TraceEvent, EVENT_SIZE, FTRACE_BUF_CAP, MAX_TRACE_POINTS, FTRACE,
};
pub use super::kgdb::{
    kgdb_active, kgdb_breakpoint, kgdb_handle_exception, kgdb_loop, kgdb_serial_ready,
    kgdb_set_serial, kgdb_try_getc, kgdb_write_str, KgdbRegs, KgdbSerial,
};
pub use super::ringbuf::{RingBuffer, DEFAULT_RING_CAPACITY};

/// 初始化 debug 子系统
pub fn debug_init() {
    super::ftrace::ftrace_init();
}

/// 启用 ftrace 全局开关
pub fn ftrace_enable() {
    FTRACE.enable();
}

/// 禁用 ftrace 全局开关
pub fn ftrace_disable() {
    FTRACE.disable();
}

/// 查询 ftrace 启用状态
pub fn ftrace_is_enabled() -> bool {
    FTRACE.is_enabled()
}

/// 累计事件计数
pub fn ftrace_event_count() -> u64 {
    FTRACE.event_count()
}

/// 累计溢出计数
pub fn ftrace_overflow_count() -> u64 {
    FTRACE.overflow_count()
}

/// 弹出一条事件 (None = 空)
pub fn ftrace_pop_event() -> Option<TraceEvent> {
    FTRACE.pop()
}

/// 注册一个跟踪点 (按 name hash 查重, 返回是否成功)
pub fn ftrace_register_point(name_hash: u32) -> bool {
    FTRACE.register_point(name_hash)
}

/// KGDB 主动断点入口 (panic 路径调用)
pub fn kgdb_break_now() {
    let mut regs = KgdbRegs::default();
    kgdb_breakpoint(&mut regs);
}

