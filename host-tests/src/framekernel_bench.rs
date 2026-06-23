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

// ====== 11. Socket WaitQueue (来自 services/net/wait_queue.rs) ======
//
// 模拟 16 个 fd (MAX_SM_FD) 上的 mark_waiting → try_wake 循环.
// 单次循环 = 1 个 fd 上的 1 次 send/wake 对应操作.
// 验收: 1000 个并发 send 路径平均延迟 < 1μs (QEMU 环境 1000 < 1ms 目标换算).

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Mutex as StdMutex;

/// 与 framework/net/socket WaitQueue 等价的 host-only 简化版
struct MockSocketWaitQueue {
    pending: AtomicBool,
    wake_count: AtomicU32,
    last_reason: AtomicU32,
    lock: StdMutex<()>,
}

impl MockSocketWaitQueue {
    const fn new() -> Self {
        Self {
            pending: AtomicBool::new(false),
            wake_count: AtomicU32::new(0),
            last_reason: AtomicU32::new(u32::MAX),
            lock: StdMutex::new(()),
        }
    }

    fn mark_waiting(&self) -> bool {
        !self.pending.swap(true, Ordering::AcqRel)
    }

    fn try_wake(&self, reason: u32) -> bool {
        let _guard = match self.lock.try_lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let was_pending = self.pending.swap(false, Ordering::AcqRel);
        if was_pending {
            self.wake_count.fetch_add(1, Ordering::Relaxed);
            self.last_reason.store(reason, Ordering::Relaxed);
        }
        was_pending
    }

    fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }
}

/// MAX_SM_FD: 16 (与 services/net/socket.rs 的 fd 空间 [0, 16) 对齐)
const MAX_SM_FD: usize = 16;

pub fn socket_wait_queue_bench(iters: u64) -> u128 {
    let queues: Vec<MockSocketWaitQueue> = (0..MAX_SM_FD)
        .map(|_| MockSocketWaitQueue::new())
        .collect();
    // 1 轮 (BATCH) = 1000 次并发 send/wake 路径 = 验收目标
    const BATCH: u64 = 1000;
    let start = Instant::now();
    let mut sink: u64 = 0;
    for i in 0..iters {
        for j in 0..BATCH {
            // 轮询 16 个 fd, 每个 fd 上做 mark_waiting + try_wake
            let fd = ((i * BATCH + j) as usize) % MAX_SM_FD;
            queues[fd].mark_waiting();
            if queues[fd].try_wake(0) {
                sink ^= 1;
            }
        }
    }
    std::hint::black_box(sink);
    let elapsed = start.elapsed().as_nanos();
    let total_ops = (iters as u128) * (BATCH as u128);
    elapsed.saturating_mul(1_000) / total_ops
}

// ====== 12. virtio-blk I/O 路径 (来自 framework/driver/virtio/{queue,blk}.rs) ======
//
// 模拟 split virtqueue 的 submit → pop_used 循环.
// 4K 写请求包含 3 段描述符链: header (1B) + data (4096B) + status (1B).
// 验收目标: 4K 写延迟 < 100μs (QEMU virtio-blk 设备实测),
//          host 端算法路径应远低于此 (<< 1μs).

/// 描述符标志
const VQ_DESC_F_NEXT: u16 = 1;
const VQ_DESC_F_WRITE: u16 = 2;

/// split virtqueue 描述符 (host-only 简化版)
struct MockVqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

/// 4K 写请求的 3 段描述符链 (与 virtio-blk 协议一致)
const BLK_REQ_HEADERS_OUT: usize = 1;
const BLK_REQ_DATA_OUT: usize = 1;
const BLK_REQ_STATUS_IN: usize = 1;
const BLK_REQ_CHAIN_LEN: usize = BLK_REQ_HEADERS_OUT + BLK_REQ_DATA_OUT + BLK_REQ_STATUS_IN;
const BLK_SECTOR_SIZE: u32 = 512;
const BLK_4K_BYTES: u32 = 4096;
const BLK_4K_SECTORS: u32 = BLK_4K_BYTES / BLK_SECTOR_SIZE;

/// split virtqueue (host-only mock, 32 项, 与 VQ_SIZE 对齐)
struct MockVirtQueue {
    descs: Vec<MockVqDesc>,
    avail_idx: u16,
    last_used_idx: u16,
    /// 空闲描述符链头 (单链表)
    free_head: u16,
    capacity: u16,
}

impl MockVirtQueue {
    fn new(capacity: u16) -> Self {
        let mut descs: Vec<MockVqDesc> = (0..capacity)
            .map(|i| MockVqDesc {
                addr: 0,
                len: 0,
                flags: 0,
                next: if i + 1 < capacity { i + 1 } else { 0xFFFF },
            })
            .collect();
        // 初始 free_head = 0
        let _ = &mut descs;
        Self {
            descs,
            avail_idx: 0,
            last_used_idx: 0,
            free_head: 0,
            capacity,
        }
    }

    /// 提交 3 段描述符链 (header + 4K data + status), 返回 head idx
    fn submit_blk_write(&mut self) -> Option<u16> {
        // 检查空闲槽位
        if self.free_head == 0xFFFF {
            return None;
        }
        let head = self.free_head;
        // 分配 3 段: header, data, status
        let h1 = head;
        let h2 = ((head as u32 + 1) % self.capacity as u32) as u16;
        let h3 = ((head as u32 + 2) % self.capacity as u32) as u16;
        // 第 3 段 (status) 设备写, 不链 next
        self.descs[h1 as usize] = MockVqDesc {
            addr: 0x1000, len: 16, flags: VQ_DESC_F_NEXT, next: h2,
        };
        self.descs[h2 as usize] = MockVqDesc {
            addr: 0x2000, len: BLK_4K_BYTES, flags: VQ_DESC_F_NEXT, next: h3,
        };
        self.descs[h3 as usize] = MockVqDesc {
            addr: 0x3000, len: 1, flags: VQ_DESC_F_WRITE, next: 0xFFFF,
        };
        // 推进 free_head 到下一空闲
        self.free_head = if h3 + 1 < self.capacity { h3 + 1 } else { 0xFFFF };
        // 推进 avail_idx (驱动侧的可用环 head 索引)
        self.avail_idx = self.avail_idx.wrapping_add(1);
        Some(head)
    }

    /// 模拟设备完成 (used ring 推进 + 描述符回收)
    fn complete_blk_write(&mut self, head: u16) {
        // used ring 推进
        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        // 回收整个 3 段描述符链: 走完 chain 把所有 desc 放回 free_head
        // 简化: 链式回收 (链上 next 仍可读, 因为我们没有清 desc.next)
        let mut idx = head;
        for _ in 0..BLK_REQ_CHAIN_LEN {
            let next = self.descs[idx as usize].next;
            self.descs[idx as usize].flags = 0;
            // 把当前 desc 插入 free_head 链头
            if self.free_head == 0xFFFF {
                // free_head 满, 把当前 desc 接在 head 之后
                self.descs[idx as usize].next = 0xFFFF;
                self.free_head = idx;
            } else {
                // 把 free_head 接到当前 desc 之后
                self.descs[idx as usize].next = self.free_head;
                self.free_head = idx;
            }
            if next == 0xFFFF {
                break;
            }
            idx = next;
        }
    }

