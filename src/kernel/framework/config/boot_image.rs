//! 演进 10: ConfigSummary 启动期编码为紧凑字节序列
//!
//! ## 动机
//!
//! 内核崩溃后, klog 缓冲区可能被破坏, 但"boot_image 配置区"可以保留
//! 整个 ConfigSummary 的原始字节。开发者可以从物理 dump (QEMU -dump-vm
//! 等等) 中提取前几个字节, 立即知道当前内核的容量/能力配置, 而不必
//! 启动 dmesg 解析。
//!
//! ## 编码格式 (固定 64 字节, 大端 little-endian 双方案简化 → 全部 little-endian)
//!
//! ```text
//! offset  type   field
//! 0       u32    魔数 0xC0_FF_EE_01 ("config-encoding v1")
//! 4       u32    boot_epoch_lo (低 32 位 tick)
//! 8       u32    boot_epoch_hi
//! 12      u32    max_cpus
//! 16      u32    max_irqs
//! 20      u32    max_processes
//! 24      u32    max_threads
//! 28      u32    actual_cpus
//! 32      u32    page_size
//! 36      u32    kaslr_offset_lo
//! 40      u32    kaslr_offset_hi
//! 44      u8     smp  + kaslr + preempt + kpti + barrier (5 个 bool 拼成 1 字节)
//! 45      u8     reserved
//! 46      u16    feature_id  (高位: smp=1, preempt=2, kaslr=4, kpti=8, barrier=16)
//! 48      u64    checksum (简单 XOR) (48..56)
//! 56      u8[8]  magic tail 0xEE_FF_C0_01_00_00_00_00
//! 共 64 字节
//! ```
//!
//! ## 写入方式
//!
//! `BootImage` 是全局静态数组, 写入由 `init()` 调用一次, 之后多核只读。
//! 读端 (`read_boot_image()`) 供崩溃诊断 / 测试 / 调试器使用。

use super::caps::get_config_summary;

const ENCODED_LEN: usize = 64;
const HEADER_MAGIC: u32 = 0xC0FFEE01;
/// 8 字节尾部 magic: 前 4 字节为可识别签名, 后 4 字节保留 0 (用于 8 字节对齐)
/// 与文件头注释中的 `0xEE_FF_C0_01_00_00_00_00` 完全对应, 即便没有 `fill(0)` 也安全。
const TAIL_MAGIC: [u8; 8] = [0xEE, 0xFF, 0xC0, 0x01, 0x00, 0x00, 0x00, 0x00];

/// Global "boot image" — populated once by `init()`.
pub static BOOT_IMAGE: spin::Mutex<[u8; ENCODED_LEN]> =
    spin::Mutex::new([0u8; ENCODED_LEN]);

/// Pack a `u32` into the buffer at `offset` (little-endian).
#[inline]
fn pack_u32(buf: &mut [u8], offset: usize, v: u32) {
    if offset + 4 > buf.len() {
        return;
    }
    buf[offset] = (v & 0xFF) as u8;
    buf[offset + 1] = ((v >> 8) & 0xFF) as u8;
    buf[offset + 2] = ((v >> 16) & 0xFF) as u8;
    buf[offset + 3] = ((v >> 24) & 0xFF) as u8;
}

/// Pack a `u64` into the buffer at `offset` (little-endian).
#[inline]
fn pack_u64(buf: &mut [u8], offset: usize, v: u64) {
    if offset + 8 > buf.len() {
        return;
    }
    pack_u32(buf, offset, v as u32);
    pack_u32(buf, offset + 4, (v >> 32) as u32);
}

/// Pack a `u8` at `offset`.
#[inline]
fn pack_u8(buf: &mut [u8], offset: usize, v: u8) {
    if offset < buf.len() {
        buf[offset] = v;
    }
}

/// Pack a `u16` at `offset`.
#[inline]
fn pack_u16(buf: &mut [u8], offset: usize, v: u16) {
    if offset + 2 > buf.len() {
        return;
    }
    buf[offset] = (v & 0xFF) as u8;
    buf[offset + 1] = ((v >> 8) & 0xFF) as u8;
}

/// Encode the current `ConfigSummary` into the global boot_image buffer.
///
/// **调用语义**: 在 `config::init()` 末尾调用一次, 单线程上下文。
pub fn encode_boot_image() {
    let s = get_config_summary();
    let caps = s.capabilities;

    let mut guard = BOOT_IMAGE.lock();
    // 必须显式解引用: `&mut guard` 类型是 `&mut MutexGuard<...>`,
    // 而 `pack_u32` 需要 `&mut [u8; N]`。
    #[allow(clippy::explicit_auto_deref)]
    let buf: &mut [u8; ENCODED_LEN] = &mut *guard;
    buf.fill(0);
    pack_u32(buf, 0, HEADER_MAGIC);
    // boot_epoch: 在 32/64 位系统都安全 — 当前用 0 占位, 未来接 tick
    pack_u32(buf, 4, 0);
    pack_u32(buf, 8, 0);
    pack_u32(buf, 12, s.max_cpus as u32);
    pack_u32(buf, 16, s.max_irqs as u32);
    pack_u32(buf, 20, s.max_processes as u32);
    pack_u32(buf, 24, s.max_threads as u32);
    pack_u32(buf, 28, s.actual_cpus);
    pack_u32(buf, 32, s.page_size as u32);
    pack_u32(buf, 36, s.kaslr_offset as u32);
    pack_u32(buf, 40, (s.kaslr_offset >> 32) as u32);

    // 第 44 字节: 5 个 bool 紧凑编码
    let mut flags = 0u8;
    if caps.smp {
        flags |= 1 << 0;
    }
    if caps.kaslr {
        flags |= 1 << 1;
    }
    if caps.preempt {
        flags |= 1 << 2;
    }
    if caps.kpti {
        flags |= 1 << 3;
    }
    if caps.barrier {
        flags |= 1 << 4;
    }
    pack_u8(buf, 44, flags);
    pack_u8(buf, 45, 0);

    // 第 46 字节: feature_id (与 caps 相同含义, 但保留为 u16 便于未来扩展)
    let mut feature_id: u16 = 0;
    if caps.smp {
        feature_id |= 1 << 0;
    }
    if caps.preempt {
        feature_id |= 1 << 1;
    }
    if caps.kaslr {
        feature_id |= 1 << 2;
    }
    if caps.kpti {
        feature_id |= 1 << 3;
    }
    if caps.barrier {
        feature_id |= 1 << 4;
    }
    pack_u16(buf, 46, feature_id);

    // 第 48..56: 简单 XOR checksum
    let mut xor = 0u64;
    for i in 0..48 {
        xor ^= (buf[i] as u64) << ((i & 7) * 8);
    }
    pack_u64(buf, 48, xor);

    // 尾部 magic
    for (i, &b) in TAIL_MAGIC.iter().enumerate() {
        pack_u8(buf, 56 + i, b);
    }
}

/// Read a snapshot of the boot_image (供测试/调试).
pub fn read_boot_image() -> [u8; ENCODED_LEN] {
    *BOOT_IMAGE.lock()
}

/// Number of bytes occupied by the encoded image.
pub const fn encoded_len() -> usize {
    ENCODED_LEN
}
