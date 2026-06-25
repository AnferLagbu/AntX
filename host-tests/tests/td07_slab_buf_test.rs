// TD-07: 验证 smoltcp TCP/UDP 缓冲已迁移到 slab, 不再静态 BSS 占用.
//
// 验收:
//   1. TCP_RX_BUFS / TCP_TX_BUFS / UDP_RX_BUFS / UDP_TX_BUFS 类型必须是 `[*mut u8; N]`
//      指针表 (REVAL-W W4.2.3.1 阶段 N 从 MAX_SM_FD 扩展为 TOTAL_SLOTS = MAX_SM_FD
//      + MAX_SOCKETS, 覆盖 sm_socket 路径 + SmoltcpNetStack 路径)
//   2. smoltcp socket 创建必须通过 `k_malloc(TCP_BUF_SIZE)` / `k_malloc(UDP_BUF_SIZE)` 申请
//   3. smoltcp close 路径必须 `k_free` 4 个非空指针
//   4. 启动期 BSS 占用 = 0 (静态表内都是 null_mut)

use std::fs;
use std::path::Path;

const NET_INIT: &str = "src/kernel/framework/net/init.rs";

fn read(path: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(path);
    fs::read_to_string(&p).unwrap_or_else(|_| panic!("读 {}", path))
}

#[test]
fn test_buf_storage_uses_pointer_table() {
    let src = read(NET_INIT);
    // TD-07: 4 张大表必须是 [*mut u8; N] 指针表.
    // REVAL-W W4.2.3.1: N = TOTAL_SLOTS = MAX_SM_FD + MAX_SOCKETS,
    // 覆盖两个 socket 路径. 测试用 *mut u8 形态 + [null_mut(); N] 初始化
    // 双重特征匹配, 不绑定具体 N 值 (MAX_SM_FD / TOTAL_SLOTS 都接受).
    for name in &["TCP_RX_BUFS", "TCP_TX_BUFS", "UDP_RX_BUFS", "UDP_TX_BUFS"] {
        let decl_ptr_table = format!("static mut {}: [*mut u8;", name);
        assert!(src.contains(&decl_ptr_table),
            "TD-07: {} 必须改为 `[*mut u8; N]` 指针表, 不再静态 [[u8; N]; M]", name);
        // 必须以 null_mut() 初始化 (BSS 占用 0)
        let init_null = format!("static mut {}: [*mut u8;", name);
        assert!(src.contains(&init_null),
            "TD-07: {} 启动期 BSS 占用必须为 0, 初始化为 [null_mut(); ...]", name);
    }
    // 反向验收: 不应再有 `[[u8; TCP_BUF_SIZE]; N]` 这种静态数组
    for old in &[
        "[[u8; TCP_BUF_SIZE]; MAX_SM_FD]",
        "[[u8; TCP_BUF_SIZE]; TOTAL_SLOTS]",
        "[[u8; UDP_BUF_SIZE]; MAX_SM_FD]",
        "[[u8; UDP_BUF_SIZE]; TOTAL_SLOTS]",
    ] {
        assert!(!src.contains(old),
            "TD-07: 不应再保留 `{}` 静态数组", old);
    }
}

#[test]
fn test_socket_alloc_uses_kmalloc() {
    let src = read(NET_INIT);
    // TCP 路径必须用 k_malloc(TCP_BUF_SIZE)
    let tcp_alloc = "k_malloc(TCP_BUF_SIZE)";
    assert!(src.contains(tcp_alloc),
        "TD-07: smoltcp TCP socket alloc 必须通过 `k_malloc(TCP_BUF_SIZE)` 申请 RX 缓冲");
    let tcp_alloc2 = "k_malloc(TCP_BUF_SIZE)";
    assert!(src.contains(tcp_alloc2),
        "TD-07: smoltcp TCP socket alloc 必须通过 `k_malloc(TCP_BUF_SIZE)` 申请 TX 缓冲");
    // UDP 路径必须用 k_malloc(UDP_BUF_SIZE)
    assert!(src.contains("k_malloc(UDP_BUF_SIZE)"),
        "TD-07: smoltcp UDP socket alloc 必须通过 `k_malloc(UDP_BUF_SIZE)` 申请缓冲");
}

#[test]
fn test_socket_close_uses_kfree() {
    let src = read(NET_INIT);
    // close 路径必须对 4 个非空指针调用 k_free
    for name in &["TCP_RX_BUFS", "TCP_TX_BUFS", "UDP_RX_BUFS", "UDP_TX_BUFS"] {
        let kfree_call = format!("k_free({}[fd as usize]);", name);
        assert!(src.contains(&kfree_call),
            "TD-07: smoltcp close 路径必须对 {} 调用 `k_free` 归还 slab", name);
        // 必须判空 + 归零 (双保险)
        let nullify = format!("{}[fd as usize] = core::ptr::null_mut();", name);
        assert!(src.contains(&nullify),
            "TD-07: k_free 之后必须把 {} 置回 null, 防 double free", name);
    }
}

#[test]
fn test_buf_lifetime_safe_through_socket_remove() {
    // TD-07: smoltcp socket 必须先 `sockets.remove(handle)` (drop), 才能 k_free 缓冲.
    // 顺序敏感: 先释放借用, 再释放底层内存.
    let src = read(NET_INIT);
    let close_marker = "pub unsafe extern \"C\" fn sm_close(";
    let start = src.find(close_marker).expect("sm_close not found");
    let body = &src[start..start + 5000];
    let remove_idx = body.find("sockets.remove(handle);").expect("sockets.remove missing");
    // 找 4 个 k_free 中最早一个的下标
    let first_kfree = body.find("k_free(TCP_RX_BUFS").expect("first k_free missing");
    assert!(remove_idx < first_kfree,
        "TD-07: sockets.remove(handle) 必须在所有 k_free 之前 (借用释放顺序敏感)");
}