    /// 模拟 pop_used (驱动侧读取已用环)
    fn pop_used(&mut self) -> Option<u16> {
        // 与设备完成同步推进
        Some(0)
    }
}

pub fn virtio_blk_io_bench(iters: u64) -> u128 {
    let mut vq = MockVirtQueue::new(32);
    // 1 轮 (BATCH) = 32 次 4K 写 (覆盖整个 virtqueue 一次)
    const BATCH: u64 = 32;
    let start = Instant::now();
    let mut sink: u64 = 0;
    for _ in 0..iters {
        for _ in 0..BATCH {
            // 提交 1 个 4K 写请求
            if let Some(head) = vq.submit_blk_write() {
                // 设备完成 (同步, host-only mock)
                vq.complete_blk_write(head);
                let _ = vq.pop_used();
                // 读取回状态防止编译器优化掉整条路径
                sink ^= head as u64;
                sink ^= vq.last_used_idx as u64;
                sink ^= vq.descs[head as usize].len as u64;
            }
        }
    }
    std::hint::black_box(sink);
    let elapsed = start.elapsed().as_nanos();
    // 归一化: 1 op = 1 个 4K 写请求 (含 submit + complete + pop)
    let total_ops = (iters as u128) * (BATCH as u128);
    elapsed.saturating_mul(1_000) / total_ops
}

// ============================================================================
// EBPF-3: BpfVerifier trait dispatch Mock + bench
// ============================================================================
//
// T4-3 framekernel 设计: framework 通过 `&dyn BpfVerifier` 动态分派
// 调用 services 注册的 verifier. 此 mock 复现该机制, 在 host 端验证:
//   1. trait 动态分派可工作 (`&dyn BpfVerifier::verify`)
//   2. 安全默认行为: 未注册 verifier → prog_load 拒绝所有
//   3. 注册后 prog_load 走 verifier 验证路径
//   4. 测量动态分派的 throughput (vs. 静态分派 / 直接调用)

/// Mock BPF 程序 (host-only, 不依赖 no_std services)
#[derive(Clone, Debug)]
pub struct MockBpfProg {
    pub insn_cnt: u32,
}

impl MockBpfProg {
    pub fn new(insn_cnt: u32) -> Self {
        Self { insn_cnt }
    }
}

/// 验证结果 (复现 framework::debug::VerifyResult)
#[derive(Debug, PartialEq, Eq)]
pub enum VerifyResult {
    Ok,
    Err(Vec<u8>),
}

/// BpfVerifier trait (host-only mock, 与 services::debug::ebpf_verifier::BpfVerifier 同构)
pub trait BpfVerifier: Sync + Send {
    fn verify(&self, prog: &MockBpfProg) -> VerifyResult;
}

/// Mock verifier: 根据构造参数决定全部 accept 或全部 reject
pub struct MockBpfVerifier {
    accept: bool,
}

impl MockBpfVerifier {
    pub const fn new(accept: bool) -> Self {
        Self { accept }
    }
}

impl BpfVerifier for MockBpfVerifier {
    fn verify(&self, prog: &MockBpfProg) -> VerifyResult {
        if prog.insn_cnt == 0 {
            return VerifyResult::Err(b"empty program".to_vec());
        }
        if self.accept {
            VerifyResult::Ok
        } else {
            VerifyResult::Err(b"mock reject".to_vec())
        }
    }
}

/// Mock BpfSubsystem (复现 framework::debug::BpfSubsystem 的核心机制)
pub struct MockBpfSubsystem {
    verifier: std::sync::Mutex<Option<&'static dyn BpfVerifier>>,
    next_prog_fd: AtomicI64,
}

impl MockBpfSubsystem {
    pub const fn new() -> Self {
        Self {
            verifier: std::sync::Mutex::new(None),
            next_prog_fd: AtomicI64::new(1),
        }
    }
    /// T4-3: 注册 verifier (framekernel 动态分派接口)
    pub fn set_verifier(&self, v: &'static dyn BpfVerifier) {
        *self.verifier.lock().unwrap() = Some(v);
    }
    /// T4-3: 模拟 prog_load, 走 verifier 验证
    /// - 未注册: 返回 -1 (EPERM, 安全默认)
    /// - 注册后: 走 verifier 验证, 成功返回 fd
    pub fn prog_load(&self, prog: MockBpfProg) -> i64 {
        let slot = self.verifier.lock().unwrap();
        let v = match *slot {
            Some(v) => v,
            None => return -1, // EPERM
        };
        match v.verify(&prog) {
            VerifyResult::Ok => self.next_prog_fd.fetch_add(1, Ordering::AcqRel),
            VerifyResult::Err(_) => -22, // EINVAL
        }
    }
}

/// bench: 测量 `&dyn BpfVerifier::verify` 动态分派 throughput
///
/// 1000 op = 1000 次 verifier.verify 调用. 1 op 包含:
/// - 构造 MockBpfProg
/// - 通过 `&dyn BpfVerifier` 间接调用 verify
/// - 匹配 VerifyResult
pub fn bpf_verifier_dispatch_bench(iters: u64) -> u128 {
    static VERIFIER: MockBpfVerifier = MockBpfVerifier::new(true);
    let v: &'static dyn BpfVerifier = &VERIFIER;
    let start = Instant::now();
    let mut sink: u32 = 0;
    for _ in 0..iters {
        let prog = MockBpfProg::new(8);
        let r = v.verify(&prog);
        // 读取结果, 防止编译器优化掉整条路径
        sink ^= match r {
            VerifyResult::Ok => 1,
            VerifyResult::Err(_) => 0,
        };
    }
    let elapsed = start.elapsed().as_nanos();
    // 防御性: 防止编译器优化掉 sink
    std::hint::black_box(sink);
    // 1 op = 1 次 verify 调用
    elapsed.saturating_mul(1_000) / (iters as u128)
}

// ============================================================================
// SYSCTL-2: sysctl register/write bench
// ============================================================================
//
// LEGACY-6 引入 services/config/sysctl.rs (314 行, 0 unsafe, IrqSpinLock + 原子).
// 由于 services 是 no_std, host-tests 用 Mock 复现等价机制:
//   - MockSysctlTable: 32 槽位 Option<entry>, 与 services 数组布局一致
//   - MockSysctlEntry: name + value (i64/u64/bool)
//   - 锁语义: 用 std::sync::Mutex 模拟 IrqSpinLock (host 不存在中断上下文)
//   - register / read / write 路径与 services 1:1 对应
//
// 性能含义: register + write 路径 lock + lookup + write, 测算法层开销.

