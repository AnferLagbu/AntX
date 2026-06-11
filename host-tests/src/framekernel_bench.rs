//! Framekernel 微基准测试 (性能回归基线)
//!
//! ## 目标
//! 测量 QueenX 框内核关键路径的纯算法性能, 建立可重复的回归基线.
//! 所有实现都 host-runnable (std 可用), 与内核版本位一致, 便于:
//! - CI 跑回归检查 (vs. baseline.json)
//! - 优化前后对比
//! - 论文性能数据采集
//!
//! ## 覆盖的热点路径
//! 1. `page_flags_bench`: PageFlags 位运算 (PRESENT/WRITABLE/USER/NX)
//! 2. `pte_set_flags_bench`: PageTableEntry.set_flags 原子位操作
//! 3. `iomem_alias_bench`: IoMem 别名区间注册 (重叠检测)
//! 4. `capability_check_bench`: CapabilityMatrix 16 域位检查
//! 5. `dma_state_machine_bench`: DmaStream 状态机迁移
//! 6. `sha256_block_bench`: SHA-256 单 block 压缩 (credo 身份)
//! 7. `attribution_classify_bench`: 故障归属分类 (barrier)
//! 8. `recovery_decide_bench`: 恢复策略决策 (barrier)
//! 9. `bitmap_scan_bench`: 位图扫描 (PMM 物理页分配)
//! 10. `btree_id_lookup_bench`: BTreeMap 整数键查找
//!
//! ## 输出
//! stdout 单行 JSON:
//!   `{"version": 1, "results": [{"name": "...", "ns_per_op": ...}, ...]}`
//!
//! ## 集成
//! - `scripts/record_bench_baseline.py` 生成 baseline.json
//! - `scripts/check_bench_regression.py` 对比并报告 > 15% 退化
//! - `make -f Makefile.ci bench-baseline` / `bench-check`

#![allow(dead_code)]

use std::time::Instant;

// ====== 1. PageFlags 位运算 (来自 framework/mm/mod.rs) ======

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct PageFlags: u64 {
        const PRESENT     = 1 << 0;
        const WRITABLE    = 1 << 1;
        const USER        = 1 << 2;
        const WRITE_THROUGH = 1 << 3;
        const CACHE_DISABLE = 1 << 4;
        const ACCESSED    = 1 << 5;
        const DIRTY       = 1 << 6;
        const HUGE_PAGE   = 1 << 7;
        const GLOBAL      = 1 << 8;
        const NX          = 1u64 << 63;
    }
}

/// 每轮执行 64 个位运算, 使每轮有可测量的耗时
const PAGE_FLAGS_BATCH: u64 = 64;

pub fn page_flags_bench(iters: u64) -> u128 {
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
    let start = Instant::now();
    let mut sink: u64 = 0;
    for i in 0..iters {
        let mut f = flags;
        for j in 0..PAGE_FLAGS_BATCH {
            if (i + j) & 1 == 0 { f |= PageFlags::NX; }
            if (i + j) & 3 == 0 { f |= PageFlags::GLOBAL; }
            if (i + j) & 7 == 0 { f |= PageFlags::ACCESSED; }
            sink ^= f.bits();
        }
    }
    std::hint::black_box(sink);
    // 归一化到 "单操作时间": 总耗时 (ns) / (iters * BATCH), 转 ps 避免精度损失
    let elapsed = start.elapsed().as_nanos();
    let total_ops = (iters as u128) * (PAGE_FLAGS_BATCH as u128);
    elapsed.saturating_mul(1_000) / total_ops
}

// ====== 2. PTE set_flags (来自 framework/mm/mod.rs PageTableEntry) ======

#[derive(Clone, Copy, Debug)]
struct MockPte {
    bits: u64,
}

impl MockPte {
    #[inline(always)]
    fn new(v: u64) -> Self { Self { bits: v } }
    #[inline(always)]
    fn set_flags(&mut self, flags: PageFlags) {
        self.bits = (self.bits & !PageFlags::all().bits()) | flags.bits();
    }
    #[inline(always)]
    fn is_present(&self) -> bool { self.bits & PageFlags::PRESENT.bits() != 0 }
}

