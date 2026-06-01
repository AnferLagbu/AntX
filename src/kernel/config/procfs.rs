//! /proc/sys/config 接口: 用户态可读取内核编译期/启动期配置
//!
//! ## 接口契约
//!
//! - 入口名称: `sys/config` (text) 与 `sys/config.json` (JSON)
//! - 输出格式: text 模式 = 与 klog 一致的纯文本, 行分隔 `\n`;
//!            json 模式 = 严格 RFC 8259 风格, 无外部依赖
//! - 调用: `PROCFS_DATA.read("sys/config", buf)` 或 `sys/config.json`
//! - 每次调用**动态生成**内容 (非缓存), 以反映运行时状态
//!
//! ## 用途
//!
//! - 调试: 用户态 `cat /proc/sys/config` 查看当前内核配额
//! - 监控: 容量使用统计 (未来扩展)
//! - 跨架构诊断: 不依赖 dmesg, 仅依赖 procfs
//! - 机器可读: 监控系统可消费 `sys/config.json`

use super::caps::get_config_summary;

/// Text format selector for `/proc/sys/config` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// 人类可读纯文本 (klog 风格)。
    Text,
    /// 严格 RFC 8259 JSON, 字段顺序固定以保证可读性。
    Json,
}

/// Parse a procfs entry name to a `ConfigFormat`.
///
/// 规则: 文件名以 `.json` 结尾 → JSON; 否则 → Text。**调用方在 `mount`
/// 时按需注册两个条目**。
pub fn parse_format(name: &str) -> ConfigFormat {
    if name.ends_with(".json") {
        ConfigFormat::Json
    } else {
        ConfigFormat::Text
    }
}

/// Generate text content for `/proc/sys/config`.
///
/// Returns the number of bytes written into `buf`.
///
/// **安全要点**: 写指针不超过 `buf.len()`, 不依赖全局 alloc
/// (在无 alloc 上下文中也可安全调用)。
pub fn read_sys_config(buf: &mut [u8]) -> usize {
    write_text(buf)
}

/// Generate JSON content for `/proc/sys/config.json`.
///
/// 输出与 text 模式字段**一一对应** (除 `format_version` 标识), 便于监控
/// 系统 `jq` 抽取。
pub fn read_sys_config_json(buf: &mut [u8]) -> usize {
    write_json(buf)
}

// ============================================================================
// Text 输出 (演进 4 原始实现)
// ============================================================================

fn write_text(buf: &mut [u8]) -> usize {
    let s = get_config_summary();
    let caps = s.capabilities;

    let mut pos = 0usize;
    let push_str = |dst: &mut [u8], p: &mut usize, src: &str| {
        let b = src.as_bytes();
        let end = (*p + b.len()).min(dst.len());
        let len = end - *p;
        dst[*p..end].copy_from_slice(&b[..len]);
        *p += len;
    };
    let push_u64 = |dst: &mut [u8], p: &mut usize, val: u64| {
        if val == 0 && *p < dst.len() {
            dst[*p] = b'0';
            *p += 1;
            return;
        }
        let mut tmp = [0u8; 20];
        let mut i = 20;
        let mut v = val;
        while v > 0 && i > 0 {
            i -= 1;
            tmp[i] = (v % 10) as u8 + b'0';
            v /= 10;
        }
        let end = (*p + (20 - i)).min(dst.len());
        let len = end - *p;
        dst[*p..end].copy_from_slice(&tmp[i..i + len]);
        *p += len;
    };
    let push_usize = |dst: &mut [u8], p: &mut usize, val: usize| {
        push_u64(dst, p, val as u64);
    };
    let push_bool = |dst: &mut [u8], p: &mut usize, val: bool| {
        push_str(dst, p, if val { "yes" } else { "no" });
    };

    // Header
    push_str(buf, &mut pos, "AntX Kernel Configuration\n");
    push_str(buf, &mut pos, "==========================\n");
    push_str(buf, &mut pos, "max_cpus:        ");
    push_usize(buf, &mut pos, s.max_cpus);
    push_str(buf, &mut pos, "\nactual_cpus:     ");
    push_u64(buf, &mut pos, s.actual_cpus as u64);
    push_str(buf, &mut pos, "\nmax_irqs:        ");
    push_usize(buf, &mut pos, s.max_irqs);
    push_str(buf, &mut pos, "\nmax_processes:   ");
    push_usize(buf, &mut pos, s.max_processes);
    push_str(buf, &mut pos, "\nmax_threads:     ");
    push_usize(buf, &mut pos, s.max_threads);
    push_str(buf, &mut pos, "\npage_size:       ");
    push_u64(buf, &mut pos, s.page_size);
    push_str(buf, &mut pos, "\nkaslr_offset:    0x");
    push_u64(buf, &mut pos, s.kaslr_offset);
    push_str(buf, &mut pos, "\napic_enabled:    ");
    push_bool(buf, &mut pos, s.apic_enabled);
    push_str(buf, &mut pos, "\nioapic_enabled:  ");
    push_bool(buf, &mut pos, s.ioapic_enabled);
    push_str(buf, &mut pos, "\n-- capabilities --\n");
    push_str(buf, &mut pos, "smp:             ");
    push_bool(buf, &mut pos, caps.smp);
    push_str(buf, &mut pos, "\npreempt:         ");
    push_bool(buf, &mut pos, caps.preempt);
    push_str(buf, &mut pos, "\nkaslr:           ");
    push_bool(buf, &mut pos, caps.kaslr);
    push_str(buf, &mut pos, "\nkpti:            ");
    push_bool(buf, &mut pos, caps.kpti);
    push_str(buf, &mut pos, "\nbarrier:         ");
    push_bool(buf, &mut pos, caps.barrier);
    push_str(buf, &mut pos, "\n");

    pos
}