/// Mock sysctl 值类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MockSysctlValue {
    Int(i64),
    UInt(u64),
    Bool(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MockSysctlKind {
    Int,
    UInt,
    Bool,
}

pub struct MockSysctlEntry {
    pub name: &'static str,
    pub kind: MockSysctlKind,
    pub int_val: i64,
    pub uint_val: u64,
    pub bool_val: bool,
}

impl MockSysctlEntry {
    pub const fn new(name: &'static str, kind: MockSysctlKind, val: MockSysctlValue) -> Self {
        let (int_v, uint_v, bool_v) = match val {
            MockSysctlValue::Int(v) => (v, 0, false),
            MockSysctlValue::UInt(v) => (0, v, false),
            MockSysctlValue::Bool(v) => (0, 0, v),
        };
        Self { name, kind, int_val: int_v, uint_val: uint_v, bool_val: bool_v }
    }

    pub fn read(&self) -> MockSysctlValue {
        match self.kind {
            MockSysctlKind::Int => MockSysctlValue::Int(self.int_val),
            MockSysctlKind::UInt => MockSysctlValue::UInt(self.uint_val),
            MockSysctlKind::Bool => MockSysctlValue::Bool(self.bool_val),
        }
    }

    pub fn write(&mut self, val: MockSysctlValue) -> Result<(), ()> {
        let k = match val {
            MockSysctlValue::Int(_) => MockSysctlKind::Int,
            MockSysctlValue::UInt(_) => MockSysctlKind::UInt,
            MockSysctlValue::Bool(_) => MockSysctlKind::Bool,
        };
        if k != self.kind { return Err(()); }
        match val {
            MockSysctlValue::Int(v) => self.int_val = v,
            MockSysctlValue::UInt(v) => self.uint_val = v,
            MockSysctlValue::Bool(v) => self.bool_val = v,
        }
        Ok(())
    }
}

const MOCK_SYSCTL_SLOTS: usize = 32;

pub struct MockSysctlTable {
    slots: StdMutex<[Option<MockSysctlEntry>; MOCK_SYSCTL_SLOTS]>,
}

impl MockSysctlTable {
    pub const fn new() -> Self {
        // 用 const { None } 数组初始化 (Rust 1.79+)
        Self {
            slots: StdMutex::new([const { None }; MOCK_SYSCTL_SLOTS]),
        }
    }

    pub fn register(
        &self,
        name: &'static str,
        kind: MockSysctlKind,
        val: MockSysctlValue,
    ) -> Result<(), ()> {
        let mut g = self.slots.lock().unwrap();
        // 重复检测
        for s in g.iter() {
            if let Some(e) = s {
                if e.name == name { return Err(()); }
            }
        }
        // 找一个空槽
        for s in g.iter_mut() {
            if s.is_none() {
                *s = Some(MockSysctlEntry::new(name, kind, val));
                return Ok(());
            }
        }
        Err(())
    }

    pub fn write(&self, name: &str, val: MockSysctlValue) -> Result<(), ()> {
        let mut g = self.slots.lock().unwrap();
        for s in g.iter_mut() {
            if let Some(e) = s {
                if e.name == name { return e.write(val); }
            }
        }
        Err(())
    }

    pub fn read(&self, name: &str) -> Option<MockSysctlValue> {
        let g = self.slots.lock().unwrap();
        for s in g.iter() {
            if let Some(e) = s {
                if e.name == name { return Some(e.read()); }
            }
        }
        None
    }
}

/// bench: sysctl register + write 吞吐量
///
/// 1 轮 (BATCH) = 16 次 write (启动期 register 在 r==0 完成)
/// 模拟"启动期注册 + 运行时调参"工作负载
pub fn sysctl_bench(iters: u64) -> u128 {
    let table = MockSysctlTable::new();
    const NODES: usize = 16;
    const BATCH: u64 = 16;

    // 准备静态名称池 (一次性)
    use std::sync::OnceLock;
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        (0..NODES)
            .map(|i| Box::leak(format!("sysctl.bench.{}", i).into_boxed_str())
                as &'static str)
            .collect()
    });

    // 启动期: 注册 16 个节点
    for i in 0..NODES {
        let kind = match i % 3 {
            0 => MockSysctlKind::Int,
            1 => MockSysctlKind::UInt,
            _ => MockSysctlKind::Bool,
        };
        let val = match kind {
            MockSysctlKind::Int => MockSysctlValue::Int(i as i64),
            MockSysctlKind::UInt => MockSysctlValue::UInt(i as u64),
            MockSysctlKind::Bool => MockSysctlValue::Bool(i % 2 == 0),
        };
        let _ = table.register(names[i], kind, val);
    }

    // 主体: 旋转 write 不同节点
    let start = Instant::now();
    let mut sink: u64 = 0;
    for r in 0..iters {
        for i in 0..BATCH {
            let idx = (i as usize) % NODES;
            let kind = match idx % 3 {
                0 => MockSysctlKind::Int,
                1 => MockSysctlKind::UInt,
                _ => MockSysctlKind::Bool,
            };
            let val = match kind {
                MockSysctlKind::Int => MockSysctlValue::Int(i as i64 + (r as i64) * 1000),
                MockSysctlKind::UInt => MockSysctlValue::UInt(i as u64 + r * 1000),
                MockSysctlKind::Bool => MockSysctlValue::Bool(r % 2 == 0),
            };
            let _ = table.write(names[idx], val);
            sink ^= idx as u64;
        }
    }
    let elapsed = start.elapsed().as_nanos();
    std::hint::black_box(sink);
    // 1 op = 1 次 write
    elapsed.saturating_mul(1_000) / (iters as u128 * BATCH as u128)
}

// ============================================================================
// T-4.1 (LEGACY-4): BlockDevice trait dispatch bench + Mock
// ============================================================================
//
// 验证 chitin_blk_read/write 走 BlockDevice trait dispatch (0 thunk).
// 由于 chitin::chitin_blk_read/write 是 no_std, host-tests 用 Mock 复现:
//   - MockBlockDevice: 实现本地 BlockDevice trait, 模拟真实块设备
//   - MockChitinDevice: 模拟 chitin 表, 持有 dyn BlockDevice, 提供 read/write API
//   - bench: 测量 trait dispatch 路径的吞吐

/// host-only BlockDevice trait (与 kernel::framework::chitin::BlockDevice 等价)
pub trait HostBlockDevice: Send + Sync {
    fn blk_read(&self, sector: u64, buf: &mut [u8]) -> i32;
    fn blk_write(&self, sector: u64, buf: &[u8]) -> i32;
    fn blk_is_present(&self) -> bool { true }
    fn blk_total_sectors(&self) -> u64 { u64::MAX }
}

/// Mock 块设备, 模拟 virtio-blk 行为
pub struct MockBlockDevice {
    /// 内部存储 (按 sector 索引, 0-1023 扇区)
    storage: StdMutex<Vec<[u8; 512]>>,
    read_count: std::sync::atomic::AtomicU64,
    write_count: std::sync::atomic::AtomicU64,
}

