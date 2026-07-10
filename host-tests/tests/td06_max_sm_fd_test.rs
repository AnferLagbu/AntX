// TD-06: 验证 smoltcp FD 容量配置.
//
// 验收:
//   1. `framework/net/init.rs` 的 `MAX_SOCKETS` 必须为 256
//   2. `MAX_SM_FD` 与 `FdPlan::SMOLTCP.capacity` 仍为单一来源 (MAX_SOCKETS == MAX_SM_FD 隐含)
//   3. 默认容量 = 256 (与 I-47 G_MAX_SOCKETS 初始 DEFAULT_MAX_SOCKETS 一致)

use std::fs;
use std::path::Path;

const NET_INIT: &str = "src/kernel/framework/net/init.rs";

fn read(path: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(path);
    fs::read_to_string(&p).unwrap_or_else(|_| panic!("读 {}", path))
}

#[test]
fn test_max_sockets_is_256() {
    let src = read(NET_INIT);
    // 验: MAX_SOCKETS 必须为 256
    assert!(src.contains("const MAX_SOCKETS: usize = 256;"),
        "TD-06: MAX_SOCKETS 必须为 256");
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