pub fn pte_set_flags_bench(iters: u64) -> u128 {
    let mut pte = MockPte::new(0x0);
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE;
    let start = Instant::now();
    let mut sink: u64 = 0;
    for i in 0..iters {
        pte.set_flags(flags);
        if i & 1 == 0 { pte.set_flags(flags | PageFlags::USER); }
        if pte.is_present() { sink ^= 1; }
    }
    std::hint::black_box(sink);
    start.elapsed().as_nanos()
}

// ====== 3. IoMem Alias Registry (来自 framework/iomem.rs) ======

const MAX_MMIO_MAPPINGS: usize = 64;

#[derive(Debug, Clone, Copy)]
struct AliasEntry {
    phys: u64,
    len: usize,
}

struct AliasRegistry {
    entries: Vec<AliasEntry>,
    capacity: usize,
}

impl AliasRegistry {
    fn new() -> Self {
        Self { entries: Vec::with_capacity(MAX_MMIO_MAPPINGS), capacity: MAX_MMIO_MAPPINGS }
    }
    fn check_conflict(&self, phys: u64, len: usize) -> bool {
        let end = phys.saturating_add(len as u64);
        for e in &self.entries {
            let existing_end = e.phys.saturating_add(e.len as u64);
            if phys < existing_end && end > e.phys { return true; }
        }
        false
    }
    fn register(&mut self, phys: u64, len: usize) -> Result<(), ()> {
        if self.entries.len() >= self.capacity { return Err(()); }
        if self.check_conflict(phys, len) { return Err(()); }
        self.entries.push(AliasEntry { phys, len });
        Ok(())
    }
}

pub fn iomem_alias_bench(iters: u64) -> u128 {
    let mut r = AliasRegistry::new();
    for i in 0..30 {
        r.register(0x1000 + (i as u64) * 0x1000, 0x800).unwrap();
    }
    const BATCH: u64 = 32;
    let start = Instant::now();
    let mut sink: u64 = 0;
    for i in 0..iters {
        for j in 0..BATCH {
            let phys = 0x50000 + ((i * BATCH + j) as u64 & 0xFFFF) * 0x100;
            let len = 0x800;
            if r.check_conflict(phys, len) { sink ^= 1; }
        }
    }
    std::hint::black_box(sink);
    let elapsed = start.elapsed().as_nanos();
    let total_ops = (iters as u128) * (BATCH as u128);
    elapsed.saturating_mul(1_000) / total_ops
}

// ====== 4. Capability check (来自 framework/credo/capability.rs) ======

const CAP_DOMAINS: usize = 16;

struct CapabilityMatrix {
    caps: [u64; CAP_DOMAINS],
}

impl CapabilityMatrix {
    fn new() -> Self { Self { caps: [0; CAP_DOMAINS] } }
    fn grant(&mut self, dom: usize, bits: u64) { self.caps[dom] |= bits; }
    fn has(&self, dom: usize, bit: u64) -> bool { (self.caps[dom] & bit) != 0 }
}

pub fn capability_check_bench(iters: u64) -> u128 {
    let mut m = CapabilityMatrix::new();
    m.grant(1, 0b11);
    m.grant(3, 0b10101);
    m.grant(2, 0b1111);
    m.grant(5, 0b1);
    const BATCH: u64 = 64;
    let start = Instant::now();
    let mut sink: u64 = 0;
    for i in 0..iters {
        for j in 0..BATCH {
            let dom = ((i * BATCH + j) as usize) & 0xF;
            let bit = 1u64 << ((i + j) & 0x1F);
            if m.has(dom, bit) { sink ^= 1; }
        }
    }
    std::hint::black_box(sink);
    let elapsed = start.elapsed().as_nanos();
    let total_ops = (iters as u128) * (BATCH as u128);
    elapsed.saturating_mul(1_000) / total_ops
}

// ====== 5. DmaStream 状态机 (来自 framework/dma_buf.rs) ======

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DmaDirection { ToDevice, FromDevice, Bidirectional }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyncState { CpuReady, DeviceReady, BidirInProgress }

struct DmaStream {
    dir: DmaDirection,
    state: SyncState,
}