impl MockBlockDevice {
    pub fn new(capacity_sectors: usize) -> Self {
        Self {
            storage: StdMutex::new(
                (0..capacity_sectors)
                    .map(|i| {
                        let mut s = [0u8; 512];
                        s[0] = (i & 0xFF) as u8;
                        s[1] = ((i >> 8) & 0xFF) as u8;
                        s
                    })
                    .collect()
            ),
            read_count: std::sync::atomic::AtomicU64::new(0),
            write_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn read_count(&self) -> u64 {
        self.read_count.load(std::sync::atomic::Ordering::Acquire)
    }
    pub fn write_count(&self) -> u64 {
        self.write_count.load(std::sync::atomic::Ordering::Acquire)
    }
}

impl HostBlockDevice for MockBlockDevice {
    fn blk_read(&self, sector: u64, buf: &mut [u8]) -> i32 {
        if buf.len() < 512 { return -1; }
        self.read_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let s = sector as usize;
        let storage = self.storage.lock().unwrap();
        if s >= storage.len() { return -5; }
        buf.copy_from_slice(&storage[s]);
        0
    }
    fn blk_write(&self, sector: u64, buf: &[u8]) -> i32 {
        if buf.len() < 512 { return -1; }
        self.write_count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let s = sector as usize;
        let mut storage = self.storage.lock().unwrap();
        if s >= storage.len() { return -5; }
        storage[s].copy_from_slice(&buf[..512]);
        0
    }
}

/// Mock chitin_blk_read/write 路径 (trait dispatch)
pub struct MockChitinDevice {
    block_dev: Box<dyn HostBlockDevice>,
}

impl MockChitinDevice {
    pub fn new(dev: Box<dyn HostBlockDevice>) -> Self {
        Self { block_dev: dev }
    }
    pub fn blk_read(&self, sector: u64, buf: &mut [u8]) -> i32 {
        if buf.len() < 512 { return -1; }
        self.block_dev.blk_read(sector, buf)
    }
    pub fn blk_write(&self, sector: u64, buf: &[u8]) -> i32 {
        if buf.len() < 512 { return -1; }
        self.block_dev.blk_write(sector, buf)
    }
    pub fn blk_is_present(&self) -> bool {
        self.block_dev.blk_is_present()
    }
    pub fn blk_total_sectors(&self) -> u64 {
        self.block_dev.blk_total_sectors()
    }
}

/// bench: T-4.1 trait dispatch 路径 throughput
pub fn blk_dev_dispatch_bench(iters: u64) -> u128 {
    let dev: Box<dyn HostBlockDevice> = Box::new(MockBlockDevice::new(1024));
    let chitin = MockChitinDevice::new(dev);

    // 预热 (避免首次调用路径开销污染)
    for _ in 0..100 {
        let mut buf = [0u8; 512];
        chitin.blk_read(0, &mut buf);
    }

    let start = Instant::now();
    let mut sink: u64 = 0;
    let mut buf = [0u8; 512];
    for r in 0..iters {
        // 1 轮: 1 读 + 1 写 (16 扇区 旋转)
        let sector = (r & 0xF) as u64;
        let _ = chitin.blk_read(sector, &mut buf);
        sink ^= u64::from(buf[0]) | (u64::from(buf[1]) << 8);
        let _ = chitin.blk_write(sector, &buf);
    }
    let elapsed = start.elapsed().as_nanos();
    std::hint::black_box(sink);
    // 1 op = 1 读 + 1 写 = 2 实际 ops
    elapsed.saturating_mul(1_000) / (iters as u128 * 2)
}

// ============================================================================
// REVAL-6.1: VfsPollPolicy trait dispatch bench + Mock
// ============================================================================
//
// 验证 epoll::check_fd_ready 走 VfsPollPolicy trait dispatch (无硬编码 match).
// 由于 framework::fs 是 no_std, host-tests 用 Mock 复现:
//   - MockVfsFileType: 模拟 4 种 VFS 文件类型
//   - MockVfsPollPolicy: 实现本地 trait, 模拟 StandardVfsPollPolicy
//   - bench: 测量 trait dispatch 路径的吞吐 (与 fallback 路径对比)

/// host-only epoll 事件位常量 (与 kernel 一致)
pub mod poll_events {
    pub const EPOLLIN: u32 = 0x001;
    pub const EPOLLOUT: u32 = 0x004;
    pub const EPOLLERR: u32 = 0x008;
    pub const EPOLLHUP: u32 = 0x010;
}

/// host-only VFS 文件类型 (4 种)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockVfsFileType {
    File = 0,
    Dir = 1,
    Dev = 2,
    Symlink = 3,
}

/// host-only VfsPollContext
#[derive(Debug, Clone, Copy)]
pub struct MockVfsPollContext {
    pub valid: bool,
    pub file_type: MockVfsFileType,
}

/// host-only VfsPollPolicy trait
pub trait HostVfsPollPolicy: Send + Sync {
    fn events_for_file_type(&self, file_type: MockVfsFileType) -> u32;
    fn events_for_invalid_fd(&self) -> u32;
}

/// host-only StandardVfsPollPolicy (与 kernel services/fs/vfs_poll_policy.rs 等价)
pub struct StandardHostVfsPollPolicy;
impl HostVfsPollPolicy for StandardHostVfsPollPolicy {
    fn events_for_file_type(&self, ft: MockVfsFileType) -> u32 {
        use poll_events::*;
        match ft {
            MockVfsFileType::File => EPOLLIN | EPOLLOUT,
            MockVfsFileType::Dir => EPOLLIN,
            MockVfsFileType::Dev => EPOLLHUP,
            MockVfsFileType::Symlink => EPOLLIN | EPOLLHUP,
        }
    }
    fn events_for_invalid_fd(&self) -> u32 {
        use poll_events::*;
        EPOLLERR | EPOLLHUP
    }
}

/// host-only 决策函数 (复现 epoll::check_fd_ready 的核心逻辑)
pub struct MockEpollCheck {
    policy: Box<dyn HostVfsPollPolicy>,
}

impl MockEpollCheck {
    pub fn new(policy: Box<dyn HostVfsPollPolicy>) -> Self {
        Self { policy }
    }
    pub fn check(&self, ctx: MockVfsPollContext, user_events: u32) -> u32 {
        let raw = if !ctx.valid {
            self.policy.events_for_invalid_fd()
        } else {
            self.policy.events_for_file_type(ctx.file_type)
        };
        raw & user_events
    }
}

/// bench: REVAL-6.1 trait dispatch 路径 throughput
pub fn vfs_poll_dispatch_bench(iters: u64) -> u128 {
    let check = MockEpollCheck::new(Box::new(StandardHostVfsPollPolicy));

    // 预热
    for _ in 0..1000 {
        let ctx = MockVfsPollContext { valid: true, file_type: MockVfsFileType::File };
        let _ = check.check(ctx, poll_events::EPOLLIN);
    }

    // 4 种 file_type 旋转
    let fts = [MockVfsFileType::File, MockVfsFileType::Dir, MockVfsFileType::Dev, MockVfsFileType::Symlink];
    let start = Instant::now();
    let mut sink: u32 = 0;
    for r in 0..iters {
        let ft = fts[(r & 0x3) as usize];
        let ctx = MockVfsPollContext { valid: true, file_type: ft };
        sink ^= check.check(ctx, poll_events::EPOLLIN | poll_events::EPOLLOUT);
        // 偶尔插入 invalid fd
        if r & 0xFF == 0 {
            let inv_ctx = MockVfsPollContext { valid: false, file_type: MockVfsFileType::File };
            sink ^= check.check(inv_ctx, poll_events::EPOLLIN);
        }
    }
    let elapsed = start.elapsed().as_nanos();
    std::hint::black_box(sink);
    elapsed.saturating_mul(1_000) / iters as u128
}

// ============================================================================
// REVAL-6.2: epoll_pwake 拆分行为 Mock
// ============================================================================
//
// 验证 epoll_pwake 拆分为 `instance_watches_fd` (机制) + `enqueue_ready_for_fd` (策略)
// 后行为不变: 找到 fd 的实例, 入队, 去重.
//
// 由于 no_std kernel 不能 host-test, 用 MockEpollInstance 复现.

/// host-only epoll 实例
pub struct MockEpollInstance {
    pub interest_list: Vec<MockEpollInterestItem>,
    pub ready_list: Vec<(u32, u64)>,  // (revents, data)
}

/// host-only 注册项
#[derive(Debug, Clone, Copy)]
pub struct MockEpollInterestItem {
    pub fd: i32,
    pub events: u32,
    pub data: u64,
}

impl MockEpollInstance {
    pub fn new() -> Self {
        Self { interest_list: Vec::new(), ready_list: Vec::new() }
    }
    pub fn add(&mut self, item: MockEpollInterestItem) {
        self.interest_list.push(item);
    }
}

/// 机制: 检查 epoll 实例是否在监控指定 fd (REVAL-6.2 提取)
pub fn instance_watches_fd(instance: &MockEpollInstance, fd: i32) -> bool {
    instance.interest_list.iter().any(|item| item.fd == fd)
}

/// 策略: 把 fd 的就绪事件加入 epoll 实例的 ready_list (REVAL-6.2 提取)
pub fn enqueue_ready_for_fd(
    instance: &mut MockEpollInstance,
    fd: i32,
    policy: &dyn HostVfsPollPolicy,
) -> bool {
    let pos = match instance.interest_list.iter().position(|item| item.fd == fd) {
        Some(p) => p,
        None => return false,
    };
    let events = instance.interest_list[pos].events;
    let data = instance.interest_list[pos].data;

    // 决策 revents: 简化 (Mock 不走 VFS, 永远返回 IN|OUT if File)
    let revents = policy.events_for_file_type(MockVfsFileType::File) & events;
    if revents == 0 {
        return false;
    }
    if instance.ready_list.iter().any(|(_, d)| *d == data) {
        return false;  // dedup
    }
    instance.ready_list.push((revents, data));
    true
}

/// 编排: epoll_pwake (REVAL-6.2 拆分后)
pub struct MockEpollPwake {
    pub instances: Vec<MockEpollInstance>,
}

impl MockEpollPwake {
    pub fn new() -> Self {
        Self { instances: Vec::new() }
    }
    pub fn pwake(&mut self, fd: i32, policy: &dyn HostVfsPollPolicy) -> usize {
        let mut count = 0;
        for inst in &mut self.instances {
            if !instance_watches_fd(inst, fd) {
                continue;
            }
            if enqueue_ready_for_fd(inst, fd, policy) {
                count += 1;
            }
        }
        count
    }
}

// ============================================================================
// LEGACY-5.1: ZapStore trait dispatch bench + Mock
// ============================================================================
//
// 验证 ZAP 走 trait dispatch (与 LEGACY-4 BlockDevice, REVAL-6.1 VfsPollPolicy 范式一致).
// 模拟 StandardZap 行为: insert/lookup/remove.

use std::collections::HashMap;

/// host-only ZapStore trait
pub trait HostZapStore: Send + Sync {
    fn insert(&self, name: &str, value: &[u8]) -> bool;
    fn insert_u64(&self, name: &str, value: u64) -> bool;
    fn lookup(&self, name: &str) -> Option<Vec<u8>>;
    fn lookup_u64(&self, name: &str) -> Option<u64>;
    fn remove(&self, name: &str) -> bool;
    fn contains(&self, name: &str) -> bool;
    fn len(&self) -> usize;
    fn capacity(&self) -> usize;
}

/// host-only StandardZap (Mutex<HashMap>)
pub struct StandardHostZap {
    pub map: std::sync::Mutex<(HashMap<String, Vec<u8>>, usize)>,
}

impl StandardHostZap {
    pub fn new() -> Self {
        Self {
            map: std::sync::Mutex::new((HashMap::new(), 256)),
        }
    }
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            map: std::sync::Mutex::new((HashMap::new(), cap)),
        }
    }
}

