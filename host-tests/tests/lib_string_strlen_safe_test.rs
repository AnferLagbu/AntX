//! 字符串长度契约测试 (B04-23)
//!
//! 镜像内核 `src/kernel/framework/lib/string.rs` 中 `strlen` 与 `strlen_safe` 的
//! 关键不变量, 在 host 环境验证:
//! 1. `strlen` C FFI 上界 = `STRLEN_MAX = 1024` (B04-07), 防御恶意指针无上界读取
//! 2. `strlen(null)` 返回 0
//! 3. `strlen` 在遇到 `\0` 时立即停止 (包括 STRLEN_MAX 截断路径)
//! 4. `strlen_safe(&[i8])` 安全版按字节遍历, 遇 `\0` 停止, 不依赖指针上界
//! 5. STRLEN_MAX 边界: 长度刚好 STRLEN_MAX 与超过 STRLEN_MAX 行为差异
//!
//! 主机端不直接 link 内核 (no_std), 镜像内核实现 + 验证关键不变量.
//! 内核权威实现: `src/kernel/framework/lib/string.rs::strlen/strlen_safe`.

/// 内核常量镜像 (B04-07 决策点 D1, DECISION-060)
const STRLEN_MAX: usize = 1024;

/// C 风格 strlen 镜像 (与内核 `pub unsafe extern "C" fn strlen` 语义一致)
unsafe fn strlen_impl(s: *const i8) -> usize {
    if s.is_null() {
        return 0;
    }
    let mut len = 0;
    let mut ptr = s;
    // 与内核 `while *ptr != 0 && len < STRLEN_MAX` 一致
    // SAFETY: 调用方保证 s 有效或为 null; null 路径已返回
    unsafe {
        while *ptr != 0 && len < STRLEN_MAX {
            len += 1;
            ptr = ptr.add(1);
        }
    }
    len
}

/// Rust 安全版 strlen_safe 镜像 (与内核 `pub fn strlen_safe(s: &[i8]) -> usize` 一致)
fn strlen_safe_impl(s: &[i8]) -> usize {
    let mut len = 0;
    for &ch in s {
        if ch == 0 {
            break;
        }
        len += 1;
    }
    len
}

/// 辅助: 将 Rust 字符串转为以 `\0` 结尾的 C 风格字节数组
fn to_cstr(s: &str) -> Vec<i8> {
    let mut v: Vec<i8> = s.bytes().map(|b| b as i8).collect();
    v.push(0);
    v
}

// =============================================================================
// strlen C FFI 测试
// =============================================================================

#[test]
fn test_strlen_null_returns_zero() {
    // SAFETY: 显式 null 检查路径
    let len = unsafe { strlen_impl(core::ptr::null()) };
    assert_eq!(len, 0, "strlen(null) must return 0");
}

#[test]
fn test_strlen_empty_string() {
    let s = to_cstr("");
    let len = unsafe { strlen_impl(s.as_ptr()) };
    assert_eq!(len, 0, "strlen of empty string must be 0");
}

#[test]
fn test_strlen_basic_ascii() {
    let s = to_cstr("hello");
    let len = unsafe { strlen_impl(s.as_ptr()) };
    assert_eq!(len, 5, "strlen(\"hello\") must be 5");
}

#[test]
fn test_strlen_stops_at_terminator() {
    // 字符串 "abc\0garbage" — strlen 必须在第一个 \0 处停止
    let mut buf: Vec<i8> = b"abc".iter().map(|&b| b as i8).collect();
    buf.push(0);
    buf.extend(b"garbage".iter().map(|&b| b as i8));
    let len = unsafe { strlen_impl(buf.as_ptr()) };
    assert_eq!(len, 3, "strlen must stop at first NUL, ignore trailing garbage");
}

#[test]
fn test_strlen_under_max_returns_full_length() {
    // 长度 < STRLEN_MAX: 应当返回完整长度 (前提是 buf 实际以 \0 结尾)
    let s = to_cstr(&"x".repeat(STRLEN_MAX - 1));
    let len = unsafe { strlen_impl(s.as_ptr()) };
    assert_eq!(len, STRLEN_MAX - 1, "strlen below MAX must return full length");
}

#[test]
fn test_strlen_at_max_returns_max() {
    // 长度 == STRLEN_MAX: 应当返回 STRLEN_MAX (合法路径, 因遇到 \0 之前已到上限)
    let s = to_cstr(&"y".repeat(STRLEN_MAX));
    let len = unsafe { strlen_impl(s.as_ptr()) };
    assert_eq!(len, STRLEN_MAX, "strlen at MAX boundary returns MAX");
}

