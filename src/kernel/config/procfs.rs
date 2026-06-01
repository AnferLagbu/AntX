//! /proc/sys/config 接口: 用户态可读取内核编译期/启动期配置
//!
//! ## 接口契约
//!
//! - 入口名称: `sys/config`
//! - 输出格式: 与 klog 一致的纯文本, 行分隔 `\n`
//! - 调用: `PROCFS_DATA.read("sys/config", buf)`
//! - 每次调用**动态生成**内容 (非缓存), 以反映运行时状态
//!
//! ## 用途
//!
//! - 调试: 用户态 `cat /proc/sys/config` 查看当前内核配额
//! - 监控: 容量使用统计 (未来扩展)
//! - 跨架构诊断: 不依赖 dmesg, 仅依赖 procfs

use super::caps::get_config_summary;

/// Generate text content for `/proc/sys/config`.
///
/// Returns the number of bytes written into `buf`.
///
/// **安全要点**: 写指针不超过 `buf.len()`, 不依赖全局 alloc
/// (在无 alloc 上下文中也可安全调用)。
pub fn read_sys_config(buf: &mut [u8]) -> usize {
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