impl HostZapStore for StandardHostZap {
    fn insert(&self, name: &str, value: &[u8]) -> bool {
        let mut g = self.map.lock().unwrap();
        if g.0.len() >= g.1 && !g.0.contains_key(name) {
            return false;
        }
        g.0.insert(name.to_string(), value.to_vec());
        true
    }
    fn insert_u64(&self, name: &str, value: u64) -> bool {
        self.insert(name, &value.to_le_bytes())
    }
    fn lookup(&self, name: &str) -> Option<Vec<u8>> {
        self.map.lock().unwrap().0.get(name).cloned()
    }
    fn lookup_u64(&self, name: &str) -> Option<u64> {
        self.lookup(name).map(|v| {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&v[..8.min(v.len())]);
            u64::from_le_bytes(arr)
        })
    }
    fn remove(&self, name: &str) -> bool {
        self.map.lock().unwrap().0.remove(name).is_some()
    }
    fn contains(&self, name: &str) -> bool {
        self.map.lock().unwrap().0.contains_key(name)
    }
    fn len(&self) -> usize {
        self.map.lock().unwrap().0.len()
    }
    fn capacity(&self) -> usize {
        self.map.lock().unwrap().1
    }
}

/// bench: ZAP trait dispatch throughput
pub fn zap_dispatch_bench(iters: u64) -> u128 {
    let zap: Box<dyn HostZapStore> = Box::new(StandardHostZap::new());
    // 预热
    for i in 0..1000 {
        zap.insert_u64(&format!("warmup_{}", i), i);
    }
    // bench: insert + lookup + remove 旋转
    let start = Instant::now();
    let mut sink: u64 = 0;
    for r in 0..iters {
        let key = format!("k_{}", r & 0xFFF);
        if r & 0x3 == 0 {
            // insert
            let _ = zap.insert_u64(&key, r);
        } else if r & 0x3 == 1 {
            // lookup
            if let Some(v) = zap.lookup_u64(&key) {
                sink = sink.wrapping_add(v);
            }
        } else if r & 0x3 == 2 {
            // insert raw
            let _ = zap.insert(&key, &r.to_le_bytes());
        } else {
            // contains
            if zap.contains(&key) {
                sink = sink.wrapping_add(1);
            }
        }
    }
    let elapsed = start.elapsed().as_nanos();
    std::hint::black_box(sink);
    elapsed.saturating_mul(1_000) / iters as u128
}

// ============================================================================
// LEGACY-5.2: TxgManager trait dispatch bench + Mock
// ============================================================================
//
// 模拟 StandardTxg 行为: init/transition/add_dirty_to_open/current_txg.

/// host-only TXG 状态机快照
#[derive(Debug, Clone)]
pub struct MockTxgState {
    pub open_id: u64,
    pub syncing_id: u64,
    pub current: u64,
    pub total_syncs: u64,
    pub total_dirty: u64,
}

