// TD-05: 验证 smoltcp 小型热表 (SOCKET_TABLE / FD_TYPES) 已升级为 64 字节 cache line 对齐
//
// 验收:
//   1. SOCKET_TABLE 必须包装在 `#[repr(align(64))]` 结构内 (不能直接对 `static mut [T; N]`)
//   2. FD_TYPES 同上
//   3. 所有 53+ 处访问点必须用 .0 字段索引 (防后续误改回未对齐实现)
//
// 注: 静态契约扫描, 不进内核态.

use std::fs;
use std::path::Path;

const INIT: &str = "src/kernel/framework/net/init.rs";

fn read(path: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(path);
    fs::read_to_string(&p).unwrap_or_else(|_| panic!("读 {}", path))
}

#[test]
fn test_socket_table_is_cache_aligned() {
    // TD-05: SOCKET_TABLE 必须按 64 字节 cache line 对齐
    let src = read(INIT);
    // 验: 必须有 Align64<T> 包装结构定义
    assert!(src.contains("#[repr(align(64))]\nstruct Align64<T>(T)"),
        "TD-05: 必须定义 `#[repr(align(64))] struct Align64<T>(T)` 包装");
    // 验: SOCKET_TABLE 类型必须是 Align64<...> 而非直接 [T; N]
    assert!(src.contains("static mut SOCKET_TABLE: SOCKET_TABLE_T = Align64("),
        "TD-05: SOCKET_TABLE 必须用 Align64 包装 (cache line 对齐)");
    assert!(src.contains("type SOCKET_TABLE_T = Align64<"),
        "TD-05: SOCKET_TABLE 必须有 type alias 指向 Align64<...>");
}

#[test]
fn test_fd_types_is_cache_aligned() {
    // TD-05: FD_TYPES 必须按 64 字节 cache line 对齐
    let src = read(INIT);
    assert!(src.contains("static mut FD_TYPES: FD_TYPES_T = Align64("),
        "TD-05: FD_TYPES 必须用 Align64 包装 (cache line 对齐)");
    assert!(src.contains("type FD_TYPES_T = Align64<"),
        "TD-05: FD_TYPES 必须有 type alias 指向 Align64<...>");
}

#[test]
fn test_no_unwrapped_socket_table_index() {
    // TD-05: 所有 SOCKET_TABLE[i] 访问必须走 .0 字段, 防后续误改回未对齐实现
    let src = read(INIT);
    // 排除掉含 .0 的情况, 找直接 SOCKET_TABLE[ 的残留
    // 用简单行扫描
    for (idx, line) in src.lines().enumerate() {
        let trimmed = line.trim();
        // 跳过注释
        if trimmed.starts_with("//") {
            continue;
        }
        if trimmed.contains("SOCKET_TABLE[") && !trimmed.contains("SOCKET_TABLE.0[") {
            panic!("TD-05: L{} 仍用未对齐的 SOCKET_TABLE[..], 必须改为 SOCKET_TABLE.0[..]:\n{}",
                idx + 1, line);
        }
        if trimmed.contains("FD_TYPES[") && !trimmed.contains("FD_TYPES.0[") {
            panic!("TD-05: L{} 仍用未对齐的 FD_TYPES[..], 必须改为 FD_TYPES.0[..]:\n{}",
                idx + 1, line);
        }
    }
}

#[test]
fn test_cache_line_size_documented() {
    // TD-05: cache line 大小必须有文档说明
    let src = read(INIT);
    assert!(src.contains("cache line"),
        "TD-05: 必须有 cache line 对齐的文档说明 (供维护者理解 64 字节来源)");
}
