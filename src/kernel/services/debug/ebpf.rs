#![deny(unsafe_code)]
//! eBPF 安全代理 — services 层 (0 unsafe)
//!
//! 封装 `framework::debug::ebpf` 的安全 API.

// 重导出强类型
pub use crate::kernel::framework::debug::{
    BpfInsn, BpfMapType, BpfProgType,
    BpfMapDef, BpfMap, BpfProg, BpfSubsystem, BpfVerifier, BpfInterpreter,
    BpfCtx, BpfHelper,
    BPF_MAX_INSNS, BPF_MAX_MAPS, BPF_MAX_PROGS, BPF_REG_NUM, BPF_STACK_SIZE,
};

use crate::kernel::framework::debug::{
    bpf_init, bpf_is_initialized, bpf_subsystem, sys_bpf,
};

/// 初始化 eBPF 子系统
pub fn init() {
    bpf_init();
}

/// eBPF 是否已初始化
pub fn is_initialized() -> bool {
    bpf_is_initialized()
}

/// 获取全局 BPF 子系统
pub fn subsystem() -> &'static BpfSubsystem {
    bpf_subsystem()
}

/// BPF 系统调用 (安全封装)
pub fn bpf_syscall(cmd: u64, attr: u64, size: u64) -> i64 {
    sys_bpf(cmd, attr, size)
}