// ============================================================================
// JSON 输出 (演进 8)
// ============================================================================
//
// **设计要点**:
// 1. 不依赖 serde / alloc / heap — 全部内联, 写指针不越界
// 2. 严格 RFC 8259: 字段名加双引号, 字符串值转义, bool 值为小写
// 3. 字段顺序固定 — 便于 diff/审计, 监控可直接用 `jq` 解析
// 4. 包含 `format_version` 字段 — 未来 schema 演化时给消费者版本线索

fn write_json(buf: &mut [u8]) -> usize {
    let s = get_config_summary();
    let caps = s.capabilities;

    let mut pos = 0usize;

    // 这些闭包与 write_text 内部一致; 提取到模块顶层更整洁。
    let push_str = |dst: &mut [u8], p: &mut usize, src: &str| {
        let b = src.as_bytes();
        let end = (*p + b.len()).min(dst.len());
        let len = end - *p;
        dst[*p..end].copy_from_slice(&b[..len]);
        *p += len;
    };
    let push_u64 = |dst: &mut [u8], p: &mut usize, val: u64| {
        if val == 0 && *p < dst.len() {
            dst[*p] = b'0';
            *p += 1;
            return;
        }
        let mut tmp = [0u8; 20];
        let mut i = 20;
        let mut v = val;
        while v > 0 && i > 0 {
            i -= 1;
            tmp[i] = (v % 10) as u8 + b'0';
            v /= 10;
        }
        let end = (*p + (20 - i)).min(dst.len());
        let len = end - *p;
        dst[*p..end].copy_from_slice(&tmp[i..i + len]);
        *p += len;
    };

    // 字段定义宏: 把 "field_name: <value>" 写到 buf, value 是数字 / 字符串 / bool。
    // 这样 JSON 字段顺序一目了然, 避免手写 30 行重复样板。
    let push_field_num = |dst: &mut [u8], p: &mut usize, k: &str, v: u64| {
        push_str(dst, p, "\"");
        push_str(dst, p, k);
        push_str(dst, p, "\":");
        push_u64(dst, p, v);
    };
    let push_field_bool = |dst: &mut [u8], p: &mut usize, k: &str, v: bool| {
        push_str(dst, p, "\"");
        push_str(dst, p, k);
        push_str(dst, p, "\":");
        push_str(dst, p, if v { "true" } else { "false" });
    };
    let push_field_str = |dst: &mut [u8], p: &mut usize, k: &str, v: &str| {
        push_str(dst, p, "\"");
        push_str(dst, p, k);
        push_str(dst, p, "\":\"");
        push_str(dst, p, v);
        push_str(dst, p, "\"");
    };

    // {
    push_str(buf, &mut pos, "{");
    push_field_str(buf, &mut pos, "format_version", "1");
    push_str(buf, &mut pos, ",");
    push_field_str(buf, &mut pos, "kernel", "AntX");
    push_str(buf, &mut pos, ",");
    push_field_num(buf, &mut pos, "max_cpus", s.max_cpus as u64);
    push_str(buf, &mut pos, ",");
    push_field_num(buf, &mut pos, "actual_cpus", s.actual_cpus as u64);
    push_str(buf, &mut pos, ",");
    push_field_num(buf, &mut pos, "max_irqs", s.max_irqs as u64);
    push_str(buf, &mut pos, ",");
    push_field_num(buf, &mut pos, "max_processes", s.max_processes as u64);
    push_str(buf, &mut pos, ",");
    push_field_num(buf, &mut pos, "max_threads", s.max_threads as u64);
    push_str(buf, &mut pos, ",");
    push_field_num(buf, &mut pos, "page_size", s.page_size);
    push_str(buf, &mut pos, ",");
    push_field_num(buf, &mut pos, "kaslr_offset", s.kaslr_offset);
    push_str(buf, &mut pos, ",");
    push_field_bool(buf, &mut pos, "apic_enabled", s.apic_enabled);
    push_str(buf, &mut pos, ",");
    push_field_bool(buf, &mut pos, "ioapic_enabled", s.ioapic_enabled);
    push_str(buf, &mut pos, ",");
    // 嵌套 capabilities
    push_str(buf, &mut pos, "\"capabilities\":{");
    push_field_bool(buf, &mut pos, "smp", caps.smp);
    push_str(buf, &mut pos, ",");
    push_field_bool(buf, &mut pos, "preempt", caps.preempt);
    push_str(buf, &mut pos, ",");
    push_field_bool(buf, &mut pos, "kaslr", caps.kaslr);
    push_str(buf, &mut pos, ",");
    push_field_bool(buf, &mut pos, "kpti", caps.kpti);
    push_str(buf, &mut pos, ",");
    push_field_bool(buf, &mut pos, "barrier", caps.barrier);
    push_str(buf, &mut pos, "}");
    // }
    push_str(buf, &mut pos, "}");

    pos
}