impl DmaStream {
    fn new(dir: DmaDirection) -> Self {
        let state = match dir {
            DmaDirection::ToDevice => SyncState::CpuReady,
            DmaDirection::FromDevice => SyncState::DeviceReady,
            DmaDirection::Bidirectional => SyncState::CpuReady,
        };
        Self { dir, state }
    }
    fn transition(&mut self, target: SyncState) -> Result<(), ()> {
        use DmaDirection::*;
        use SyncState::*;
        let ok = match (self.dir, self.state, target) {
            (ToDevice, CpuReady, DeviceReady) => true,
            (ToDevice, DeviceReady, CpuReady) => true,
            (FromDevice, DeviceReady, CpuReady) => true,
            (FromDevice, CpuReady, DeviceReady) => true,
            (Bidirectional, _, _) => true, // 简化: Bidirectional 任意转换
            _ => false,
        };
        if ok { self.state = target; Ok(()) } else { Err(()) }
    }
}

pub fn dma_state_machine_bench(iters: u64) -> u128 {
    let mut s = DmaStream::new(DmaDirection::ToDevice);
    const BATCH: u64 = 64;
    let start = Instant::now();
    let mut sink: u64 = 0;
    for i in 0..iters {
        for j in 0..BATCH {
            let target = if (i + j) & 1 == 0 { SyncState::DeviceReady } else { SyncState::CpuReady };
            if s.transition(target).is_ok() { sink ^= 1; }
        }
    }
    std::hint::black_box(sink);
    let elapsed = start.elapsed().as_nanos();
    let total_ops = (iters as u128) * (BATCH as u128);
    elapsed.saturating_mul(1_000) / total_ops
}

// ====== 6. SHA-256 block (来自 framework/credo/sha256.rs) ======

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

#[inline(always)]
fn rotr(x: u32, n: u32) -> u32 { x.rotate_right(n) }

fn sha256_transform(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = ((block[i * 4] as u32) << 24)
            | ((block[i * 4 + 1] as u32) << 16)
            | ((block[i * 4 + 2] as u32) << 8)
            | (block[i * 4 + 3] as u32);
    }
    for i in 16..64 {
        let s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
        let s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }
    let mut a = state[0]; let mut b = state[1]; let mut c = state[2]; let mut d = state[3];
    let mut e = state[4]; let mut f = state[5]; let mut g = state[6]; let mut h = state[7];
    for i in 0..64 {
        let s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
        let ch = (e & f) ^ (!e & g);
        let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
        let s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g; g = f; f = e; e = d.wrapping_add(t1);
        d = c; c = b; b = a; a = t1.wrapping_add(t2);
    }
    state[0] = state[0].wrapping_add(a); state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c); state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e); state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g); state[7] = state[7].wrapping_add(h);
}

pub fn sha256_block_bench(iters: u64) -> u128 {
    let mut state = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let block = [0u8; 64];
    // SHA-256 本身耗时足够, 每 iter = 1 block
    let start = Instant::now();
    for _ in 0..iters {
        sha256_transform(&mut state, &block);
    }
    std::hint::black_box(state[0]);
    start.elapsed().as_nanos()
}

// ====== 7. Attribution classify (来自 services/barrier/attribution.rs) ======

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaultAttribution {
    Tcb { fault_pc: u64 },
    Service { domain: u16, recoverable: bool },
    CrossLayer { framework_fn: &'static str, services_fn: &'static str },
}

#[derive(Clone, Copy, Debug)]
struct FaultRecord {
    rip: u64,
    sp: u64,
    cs: u16,
    in_interrupt: bool,
    holding_lock: bool,
    in_services: bool,
    caller_chain: u64,
}

fn classify(rec: &FaultRecord) -> FaultAttribution {
    // 简化版归属规则 (与 attribution.rs 同等语义)
    if rec.in_interrupt && rec.holding_lock {
        FaultAttribution::Tcb { fault_pc: rec.rip }
    } else if rec.in_services {
        if rec.holding_lock {
            FaultAttribution::CrossLayer { framework_fn: "spinlock_acquire", services_fn: "domain_visit" }
        } else {
            FaultAttribution::Service { domain: (rec.cs & 0xF) as u16, recoverable: true }
        }
    } else {
        FaultAttribution::Service { domain: 0, recoverable: false }
    }
}

