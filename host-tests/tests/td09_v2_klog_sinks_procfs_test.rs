// SPDX-License-Identifier: Apache-2.0
// TD-09 V2: /proc/sys/klog/sinks procfs 运行时管理
//
// 验收:
//   - services::klog 模块存在, 100% safe Rust (无 unsafe)
//   - klog::count() / name_at() / list_names() / render_text() / render_json()
//     全部为安全 API
//   - procfs `/proc/sys/klog/sinks` 走 services::klog::render_text
//   - procfs `/proc/sys/klog/sinks.json` 走 services::klog::render_json
//   - framework::klog 新增 klog_sink_name_at 安全访问器, services 层零 unsafe
//
// 运行: cargo test -p host-tests --test td09_v2_klog_sinks_procfs_test

use std::fs;
use std::path::Path;

const KLOG_SERVICES_RS: &str = "../src/kernel/services/klog.rs";
const KLOG_FRAMEWORK_RS: &str = "../src/kernel/framework/klog/mod.rs";
const PROCFS_CORE_RS: &str = "../src/kernel/services/fs/procfs_core.rs";

fn read(p: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(p);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// 找出从 `marker` 所在行开始, 下一个顶层 `pub fn` / `fn` / `}` 之前的文本.
/// 顶层定义为: 缩进 0 的行. 这样可避免内层闭包的 `}\n` 误截.
fn body_after<'a>(src: &'a str, marker: &str) -> &'a str {
    let start = src.find(marker).unwrap_or_else(|| panic!("未找到 {marker}"));
    // 找到 marker 所在行的下一行起点
    let line_start = src[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let bytes = src.as_bytes();
    let mut i = line_start;
    let mut depth = 0i32;
    let mut seen_open = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '{' {
            depth += 1;
            seen_open = true;
        } else if c == '}' {
            depth -= 1;
            if seen_open && depth <= 0 {
                return &src[line_start..=i];
            }
        }
        i += 1;
    }
    &src[line_start..]
}

// ============================================================================
// services::klog 模块存在性 + 100% safe
// ============================================================================

#[test]
fn services_klog_module_exists() {
    let src = read(KLOG_SERVICES_RS);
    assert!(src.contains("#![deny(unsafe_code)]"),
        "services::klog 必须 #![deny(unsafe_code)]");
    assert!(src.contains("pub fn count"),
        "services::klog 必须有 pub fn count()");
    assert!(src.contains("pub fn name_at"),
        "services::klog 必须有 pub fn name_at()");
    assert!(src.contains("pub fn list_names"),
        "services::klog 必须有 pub fn list_names()");
    assert!(src.contains("pub fn render_text"),
        "services::klog 必须有 pub fn render_text()");
    assert!(src.contains("pub fn render_json"),
        "services::klog 必须有 pub fn render_json()");
    assert!(src.contains("pub fn register_defaults"),
        "services::klog 必须有 pub fn register_defaults()");
}

#[test]
fn services_klog_uses_safe_framework_api() {
    let src = read(KLOG_SERVICES_RS);
    // services 层不允许 unsafe 块
    let has_unsafe_block = src.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .any(|l| l.contains("unsafe {") || l.contains("unsafe  {") || l.contains("unsafe{"));
    assert!(!has_unsafe_block,
        "services::klog 含 unsafe 块, 违反 @SAFE 契约 (检查 lines 是否误判注释行)");
    // 必须使用 framework 的安全 name_at 入口
    assert!(src.contains("framework_klog::klog_sink_name_at"),
        "services::klog 必须走 framework::klog::klog_sink_name_at 安全入口");
    // 不应再直接调用 unsafe 入口
    assert!(!src.contains("klog_sink_at("),
        "services::klog 不应直接调用 unsafe klog_sink_at");
}

#[test]
fn services_klog_declared_in_mod_rs() {
    let src = read("../src/kernel/services/mod.rs");
    assert!(src.contains("pub mod klog"),
        "services::mod.rs 必须声明 pub mod klog (TD-09 V2)");
}

// ============================================================================
// framework::klog 安全入口 klog_sink_name_at
// ============================================================================

#[test]
fn framework_klog_has_safe_name_at() {
    let src = read(KLOG_FRAMEWORK_RS);
    assert!(src.contains("pub fn klog_sink_name_at"),
        "framework::klog 必须新增 pub fn klog_sink_name_at (safe) 安全入口");
    // 必须做越界检查
    let body = body_after(&src, "pub fn klog_sink_name_at");
    assert!(body.contains("idx >= n"),
        "klog_sink_name_at 必须做 idx >= n 越界检查");
    assert!(body.contains("return None"),
        "klog_sink_name_at 越界时必须 return None");
}

// ============================================================================
// render_text / render_json 内容契约
// ============================================================================

#[test]
fn render_text_contains_header_and_count() {
    let src = read(KLOG_SERVICES_RS);
    let body = body_after(&src, "pub fn render_text");
    assert!(body.contains("QueenX klog sinks"),
        "render_text 必须含 'QueenX klog sinks' 头 (内核项目标识)");
    assert!(body.contains("count: "),
        "render_text 必须含 'count: ' 字段");
    assert!(body.contains(": "),
        "render_text 行格式必须含 ': ' 分隔符");
}

#[test]
fn render_json_contains_format_version() {
    let src = read(KLOG_SERVICES_RS);
    let body = body_after(&src, "pub fn render_json");
    assert!(body.contains("format_version"),
        "render_json 必须含 format_version 字段 (与 config JSON 模式对齐)");
    // 源码中是转义字符串字面量 \\"sinks\\"
    assert!(body.contains("sinks") && body.contains("["),
        "render_json 必须含 sinks 数组");
    assert!(body.contains("count") && body.contains(":"),
        "render_json 必须含 count 字段");
}

#[test]
fn render_parse_format_routes_correctly() {
    let src = read(KLOG_SERVICES_RS);
    assert!(src.contains("pub fn parse_format"),
        "services::klog 必须暴露 pub fn parse_format");
    let body = body_after(&src, "pub fn parse_format");
    assert!(body.contains(".ends_with(\".json\")"),
        "parse_format 必须按 .json 后缀路由");
    assert!(body.contains("SinkListFormat::Json"),
        "parse_format .json → Json");
    assert!(body.contains("SinkListFormat::Text"),
        "parse_format 非 .json → Text");
}

// ============================================================================
// procfs 入口挂载
// ============================================================================

#[test]
fn procfs_routes_klog_sinks() {
    let src = read(PROCFS_CORE_RS);
    assert!(src.contains("sys/klog/sinks\""),
        "procfs 必须挂载 sys/klog/sinks 入口");
    assert!(src.contains("sys/klog/sinks.json\""),
        "procfs 必须挂载 sys/klog/sinks.json 入口");
    assert!(src.contains("services::klog::render_text"),
        "procfs sys/klog/sinks 必须走 services::klog::render_text");
    assert!(src.contains("services::klog::render_json"),
        "procfs sys/klog/sinks.json 必须走 services::klog::render_json");
}

// ============================================================================
// MAX_LOG_SINKS 在 services 层可用
// ============================================================================

#[test]
fn services_klog_uses_framework_max_log_sinks() {
    let src = read(KLOG_SERVICES_RS);
    assert!(src.contains("framework_klog::MAX_LOG_SINKS"),
        "services::klog 必须复用 framework::klog::MAX_LOG_SINKS 常量");
}
