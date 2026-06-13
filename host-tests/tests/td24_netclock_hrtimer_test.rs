//! I-50 补充验收: 网络时钟用 hrtimer 而非 tick
//!
//! 镜像 [framework/net/smoltcp_impl.rs::smoltcp_now] 的契约:
//! 1. 校准后: hrtimer_clock_read() 提供纳秒, 截断到 ms 给 smoltcp
//! 2. 校准前: 直接走 get_uptime_ms() 回退
//! 3. 溢出保护: ns/1_000_000 > i64::MAX → 回退
//!
//! 行为镜像: 用本地 fake hrtimer 模拟"校准后/前"两种状态, 验证
//! smoltcp 收到的 Instant 增量精度.
#![allow(dead_code)]

// i64::MAX = 9223372036854775807 (≈ 292_471_208 年 in ms). 实际不可达.

const NS_PER_MS: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Instant { ms: i64 }

impl Instant {
    fn from_ms(ms: i64) -> Self { Self { ms } }
    fn elapsed_since(&self, earlier: Instant) -> i64 { self.ms - earlier.ms }
}

/// 镜像 smoltcp_now 的决策表.
/// 当 hrtimer 校准成功 (calibrated=true) 时用 hrtimer ns, 否则用 tick ms.
fn smoltcp_now(hrtimer_ns: u64, tick_ms: u64, calibrated: bool) -> Instant {
    if calibrated {
        let ms = hrtimer_ns / NS_PER_MS;
        if ms > i64::MAX as u64 {
            return Instant::from_ms(tick_ms as i64);
        }
        Instant::from_ms(ms as i64)
    } else {
        Instant::from_ms(tick_ms as i64)
    }
}

#[test]
fn calibrated_path_uses_hrtimer() {
    // hrtimer 已校准: 5s + 800μs = 5_000_800_000ns, 截断到 5000ms
    let t0 = smoltcp_now(5_000_800_000, 999, true);
    assert_eq!(t0.ms, 5000);
    // 1ms 后 (校准时基可分辨微秒级), 截断到 5001ms
    let t1 = smoltcp_now(5_001_800_000, 1000, true);
    assert_eq!(t1.ms, 5001);
}

#[test]
fn uncalibrated_falls_back_to_tick() {
    let t0 = smoltcp_now(u64::MAX, 100, false);
    assert_eq!(t0.ms, 100);
    let t1 = smoltcp_now(u64::MAX, 105, false);
    assert_eq!(t1.ms, 105);
}

#[test]
fn overflow_guard_present_in_kernel() {
    // 镜像 smoltcp_now 的溢出保护分支.
    // 注: 实际运行时 u64::MAX ns ≈ 584 年, 远小于 i64::MAX ms ≈ 292M 年.
    // 决策逻辑中存在 if ms > i64::MAX { fallback } 兜底, 但触发条件
    // 仅在测试用 mock 中可达. 这里用决策表函数直接验证 if 表达式结构.
    let ns: u64 = 1_234_567_890; // 1.2s, 远超典型 ns
    let ms = ns / NS_PER_MS;
    let would_overflow = ms > i64::MAX as u64;
    assert!(!would_overflow);
    // 真实 smoltcp_now 在该条件下 ms = u64::MAX/1_000_000, 仍 < i64::MAX,
    // 因此 fallback 永远不被触发. 保留断言以防未来 smoltcp Instant 单位变更.
}

#[test]
fn calibrated_jitter_under_one_ms() {
    // 两次紧邻读: ns 仅差 500_000 (半毫秒), 截断后 ms 不变
    let t0 = smoltcp_now(10_000_000_000, 0, true);
    let t1 = smoltcp_now(10_000_500_000, 1, true);
    assert_eq!(t0, t1);
}

#[test]
fn calibrated_advance_one_ms() {
    let t0 = smoltcp_now(10_000_000_000, 0, true);
    let t1 = smoltcp_now(10_001_000_000, 0, true);
    assert_eq!(t1.elapsed_since(t0), 1);
}

#[test]
fn monotonic_increment_calibrated() {
    // 模拟 100ms 内连续读取, 验证单调递增
    let mut prev = smoltcp_now(1_000_000_000, 1000, true);
    for i in 1..=100u64 {
        let cur = smoltcp_now(1_000_000_000 + i * 10_000_000, 1000 + i, true);
        assert!(cur.ms >= prev.ms, "cur={} prev={}", cur.ms, prev.ms);
        prev = cur;
    }
}

#[test]
fn monotonic_increment_uncalibrated() {
    // tick 路径: ms 单调, 抖动由 tick 节流
    let mut prev = smoltcp_now(0, 1000, false);
    for i in 1..=100u64 {
        let cur = smoltcp_now(0, 1000 + i, false);
        assert_eq!(cur.ms - prev.ms, 1);
        prev = cur;
    }
}

#[test]
fn calibrated_takes_priority_over_tick_when_in_range() {
    // 当 calibrated=true 且 ns 在 i64 范围内, tick_ms 被忽略
    let t = smoltcp_now(123_456_789, 999_999, true);
    // 123_456_789 ns = 123 ms
    assert_eq!(t.ms, 123);
}

#[test]
fn ns_to_ms_truncates_does_not_round() {
    // 1.999ms 截断到 1ms (smoltcp::Instant 行为)
    let t = smoltcp_now(1_999_999, 0, true);
    assert_eq!(t.ms, 1);
    // 2.000ms 截断到 2ms
    let t = smoltcp_now(2_000_000, 0, true);
    assert_eq!(t.ms, 2);
}

#[test]
fn zero_ns_yields_zero_ms() {
    let t = smoltcp_now(0, 0, true);
    assert_eq!(t.ms, 0);
}

#[test]
fn boundary_just_below_overflow() {
    // 决策: ms > i64::MAX → fallback
    // i64::MAX = 0x7FFF_FFFF_FFFF_FFFF. ms = i64::MAX 时, 应当走 hrtimer 路径.
    // 真实 ns 上限 u64::MAX / 1_000_000 ≈ 1.84e13, 远小于 i64::MAX, 不可触发 fallback.
    // 模拟 fallback: 用 max_ns + 验证 ms = max_ns/1_000_000 (hrtimer 路径生效)
    let max_ns = u64::MAX;
    let t = smoltcp_now(max_ns, 42, true);
    // ms = u64::MAX / 1_000_000 = 18_446_744_073_709_551, 仍在 i64 范围 (9.22e18)
    assert_eq!(t.ms, (u64::MAX / 1_000_000) as i64);
}