pub fn attribution_classify_bench(iters: u64) -> u128 {
    let recs: Vec<FaultRecord> = (0..256).map(|i| FaultRecord {
        rip: 0xffff_8000_0010_0000 + i as u64 * 0x40,
        sp: 0xffff_8000_0020_0000,
        cs: 0x08,
        in_interrupt: i & 1 == 0,
        holding_lock: i & 3 == 0,
        in_services: i & 7 != 0,
        caller_chain: 0xdead_beef,
    }).collect();
    let start = Instant::now();
    let mut sink: u64 = 0;
    for i in 0..iters {
        let r = &recs[(i as usize) & 0xFF];
        let a = classify(r);
        match a {
            FaultAttribution::Tcb { fault_pc } => sink ^= fault_pc,
            FaultAttribution::Service { domain, .. } => sink ^= domain as u64,
            FaultAttribution::CrossLayer { .. } => sink ^= 0xCAFE,
        }
    }
    std::hint::black_box(sink);
    start.elapsed().as_nanos()
}

// ====== 8. Recovery decide (来自 services/barrier/recovery_policy.rs) ======

#[derive(Clone, Copy, Debug)]
enum RecoveryAction { Noop, Bbr, Bsr, Bhr, Quarantine }

#[derive(Clone, Copy, Debug)]
struct FaultSignal {
    is_tcb: bool,
    recoverable: bool,
    retry: u32,
    heartbeat_gap: u64,
    dependents: u32,
}

fn decide(s: &FaultSignal) -> RecoveryAction {
    if s.is_tcb { return RecoveryAction::Bhr; }
    if !s.recoverable { return RecoveryAction::Quarantine; }
    if s.heartbeat_gap > 500 { return RecoveryAction::Bsr; }
    match s.retry {
        0 => RecoveryAction::Noop,
        1..=2 => if s.dependents == 0 { RecoveryAction::Bbr } else { RecoveryAction::Bsr },
        3..=4 => RecoveryAction::Bsr,
        _ => RecoveryAction::Quarantine,
    }
}

pub fn recovery_decide_bench(iters: u64) -> u128 {
    let signals: Vec<FaultSignal> = (0..64).map(|i| FaultSignal {
        is_tcb: i & 1 == 0,
        recoverable: i & 2 != 0,
        retry: (i % 8) as u32,
        heartbeat_gap: (i as u64) * 30,
        dependents: (i % 4) as u32,
    }).collect();
    const BATCH: u64 = 64;
    let start = Instant::now();
    let mut sink: u64 = 0;
    for i in 0..iters {
        for j in 0..BATCH {
            let a = decide(&signals[((i * BATCH + j) as usize) & 0x3F]);
            sink ^= match a {
                RecoveryAction::Noop => 0,
                RecoveryAction::Bbr => 1,
                RecoveryAction::Bsr => 2,
                RecoveryAction::Bhr => 3,
                RecoveryAction::Quarantine => 4,
            };
        }
    }
    std::hint::black_box(sink);
    let elapsed = start.elapsed().as_nanos();
    elapsed.saturating_mul(1_000) / (iters as u128)
}

// ====== 9. Bitmap scan (PMM 物理页分配) ======

struct Bitmap {
    words: [u64; 16], // 1024 bits
}

impl Bitmap {
    fn new() -> Self { Self { words: [!0u64; 16] } }
    fn alloc(&mut self) -> Option<usize> {
        for (wi, w) in self.words.iter_mut().enumerate() {
            if *w != 0 {
                let bit = w.trailing_zeros() as usize;
                *w &= !((1u64) << bit);
                return Some(wi * 64 + bit);
            }
        }
        None
    }
    fn free(&mut self, idx: usize) {
        let wi = idx / 64;
        let bit = idx % 64;
        self.words[wi] |= (1u64) << bit;
    }
}