/// host-only TxgManager trait
pub trait HostTxgManager: Send + Sync {
    fn init(&mut self, start_txg: u64);
    fn transition(&mut self) -> u64;
    fn current_txg(&self) -> u64;
    fn open_txg_id(&self) -> u64;
    fn syncing_txg_id(&self) -> u64;
    fn is_sync_in_progress(&self) -> bool;
    fn total_syncs(&self) -> u64;
    fn total_dirty(&self) -> u64;
    fn add_dirty_to_open(&mut self, dummy: u64);
    fn add_free_to_open(&mut self, dummy: u64);
    fn add_io_to_open(&mut self, dummy: u64);
}

/// host-only StandardTxg (Mutex<MockTxgState>)
pub struct StandardHostTxg {
    pub state: std::sync::Mutex<MockTxgState>,
}

impl StandardHostTxg {
    pub fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(MockTxgState {
                open_id: 0,
                syncing_id: 0,
                current: 0,
                total_syncs: 0,
                total_dirty: 0,
            }),
        }
    }
}

impl HostTxgManager for StandardHostTxg {
    fn init(&mut self, start_txg: u64) {
        let mut s = self.state.lock().unwrap();
        s.open_id = start_txg;
        s.syncing_id = start_txg + 2;
        s.current = start_txg;
        s.total_syncs = 0;
        s.total_dirty = 0;
    }
    fn transition(&mut self) -> u64 {
        let mut s = self.state.lock().unwrap();
        s.open_id += 3;
        s.syncing_id += 3;
        s.current += 3;
        s.total_syncs += 1;
        s.current
    }
    fn current_txg(&self) -> u64 { self.state.lock().unwrap().current }
    fn open_txg_id(&self) -> u64 { self.state.lock().unwrap().open_id }
    fn syncing_txg_id(&self) -> u64 { self.state.lock().unwrap().syncing_id }
    fn is_sync_in_progress(&self) -> bool {
        self.state.lock().unwrap().total_syncs > 0
    }
    fn total_syncs(&self) -> u64 { self.state.lock().unwrap().total_syncs }
    fn total_dirty(&self) -> u64 { self.state.lock().unwrap().total_dirty }
    fn add_dirty_to_open(&mut self, _dummy: u64) {
        self.state.lock().unwrap().total_dirty += 1;
    }
    fn add_free_to_open(&mut self, _dummy: u64) {}
    fn add_io_to_open(&mut self, _dummy: u64) {}
}