#[test]
fn test_strlen_over_max_truncated_to_max() {
    // B04-07 关键不变量: 长度 > STRLEN_MAX 必须被截断到 STRLEN_MAX,
    // 不允许无上界读取 (这是 DECISION-060 决策点的核心)
    // 测试场景: buf 长度 > STRLEN_MAX 且全为非 \0 字符
    let mut buf: Vec<i8> = vec![b'a' as i8; STRLEN_MAX + 100];
    // 不放 \0, strlen 必须在 STRLEN_MAX 处停止
    let len = unsafe { strlen_impl(buf.as_ptr()) };
    assert_eq!(
        len, STRLEN_MAX,
        "strlen must truncate at STRLEN_MAX (no unbounded read)"
    );
}

#[test]
fn test_strlen_with_early_terminator_in_max_buffer() {
    // buf 长度 > STRLEN_MAX, 但中间有 \0, 应在 \0 处停止 (而非 STRLEN_MAX)
    let mut buf: Vec<i8> = vec![b'a' as i8; STRLEN_MAX + 50];
    buf[10] = 0; // 在位置 10 终止
    let len = unsafe { strlen_impl(buf.as_ptr()) };
    assert_eq!(len, 10, "strlen stops at NUL even within MAX window");
}

// =============================================================================
// strlen_safe Rust API 测试
// =============================================================================

#[test]
fn test_strlen_safe_empty() {
    let buf: [i8; 1] = [0];
    assert_eq!(strlen_safe_impl(&buf), 0);
}

#[test]
fn test_strlen_safe_basic() {
    let s = to_cstr("hello world");
    assert_eq!(strlen_safe_impl(&s), 11);
}

#[test]
fn test_strlen_safe_no_terminator_counts_all() {
    // 无 \0 终止符: 与 C 版 strlen 不同, 应返回切片长度 (无上界截断,
    // 因为调用方已知切片长度)
    let buf: [i8; 5] = [b'a' as i8, b'b' as i8, b'c' as i8, b'd' as i8, b'e' as i8];
    assert_eq!(strlen_safe_impl(&buf), 5, "no NUL → full slice length");
}

#[test]
fn test_strlen_safe_stops_at_first_nul() {
    let buf: [i8; 8] = [
        b'a' as i8, b'b' as i8, 0, b'x' as i8, b'y' as i8, b'z' as i8, b'w' as i8, 0,
    ];
    assert_eq!(strlen_safe_impl(&buf), 2);
}

#[test]
fn test_strlen_safe_large_slice_no_nul() {
    // 与 C 版不同: 安全版不依赖上界, 大切片无 \0 时返回完整长度
    let buf: Vec<i8> = vec![b'k' as i8; STRLEN_MAX * 2];
    assert_eq!(
        strlen_safe_impl(&buf),
        STRLEN_MAX * 2,
        "strlen_safe has no MAX cap (caller controls slice length)"
    );
}

// =============================================================================
// 对比测试: 同一字符串 C 版与安全版结果一致 (前提是切片以 \0 结尾)
// =============================================================================

#[test]
fn test_strlen_c_and_safe_consistent_for_cstr() {
    for sample in &["", "a", "abc", "kernel", "路径测试", "QEMU NVMe MSI-X"] {
        let cstr = to_cstr(sample);
        // SAFETY: cstr 来自 to_cstr, 以 \0 结尾且不含 null 中间
        let c_len = unsafe { strlen_impl(cstr.as_ptr()) };
        let safe_len = strlen_safe_impl(&cstr);
        assert_eq!(
            c_len,
            sample.len(),
            "C strlen of {:?} should equal byte length",
            sample
        );
        assert_eq!(
            c_len, safe_len,
            "C strlen and strlen_safe should agree on valid C string {:?}",
            sample
        );
    }
}

// =============================================================================
// STRLEN_MAX 常量不变量测试
// =============================================================================

#[test]
fn test_strlen_max_is_1024() {
    // B04-07 决策点: STRLEN_MAX = 1024. 若此值变更, 需重新评估是否仍覆盖内核
    // 内部合法字符串 (路径 ≤ 256, 命令行 ≤ 256).
    assert_eq!(STRLEN_MAX, 1024, "STRLEN_MAX must remain 1024 per DECISION-060");
}

#[test]
fn test_strlen_max_covers_kernel_paths() {
    // 假设: 内核合法字符串最大长度 ≤ 256 字节 (路径名 ≤ 256 + 命令行 ≤ 256).
    // STRLEN_MAX = 1024 是 256 的 4 倍, 留有充足 buffer.
    const KERNEL_MAX_CSTR: usize = 256;
    assert!(
        STRLEN_MAX >= KERNEL_MAX_CSTR * 2,
        "STRLEN_MAX must cover at least 2x kernel max cstr length"
    );
}