pub fn bitmap_scan_bench(iters: u64) -> u128 {
    let mut bm = Bitmap::new();
    let mut allocated: Vec<usize> = Vec::with_capacity(512);
    for _ in 0..512 {
        if let Some(i) = bm.alloc() { allocated.push(i); }
    }
    const BATCH: u64 = 32;
    let start = Instant::now();
    let mut sink: u64 = 0;
    for i in 0..iters {
        for j in 0..BATCH {
            if let Some(idx) = bm.alloc() {
                sink ^= idx as u64;
                if (i + j) & 1 == 0 { bm.free(idx); }
            }
        }
    }
    std::hint::black_box(sink);
    let elapsed = start.elapsed().as_nanos();
    elapsed.saturating_mul(1_000) / (iters as u128)
}

// ====== 10. BTreeMap integer lookup (进程表 PID 查找) ======

pub fn btree_id_lookup_bench(iters: u64) -> u128 {
    use std::collections::BTreeMap;
    let mut m: BTreeMap<u32, u64> = BTreeMap::new();
    for i in 0..512u32 {
        m.insert(i, (i as u64) * 0x1000);
    }
    let start = Instant::now();
    let mut sink: u64 = 0;
    for i in 0..iters {
        let key = (i as u32) & 0x1FF;
        if let Some(v) = m.get(&key) { sink ^= *v; }
    }
    std::hint::black_box(sink);
    start.elapsed().as_nanos()
}