/// bench: TXG trait dispatch throughput
pub fn txg_dispatch_bench(iters: u64) -> u128 {
    let mut txg: Box<dyn HostTxgManager> = Box::new(StandardHostTxg::new());
    txg.init(1);
    // 预热
    for i in 0..1000 {
        txg.add_dirty_to_open(i);
    }
    // bench: add_dirty + transition 旋转
    let start = Instant::now();
    let mut sink: u64 = 0;
    for r in 0..iters {
        if r & 0x7 == 0 {
            // transition (稀有)
            let id = txg.transition();
            sink = sink.wrapping_add(id);
        } else if r & 0x3 == 1 {
            // current_txg
            sink = sink.wrapping_add(txg.current_txg());
        } else {
            // add_dirty_to_open
            txg.add_dirty_to_open(r);
        }
    }
    let elapsed = start.elapsed().as_nanos();
    std::hint::black_box(sink);
    elapsed.saturating_mul(1_000) / iters as u128
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
    results.push(measure("socket_wait_queue", "net", 10_000, ||
        socket_wait_queue_bench(10_000)));
    results.push(measure("virtio_blk_io", "storage", 10_000, ||
        virtio_blk_io_bench(10_000)));
    // EBPF-3: eBPF verifier trait dispatch bench
    results.push(measure("bpf_verifier_dispatch", "ebpf", 100_000, ||
        bpf_verifier_dispatch_bench(100_000)));
    // SYSCTL-2: sysctl register/write bench
    results.push(measure("sysctl_rw", "config", 10_000, ||
        sysctl_bench(10_000)));
    // T-4.1: BlockDevice trait dispatch bench (LEGACY-4 验证)
    results.push(measure("blk_dev_dispatch", "block", 100_000, ||
        blk_dev_dispatch_bench(100_000)));
    // REVAL-6.1: VfsPollPolicy dispatch bench
    results.push(measure("vfs_poll_dispatch", "epoll", 100_000, ||
        vfs_poll_dispatch_bench(100_000)));
    // LEGACY-5.1: ZAP trait dispatch bench
    results.push(measure("zap_dispatch", "hvfs", 100_000, ||
        zap_dispatch_bench(100_000)));
    // LEGACY-5.2: TXG trait dispatch bench
    results.push(measure("txg_dispatch", "hvfs", 100_000, ||
        txg_dispatch_bench(100_000)));
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

    #[test]
    fn test_socket_wait_queue_mark_then_wake() {
        let q = MockSocketWaitQueue::new();
        // 首次 mark_waiting 返回 true (之前未标记)
        assert!(q.mark_waiting());
        // 重复 mark_waiting 返回 false (已经标记)
        assert!(!q.mark_waiting());
        assert!(q.is_pending());
        // try_wake 成功清掉 pending, 返回 true
        assert!(q.try_wake(0));
        assert!(!q.is_pending());
        // 没有等待者时 try_wake 返回 false
        assert!(!q.try_wake(0));
        assert_eq!(q.wake_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_socket_wait_queue_bench_runs() {
        // smoke: 调用一次小迭代 bench 确保不 panic
        let _ = socket_wait_queue_bench(10);
    }

    #[test]
    fn test_virtio_blk_submit_blk_write() {
        let mut vq = MockVirtQueue::new(32);
        // 提交 1 个 4K 写请求, 应返回 head = 0
        let head = vq.submit_blk_write().expect("free desc");
        assert_eq!(head, 0);
        // 描述符链 3 段已填充
        assert_eq!(vq.descs[0].len, 16);
        assert_eq!(vq.descs[1].len, BLK_4K_BYTES);
        assert_eq!(vq.descs[2].flags & VQ_DESC_F_WRITE, VQ_DESC_F_WRITE);
        assert_eq!(vq.descs[2].next, 0xFFFF);
        // avail_idx 已推进
        assert_eq!(vq.avail_idx, 1);
    }

    #[test]
    fn test_virtio_blk_submit_full_chain() {
        // 容量 32, 每次提交占用 3 段, 10 次后应仍有空间
        let mut vq = MockVirtQueue::new(32);
        for _ in 0..10 {
            assert!(vq.submit_blk_write().is_some());
        }
        assert_eq!(vq.avail_idx, 10);
    }

    #[test]
    fn test_virtio_blk_bench_runs() {
        // smoke: 调用一次小迭代 bench 确保不 panic
        let _ = virtio_blk_io_bench(10);
    }

    // ====== EBPF-3: BpfVerifier trait dispatch Mock + bench ======
    // 与 T4-3 framekernel 范式对齐: services 提供 verifier 实现, framework
    // 通过 `&dyn BpfVerifier` 动态分派. host-tests 用 mock 模拟 trait 契约.

    #[test]
    fn test_bpf_verifier_mock_trait_dispatch() {
        // 关键: 验证 `&dyn BpfVerifier` 动态分派机制可工作
        let v: &dyn BpfVerifier = &MockBpfVerifier::new(true);
        let prog = MockBpfProg::new(8);
        assert!(matches!(v.verify(&prog), VerifyResult::Ok));
    }

    #[test]
    fn test_bpf_verifier_reject() {
        // reject 模式: 验证拒绝路径
        let v: &dyn BpfVerifier = &MockBpfVerifier::new(false);
        let prog = MockBpfProg::new(8);
        assert!(matches!(v.verify(&prog), VerifyResult::Err(_)));
    }

    #[test]
    fn test_bpf_verifier_safety_default_no_verifier() {
        // 安全默认: 未注册 verifier 时, prog_load 拒绝所有
        // (framekernel 设计, 拒绝 = 安全默认)
        let subsys = MockBpfSubsystem::new();
        let result = subsys.prog_load(MockBpfProg::new(4));
        assert_eq!(result, -1); // EPERM: no verifier registered
    }

    #[test]
    fn test_bpf_verifier_set_then_load() {
        // 注册后 prog_load 走 verifier
        static VERIFIER: MockBpfVerifier = MockBpfVerifier::new(true);
        let subsys = MockBpfSubsystem::new();
        subsys.set_verifier(&VERIFIER);
        let result = subsys.prog_load(MockBpfProg::new(4));
        assert_eq!(result, 1); // fd = 1
    }

    #[test]
    fn test_bpf_verifier_bench_runs() {
        // smoke: 调用一次小迭代 bench
        let _ = bpf_verifier_dispatch_bench(100);
    }

    // ====== SYSCTL-2: MockSysctl 单元测试 ======

    #[test]
    fn test_mock_sysctl_register_and_read() {
        let t = MockSysctlTable::new();
        assert!(t.register("a", MockSysctlKind::Int, MockSysctlValue::Int(42)).is_ok());
        assert_eq!(t.read("a"), Some(MockSysctlValue::Int(42)));
    }

    #[test]
    fn test_mock_sysctl_write_type_mismatch() {
        let t = MockSysctlTable::new();
        assert!(t.register("a", MockSysctlKind::Int, MockSysctlValue::Int(0)).is_ok());
        // 写 Bool 到 Int 节点
        assert!(t.write("a", MockSysctlValue::Bool(true)).is_err());
    }

    #[test]
    fn test_mock_sysctl_read_not_found() {
        let t = MockSysctlTable::new();
        assert_eq!(t.read("nonexistent"), None);
    }

    #[test]
    fn test_mock_sysctl_bench_runs() {
        let _ = sysctl_bench(10);
    }

    // ====== T-4.1 (LEGACY-4): BlockDevice trait dispatch 单元测试 ======

    #[test]
    fn test_mock_blk_dev_read_write() {
        let dev = MockBlockDevice::new(16);
        let chitin = MockChitinDevice::new(Box::new(dev));
        let mut buf = [0u8; 512];
        // 读 sector 0
        assert_eq!(chitin.blk_read(0, &mut buf), 0);
        assert_eq!(buf[0], 0);
        // 写 sector 1
        let mut wbuf = [0xAB; 512];
        assert_eq!(chitin.blk_write(1, &wbuf), 0);
        // 再读 sector 1
        let mut rbuf = [0u8; 512];
        assert_eq!(chitin.blk_read(1, &mut rbuf), 0);
        assert_eq!(rbuf[0], 0xAB);
    }

    #[test]
    fn test_mock_blk_dev_oob() {
        let dev = MockBlockDevice::new(4);
        let chitin = MockChitinDevice::new(Box::new(dev));
        let mut buf = [0u8; 512];
        // 越界 sector 100 应返回 -EIO
        assert_eq!(chitin.blk_read(100, &mut buf), -5);
        assert_eq!(chitin.blk_write(100, &buf), -5);
    }

    #[test]
    fn test_mock_blk_dev_buf_too_small() {
        let dev = MockBlockDevice::new(4);
        let chitin = MockChitinDevice::new(Box::new(dev));
        let mut small = [0u8; 256];
        // buf.len() < 512 应返回 -1
        assert_eq!(chitin.blk_read(0, &mut small), -1);
    }

    #[test]
    fn test_mock_blk_dev_metadata() {
        let dev = MockBlockDevice::new(64);
        let chitin = MockChitinDevice::new(Box::new(dev));
        assert!(chitin.blk_is_present());
        assert_eq!(chitin.blk_total_sectors(), u64::MAX);
    }

    #[test]
    fn test_blk_dev_dispatch_bench_runs() {
        // smoke test
        let _ = blk_dev_dispatch_bench(100);
    }

    // ====== REVAL-6.1: VfsPollPolicy trait dispatch 单元测试 ======

    #[test]
    fn test_mock_vfs_poll_file_type() {
        let p = StandardHostVfsPollPolicy;
        use poll_events::*;
        assert_eq!(p.events_for_file_type(MockVfsFileType::File), EPOLLIN | EPOLLOUT);
        assert_eq!(p.events_for_file_type(MockVfsFileType::Dir), EPOLLIN);
        assert_eq!(p.events_for_file_type(MockVfsFileType::Dev), EPOLLHUP);
        assert_eq!(p.events_for_file_type(MockVfsFileType::Symlink), EPOLLIN | EPOLLHUP);
    }

    #[test]
    fn test_mock_vfs_poll_invalid_fd() {
        let p = StandardHostVfsPollPolicy;
        use poll_events::*;
        assert_eq!(p.events_for_invalid_fd(), EPOLLERR | EPOLLHUP);
    }

    #[test]
    fn test_mock_epoll_check_valid_file() {
        let check = MockEpollCheck::new(Box::new(StandardHostVfsPollPolicy));
        let ctx = MockVfsPollContext { valid: true, file_type: MockVfsFileType::File };
        // user 只关心 EPOLLIN
        assert_eq!(check.check(ctx, poll_events::EPOLLIN), poll_events::EPOLLIN);
        // user 只关心 EPOLLOUT
        assert_eq!(check.check(ctx, poll_events::EPOLLOUT), poll_events::EPOLLOUT);
        // user 关心 IN|OUT → 都报告
        assert_eq!(check.check(ctx, poll_events::EPOLLIN | poll_events::EPOLLOUT),
                   poll_events::EPOLLIN | poll_events::EPOLLOUT);
        // user 关心 ERR (File 不报告 ERR) → 0
        assert_eq!(check.check(ctx, poll_events::EPOLLERR), 0);
    }

    #[test]
    fn test_mock_epoll_check_invalid_fd() {
        let check = MockEpollCheck::new(Box::new(StandardHostVfsPollPolicy));
        let ctx = MockVfsPollContext { valid: false, file_type: MockVfsFileType::File };
        // invalid fd → ERR|HUP, 但 AND user mask
        assert_eq!(check.check(ctx, poll_events::EPOLLERR | poll_events::EPOLLHUP),
                   poll_events::EPOLLERR | poll_events::EPOLLHUP);
        assert_eq!(check.check(ctx, poll_events::EPOLLIN), 0);  // IN 不报告
    }

    #[test]
    fn test_mock_vfs_poll_bench_runs() {
        let _ = vfs_poll_dispatch_bench(100);
    }

    // ====== REVAL-6.2: epoll_pwake 拆分行为 单元测试 ======

    #[test]
    fn test_mock_instance_watches_fd() {
        let mut inst = MockEpollInstance::new();
        inst.add(MockEpollInterestItem { fd: 5, events: poll_events::EPOLLIN, data: 100 });
        // 包含 fd=5
        assert!(instance_watches_fd(&inst, 5));
        // 不包含 fd=6
        assert!(!instance_watches_fd(&inst, 6));
    }

    #[test]
    fn test_mock_enqueue_ready_basic() {
        let mut inst = MockEpollInstance::new();
        inst.add(MockEpollInterestItem { fd: 3, events: poll_events::EPOLLIN, data: 42 });
        let policy = StandardHostVfsPollPolicy;
        // 第一次入队
        assert!(enqueue_ready_for_fd(&mut inst, 3, &policy));
        assert_eq!(inst.ready_list.len(), 1);
        assert_eq!(inst.ready_list[0], (poll_events::EPOLLIN, 42));
    }

    #[test]
    fn test_mock_enqueue_ready_dedup() {
        let mut inst = MockEpollInstance::new();
        inst.add(MockEpollInterestItem { fd: 3, events: poll_events::EPOLLIN, data: 42 });
        let policy = StandardHostVfsPollPolicy;
        // 第一次入队
        assert!(enqueue_ready_for_fd(&mut inst, 3, &policy));
        // 第二次入队 → dedup, 失败
        assert!(!enqueue_ready_for_fd(&mut inst, 3, &policy));
        assert_eq!(inst.ready_list.len(), 1);
    }

    #[test]
    fn test_mock_enqueue_ready_no_fd() {
        let mut inst = MockEpollInstance::new();
        inst.add(MockEpollInterestItem { fd: 3, events: poll_events::EPOLLIN, data: 42 });
        let policy = StandardHostVfsPollPolicy;
        // fd=99 不在列表
        assert!(!enqueue_ready_for_fd(&mut inst, 99, &policy));
        assert_eq!(inst.ready_list.len(), 0);
    }

    #[test]
    fn test_mock_pwake_multiple_instances() {
        let mut p = MockEpollPwake::new();
        // 3 个实例, 只有 2 个监控 fd=5
        for _ in 0..2 {
            let mut inst = MockEpollInstance::new();
            inst.add(MockEpollInterestItem { fd: 5, events: poll_events::EPOLLIN, data: 100 });
            p.instances.push(inst);
        }
        p.instances.push(MockEpollInstance::new());  // 第 3 个不监控

        let policy = StandardHostVfsPollPolicy;
        let count = p.pwake(5, &policy);
        // 2 个实例成功入队
        assert_eq!(count, 2);
        // 第 3 个实例 ready_list 仍空
        assert_eq!(p.instances[2].ready_list.len(), 0);
    }

    #[test]
    fn test_mock_pwake_dedup_across_calls() {
        let mut p = MockEpollPwake::new();
        let mut inst = MockEpollInstance::new();
        inst.add(MockEpollInterestItem { fd: 5, events: poll_events::EPOLLIN, data: 100 });
        p.instances.push(inst);

        let policy = StandardHostVfsPollPolicy;
        // 第 1 次: 入队
        assert_eq!(p.pwake(5, &policy), 1);
        // 第 2 次: dedup, 不入队
        assert_eq!(p.pwake(5, &policy), 0);
        // ready_list 仍只 1 项
        assert_eq!(p.instances[0].ready_list.len(), 1);
    }

    #[test]
    fn test_mock_pwake_no_match() {
        let mut p = MockEpollPwake::new();
        let mut inst = MockEpollInstance::new();
        inst.add(MockEpollInterestItem { fd: 5, events: poll_events::EPOLLIN, data: 100 });
        p.instances.push(inst);

        let policy = StandardHostVfsPollPolicy;
        // fd=99 不在列表
        assert_eq!(p.pwake(99, &policy), 0);
    }

    // ====== LEGACY-5.1: ZapStore trait 单元测试 ======

    #[test]
    fn test_zap_insert_lookup() {
        let zap: Box<dyn HostZapStore> = Box::new(StandardHostZap::new());
        assert!(zap.insert("a", b"1"));
        assert_eq!(zap.lookup("a"), Some(b"1".to_vec()));
        assert_eq!(zap.lookup("nokey"), None);
    }

    #[test]
    fn test_zap_update() {
        let zap: Box<dyn HostZapStore> = Box::new(StandardHostZap::new());
        assert!(zap.insert("k", b"v1"));
        assert!(zap.insert("k", b"v2"));
        assert_eq!(zap.lookup("k"), Some(b"v2".to_vec()));
    }

    #[test]
    fn test_zap_capacity_limit() {
        let zap: Box<dyn HostZapStore> = Box::new(StandardHostZap::with_capacity(2));
        assert!(zap.insert("a", b"1"));
        assert!(zap.insert("b", b"2"));
        // 容量满 + 新键 → false
        assert!(!zap.insert("c", b"3"));
        // 容量满 + 旧键 (更新) → true
        assert!(zap.insert("a", b"x"));
        assert_eq!(zap.len(), 2);
    }

    #[test]
    fn test_zap_u64() {
        let zap: Box<dyn HostZapStore> = Box::new(StandardHostZap::new());
        assert!(zap.insert_u64("count", 42));
        assert_eq!(zap.lookup_u64("count"), Some(42));
    }

    #[test]
    fn test_zap_remove() {
        let zap: Box<dyn HostZapStore> = Box::new(StandardHostZap::new());
        zap.insert("a", b"1");
        assert!(zap.contains("a"));
        assert!(zap.remove("a"));
        assert!(!zap.contains("a"));
        assert!(!zap.remove("a"));
    }

    #[test]
    fn test_zap_bench_runs() {
        let _ = zap_dispatch_bench(100);
    }

    // ====== LEGACY-5.2: TxgManager trait 单元测试 ======

    #[test]
    fn test_txg_init() {
        let mut txg: Box<dyn HostTxgManager> = Box::new(StandardHostTxg::new());
        txg.init(1);
        assert_eq!(txg.current_txg(), 1);
        assert_eq!(txg.open_txg_id(), 1);
        assert_eq!(txg.syncing_txg_id(), 3);
        assert_eq!(txg.total_syncs(), 0);
    }

    #[test]
    fn test_txg_transition() {
        let mut txg: Box<dyn HostTxgManager> = Box::new(StandardHostTxg::new());
        txg.init(1);
        let old = txg.current_txg();
        let new = txg.transition();
        assert!(new > old);
        assert_eq!(txg.total_syncs(), 1);
        assert!(txg.is_sync_in_progress());
    }

    #[test]
    fn test_txg_dirty_accumulate() {
        let mut txg: Box<dyn HostTxgManager> = Box::new(StandardHostTxg::new());
        txg.init(1);
        for _ in 0..5 {
            txg.add_dirty_to_open(0);
        }
        assert_eq!(txg.total_dirty(), 5);
    }

    #[test]
    fn test_txg_bench_runs() {
        let _ = txg_dispatch_bench(100);
    }
}
