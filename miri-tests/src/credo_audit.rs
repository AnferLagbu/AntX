//! Credo / PWID 审计日志 (services 层算法等价)
//!
//! 镜像 `src/kernel/services/credo/audit.rs` 的算法, 在 Miri 下验证 UB.
//!
//! ## 关键不变量
//!
//! 1. **追加语义**: 任何 `append` 后, next_index 单调递增
//! 2. **哈希链**: node[N].prev_hash == node[N-1].hash
//! 3. **可验证**: 修改任意字节必使 verify 返回 ok=false
//! 4. **不可重用**: 槽位用尽后写入覆盖旧位置 (count 到 dropped)

#![allow(dead_code)]

pub const AUDIT_BUFFER_SIZE: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditEventKind {
    LoginSuccess,
    LoginFailed,
    Logout,
    CapabilityGranted,
    CapabilityRevoked,
    SessionKilled,
    PolicyViolation,
    BarrierRecovery,
    BarrierReset,
    Custom(u8),
}

impl AuditEventKind {
    pub const fn tag(self) -> u8 {
        match self {
            Self::LoginSuccess      => 0x01,
            Self::LoginFailed       => 0x02,
            Self::Logout            => 0x03,
            Self::CapabilityGranted => 0x04,
            Self::CapabilityRevoked => 0x05,
            Self::SessionKilled     => 0x06,
            Self::PolicyViolation   => 0x07,
            Self::BarrierRecovery   => 0x08,
            Self::BarrierReset      => 0x09,
            Self::Custom(v)         => v,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditEvent {
    pub kind: AuditEventKind,
    pub tick: u64,
    pub pwm: u64,
    pub session_id: u32,
    pub domain_id: u8,
    pub bits: u64,
    pub result: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashChainNode {
    pub index: u32,
    pub event: AuditEvent,
    pub prev_hash: u64,
    pub hash: u64,
}

#[derive(Debug)]
pub struct AuditLog {
    pub buffer: [Option<HashChainNode>; AUDIT_BUFFER_SIZE],
    pub next_index: u32,
    pub write_pos: usize,
    pub dropped: u64,
    pub last_hash: u64,
    pub total: u64,
}

impl Default for AuditLog {
    fn default() -> Self { Self::new() }
}

impl AuditLog {
    pub const fn new() -> Self {
        Self {
            buffer: [None; AUDIT_BUFFER_SIZE],
            next_index: 0,
            write_pos: 0,
            dropped: 0,
            last_hash: 0,
            total: 0,
        }
    }

    /// 追加事件 (返回新 index)
    pub fn append(&mut self, event: AuditEvent) -> u32 {
        let idx = self.next_index;
        self.next_index = self.next_index.wrapping_add(1);
        if self.next_index == 0 { self.next_index = 1; } // 跳过 0 永不重用

        let pos = self.write_pos;
        // 计算哈希
        let prev = self.last_hash;
        let hash = compute_hash(prev, &event);
        let node = HashChainNode {
            index: idx,
            event,
            prev_hash: prev,
            hash,
        };
        // 覆盖检测: write_pos 位置有数据且 index 不同 → 覆盖
        if let Some(existing) = &self.buffer[pos] {
            if existing.index != idx {
                self.dropped += 1;
            }
        }
        self.buffer[pos] = Some(node);
        self.write_pos = (pos + 1) % AUDIT_BUFFER_SIZE;
        self.last_hash = hash;
        self.total += 1;
        idx
    }

    pub fn get(&self, index: u32) -> Option<&HashChainNode> {
        if index >= self.next_index {
            return None;
        }
        self.buffer.iter().flatten().find(|n| n.index == index)
    }

    /// 验证哈希链
    pub fn verify(&self) -> (bool, Option<u32>) {
        // 收集已写入的节点 (按 index 排序)
        let mut nodes: [Option<HashChainNode>; AUDIT_BUFFER_SIZE] = [None; AUDIT_BUFFER_SIZE];
        let mut max_idx: u32 = 0;
        for n in self.buffer.iter().flatten() {
            if (n.index as usize) < AUDIT_BUFFER_SIZE {
                nodes[n.index as usize] = Some(*n);
                if n.index > max_idx { max_idx = n.index; }
            }
        }
        let mut prev: u64 = 0;
        for (i, slot) in nodes.iter().enumerate().take(max_idx as usize + 1) {
            if i >= AUDIT_BUFFER_SIZE { break; }
            if let Some(n) = slot {
                if n.prev_hash != prev {
                    return (false, Some(n.index));
                }
                let expected = compute_hash(prev, &n.event);
                if expected != n.hash {
                    return (false, Some(n.index));
                }
                prev = n.hash;
            }
            // 跳过的 index 视为已丢弃, 继续验证
        }
        (true, None)
    }

    pub fn total(&self) -> u64 { self.total }
    pub fn dropped(&self) -> u64 { self.dropped }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditError {
    /// 未使用 (保留向后兼容)
    _Reserved,
}

/// FNV-1a 64 哈希
fn compute_hash(prev: u64, event: &AuditEvent) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    let mut h = prev ^ OFFSET;
    h = (h ^ event.tick).wrapping_mul(PRIME);
    h = (h ^ event.pwm).wrapping_mul(PRIME);
    h = (h ^ (event.session_id as u64)).wrapping_mul(PRIME);
    h = (h ^ (event.domain_id as u64)).wrapping_mul(PRIME);
    h = (h ^ event.bits).wrapping_mul(PRIME);
    h = (h ^ (event.result as u64)).wrapping_mul(PRIME);
    h = (h ^ (event.kind.tag() as u64)).wrapping_mul(PRIME);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login_ok(tick: u64, pwm: u64) -> AuditEvent {
        AuditEvent {
            kind: AuditEventKind::LoginSuccess,
            tick, pwm, session_id: 0, domain_id: 0, bits: 0, result: 0,
        }
    }

    #[test]
    fn hash_deterministic() {
        let e = login_ok(100, 1);
        assert_eq!(compute_hash(0, &e), compute_hash(0, &e));
    }

    #[test]
    fn hash_sensitive_to_prev() {
        let e = login_ok(100, 1);
        assert_ne!(compute_hash(0, &e), compute_hash(12345, &e));
    }

    #[test]
    fn hash_sensitive_to_event() {
        let e1 = login_ok(100, 1);
        let e2 = AuditEvent { kind: AuditEventKind::LoginFailed, ..e1 };
        assert_ne!(compute_hash(0, &e1), compute_hash(0, &e2));
    }

    #[test]
    fn hash_sensitive_to_kind_tag() {
        let e1 = login_ok(100, 1);
        let e2 = AuditEvent { kind: AuditEventKind::Logout, ..e1 };
        assert_ne!(compute_hash(0, &e1), compute_hash(0, &e2));
    }

    #[test]
    fn append_and_get() {
        let mut log = AuditLog::new();
        let idx = log.append(login_ok(100, 1));
        assert_eq!(idx, 0);
        let got = log.get(idx).unwrap();
        assert_eq!(got.event, login_ok(100, 1));
        assert_eq!(got.prev_hash, 0);
        assert_ne!(got.hash, 0);
    }

    #[test]
    fn chain_links_correctly() {
        let mut log = AuditLog::new();
        log.append(login_ok(100, 1));
        let h0 = log.last_hash;
        log.append(login_ok(101, 2));
        // 第 1 个节点.prev_hash == 第 0 个节点.hash
        let n0 = log.get(0).unwrap();
        let n1 = log.get(1).unwrap();
        assert_eq!(n1.prev_hash, n0.hash);
        assert_eq!(n0.hash, h0);
    }

    #[test]
    fn verify_ok() {
        let mut log = AuditLog::new();
        for i in 0..20 {
            log.append(login_ok(i, i));
        }
        let (ok, bad) = log.verify();
        assert!(ok, "verify failed at {:?}", bad);
    }

    #[test]
    fn verify_detects_event_tamper() {
        let mut log = AuditLog::new();
        for i in 0..5 {
            log.append(login_ok(i, i));
        }
        // 篡改: 修改 buffer[2] 的 event.bits
        if let Some(ref mut n) = log.buffer[2] {
            n.event.bits = 0xDEADBEEF;
        }
        let (ok, _) = log.verify();
        assert!(!ok, "tamper should be detected");
    }

    #[test]
    fn verify_detects_hash_tamper() {
        let mut log = AuditLog::new();
        for i in 0..5 {
            log.append(login_ok(i, i));
        }
        // 篡改: 修改 buffer[1] 的 hash
        if let Some(ref mut n) = log.buffer[1] {
            n.hash ^= 0x1;
        }
        let (ok, _) = log.verify();
        assert!(!ok);
    }

    #[test]
    fn verify_detects_prev_hash_break() {
        let mut log = AuditLog::new();
        for i in 0..5 {
            log.append(login_ok(i, i));
        }
        // 篡改: 修改 buffer[2] 的 prev_hash
        if let Some(ref mut n) = log.buffer[2] {
            n.prev_hash ^= 0x1;
        }
        let (ok, _) = log.verify();
        assert!(!ok);
    }

    #[test]
    fn total_increments() {
        let mut log = AuditLog::new();
        assert_eq!(log.total(), 0);
        log.append(login_ok(1, 1));
        log.append(login_ok(2, 2));
        assert_eq!(log.total(), 2);
    }

    #[test]
    fn dropped_when_overwrite() {
        let mut log = AuditLog::new();
        // 写入超过 BUFFER_SIZE 个, 触发覆盖
        for i in 0..(AUDIT_BUFFER_SIZE as u64 + 10) {
            log.append(login_ok(i, i));
        }
        assert!(log.dropped() > 0);
        assert_eq!(log.total() as usize, AUDIT_BUFFER_SIZE + 10);
    }

    /// 顺序单调性: next_index 单调递增
    #[test]
    fn next_index_monotonic() {
        let mut log = AuditLog::new();
        let mut last = 0;
        for _ in 0..100 {
            let idx = log.append(login_ok(1, 1));
            assert!(idx >= last);
            last = idx;
        }
    }
}