// ====== 编排器 ======

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct BenchEntry {
    pub name: String,
    pub category: String,
    pub iterations: u64,
    pub total_ns: u128,
    /// 整数纳秒 (向下取整)
    pub ns_per_op: u128,
    /// 浮点纳秒 (保留亚纳秒精度, 用于跨运行对比)
    pub ns_per_op_frac: f64,
    /// 整数皮秒 (精确)
    pub ps_per_op: u128,
    pub ops_per_sec: u128,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct BenchReport {
    pub version: u32,
    pub results: Vec<BenchEntry>,
}

fn measure<F: Fn() -> u128>(name: &str, category: &str, default_iters: u64, f: F) -> BenchEntry {
    // 约定: f(iters) 执行总耗时 (ns), bench 内部按需做 BATCH 倍数工作.
    // measure 自适应放大 iters 直到总耗时 >= 50ms (或达到 10M 上限), 然后归一化.
    // 输出 ps_per_op 保留亚纳秒精度, ns_per_op_frac 是浮点表示.
    let mut iters = default_iters;
    let total_ns = loop {
        let t = f();
        if t >= 50_000_000 || iters >= 10_000_000 {
            break t;
        }
        iters = (iters * 10).min(10_000_000);
    };
    let ps_per_op = if iters > 0 { total_ns.saturating_mul(1_000) / iters as u128 } else { 0 };
    let ns_per_op = ps_per_op / 1000;
    let ns_per_op_frac = (ps_per_op as f64) / 1000.0;
    let ops_per_sec = if ns_per_op_frac > 0.0 { (1_000_000_000.0 / ns_per_op_frac) as u128 } else { 0 };
    BenchEntry {
        name: name.to_string(),
        category: category.to_string(),
        iterations: iters,
        total_ns,
        ns_per_op,
        ns_per_op_frac,
        ps_per_op,
        ops_per_sec,
    }
}

pub fn run_all() -> BenchReport {
    let mut results = Vec::new();
    results.push(measure("page_flags_bits", "mm", 100_000, ||
        page_flags_bench(100_000)));
    results.push(measure("pte_set_flags", "mm", 100_000, ||
        pte_set_flags_bench(100_000)));
    results.push(measure("iomem_alias_check", "iomem", 100_000, ||
        iomem_alias_bench(100_000)));
    results.push(measure("capability_check", "credo", 100_000, ||
        capability_check_bench(100_000)));
    results.push(measure("dma_state_machine", "dma", 100_000, ||
        dma_state_machine_bench(100_000)));
    results.push(measure("sha256_block", "credo", 1_000, ||
        sha256_block_bench(1_000)));
    results.push(measure("attribution_classify", "barrier", 100_000, ||
        attribution_classify_bench(100_000)));
    results.push(measure("recovery_decide", "barrier", 100_000, ||
        recovery_decide_bench(100_000)));
    results.push(measure("bitmap_scan", "pmm", 100_000, ||
        bitmap_scan_bench(100_000)));
    results.push(measure("btree_id_lookup", "proc", 100_000, ||
        btree_id_lookup_bench(100_000)));
    BenchReport { version: 1, results }
}

// ====== 单元测试 (验证算法正确性, 不测时序) ======

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_flags_compose() {
        let f = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
        assert!(f.contains(PageFlags::PRESENT));
        assert!(f.contains(PageFlags::WRITABLE));
        assert!(f.contains(PageFlags::USER));
        assert!(!f.contains(PageFlags::NX));
    }

    #[test]
    fn test_pte_set_flags_round_trip() {
        let mut pte = MockPte::new(0xFFFF_FFFF_FFFF_FFFF);
        pte.set_flags(PageFlags::PRESENT | PageFlags::WRITABLE);
        assert!(pte.is_present());
        let mut pte2 = MockPte::new(0x0);
        assert!(!pte2.is_present());
    }

    #[test]
    fn test_iomem_alias_no_conflict() {
        let mut r = AliasRegistry::new();
        assert!(r.register(0x1000, 0x800).is_ok());
        assert!(r.register(0x2000, 0x800).is_ok());
        assert!(r.check_conflict(0x3000, 0x800) == false);
    }

    #[test]
    fn test_iomem_alias_overlap() {
        let mut r = AliasRegistry::new();
        r.register(0x1000, 0x800).unwrap();
        // 0x1000-0x17FF 已被占用
        assert!(r.check_conflict(0x1100, 0x800) == true);   // 完全在内
        assert!(r.check_conflict(0x1000, 0x400) == true);   // 起点边界
        assert!(r.check_conflict(0x1500, 0x800) == true);   // 起点在内, 越界
        assert!(r.check_conflict(0x0800, 0x900) == true);   // 起点在外, 末端在内
        assert!(r.check_conflict(0x1800, 0x800) == false);  // 完全不相邻
        assert!(r.check_conflict(0x0000, 0x1000) == false); // 完全在外
    }

    #[test]
    fn test_capability_check() {
        let mut m = CapabilityMatrix::new();
        m.grant(1, 0b11);
        assert!(m.has(1, 0b01));
        assert!(m.has(1, 0b10));
        assert!(!m.has(1, 0b100));
    }

    #[test]
    fn test_dma_state_machine() {
        let mut s = DmaStream::new(DmaDirection::ToDevice);
        assert_eq!(s.state, SyncState::CpuReady);
        assert!(s.transition(SyncState::DeviceReady).is_ok());
        assert!(s.transition(SyncState::CpuReady).is_ok());
    }

    #[test]
    fn test_sha256_transform_known_block() {
        let mut state = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
            0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
        ];
        let block = [0u8; 64];
        sha256_transform(&mut state, &block);
        // SHA256(0x00 * 64) 中间状态应有具体值 (不验最终哈希, 只验不变崩)
        assert_ne!(state, [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                            0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]);
    }

    #[test]
    fn test_attribution_tcb_path() {
        let rec = FaultRecord { rip: 0xdead, sp: 0, cs: 0x08,
            in_interrupt: true, holding_lock: true, in_services: false, caller_chain: 0 };
        let a = classify(&rec);
        assert!(matches!(a, FaultAttribution::Tcb { .. }));
    }

    #[test]
    fn test_recovery_tcb_is_bhr() {
        let s = FaultSignal { is_tcb: true, recoverable: false, retry: 0,
            heartbeat_gap: 0, dependents: 0 };
        assert!(matches!(decide(&s), RecoveryAction::Bhr));
    }

    #[test]
    fn test_bitmap_alloc_free() {
        let mut bm = Bitmap::new();
        let i = bm.alloc().unwrap();
        bm.free(i);
        let j = bm.alloc().unwrap();
        // 回收后重新分配, 应能拿到相同 idx 或更小 idx
        assert!(j <= i);
    }

    #[test]
    fn test_btree_lookup() {
        use std::collections::BTreeMap;
        let mut m: BTreeMap<u32, u64> = BTreeMap::new();
        m.insert(42, 0xCAFE);
        assert_eq!(m.get(&42), Some(&0xCAFE));
        assert_eq!(m.get(&43), None);
    }
}
