// TD-06: 验证 smoltcp FD 容量可由 `cfg_smoltcp_cap()` 派生.
//
// 验收:
//   1. `fd_alloc::cfg_smoltcp_cap()` 必须存在, 返回 u16
//   2. `framework/net/init.rs` 的 `MAX_SOCKETS` 必须从 `cfg_smoltcp_cap()` 派生 (而非硬编码 256)
//   3. `MAX_SM_FD` 与 `FdPlan::SMOLTCP.capacity` 仍为单一来源 (MAX_SOCKETS == MAX_SM_FD 隐含)
//   4. 默认容量 = 256 (与 I-47 G_MAX_SOCKETS 初始 DEFAULT_MAX_SOCKETS 一致)

use std::fs;
use std::path::Path;

const FD_ALLOC: &str = "src/kernel/services/proc/fd_alloc.rs";
const NET_INIT: &str = "src/kernel/framework/net/init.rs";

fn read(path: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(path);
    fs::read_to_string(&p).unwrap_or_else(|_| panic!("读 {}", path))
}

#[test]
fn test_cfg_smoltcp_cap_exists() {
    let src = read(FD_ALLOC);
    assert!(src.contains("pub const fn cfg_smoltcp_cap() -> u16"),
        "TD-06: fd_alloc 必须暴露 `pub const fn cfg_smoltcp_cap() -> u16` 派生函数");
    // 默认 256 — 默认与 I-47 G_MAX_SOCKETS 的 DEFAULT_MAX_SOCKETS 一致
    assert!(src.contains("256\n}"),
        "TD-06: cfg_smoltcp_cap 默认值应保持 256 (与 I-47 DEFAULT_MAX_SOCKETS 对齐)");
}

#[test]
fn test_max_sockets_uses_cfg_smoltcp_cap() {
    let src = read(NET_INIT);
    // 验: MAX_SOCKETS 必须从 cfg_smoltcp_cap 派生, 不再硬编码 256
    assert!(src.contains("const MAX_SOCKETS: usize = crate::kernel::services::proc::cfg_smoltcp_cap() as usize")
            || src.contains("const MAX_SOCKETS: usize = crate::kernel::framework::proc::fd_alloc::cfg_smoltcp_cap() as usize")
            || src.contains("const MAX_SOCKETS: usize = crate::kernel::framework::proc::cfg_smoltcp_cap() as usize"),
        "TD-06: MAX_SOCKETS 必须从 cfg_smoltcp_cap() 派生");
    // 验: 注释必须提示用户改本值后须同步 8 张大表
    assert!(src.contains("SOCKET_STORAGE / TCP_*_BUFS"),
        "TD-06: 注释必须列出所有需要同步尺寸的表 (SOCKET_STORAGE + TCP_*_BUFS + ...)");
}

#[test]
fn test_max_sm_fd_derives_from_fdplan() {
    let src = read(NET_INIT);
    // MAX_SM_FD 必须仍从 FdPlan::SMOLTCP.capacity 派生 (TD-02 V3)
    assert!(src.contains("const MAX_SM_FD: usize = crate::kernel::framework::proc::fd_alloc::FdPlan::SMOLTCP.capacity as usize")
            || src.contains("const MAX_SM_FD: usize = crate::kernel::framework::proc::FdPlan::SMOLTCP.capacity as usize")
            || src.contains("const MAX_SM_FD: usize = crate::kernel::services::proc::FdPlan::SMOLTCP.capacity as usize"),
        "TD-06: MAX_SM_FD 必须仍从 FdPlan::SMOLTCP.capacity 派生 (TD-02 V3 一致性)");
}

#[test]
fn test_smoltcp_capacity_helper_exists() {
    let src = read(FD_ALLOC);
    // 同时保留 `smoltcp_capacity()` 别名, 供 build.rs 钩子未来使用
    assert!(src.contains("pub const fn smoltcp_capacity() -> u16"),
        "TD-06: 必须保留 `pub const fn smoltcp_capacity() -> u16` 别名 (供 build.rs 钩子用)");
}
