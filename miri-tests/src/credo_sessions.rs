//! Credo / PWID 会话生命周期 (services 层算法等价)
//!
//! 镜像 `src/kernel/services/credo/sessions.rs` 的算法, 在 Miri 下验证 UB.
//!
//! ## 关键不变量
//!
//! 1. **槽位唯一**: 同一 SessionId 仅一个活跃 slot
//! 2. **PWM 限额**: 同一 PWM 最多 8 个并发会话
//! 3. **过期回收**: gc_expired 在 current_tick >= expires_tick 时清空
//! 4. **原子计数**: active_count 与 len 始终一致

#![allow(dead_code)]

use crate::credo_policy::{CapBits, CapDomain, CapMatrix, InMemoryMatrix, PolicyEngine};

pub const MAX_SESSIONS: usize = 64;
pub const MAX_PWM_CONCURRENT: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Active,
    Expired,
    LoggedOut,
    Killed,
    Quarantined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Session {
    pub id: SessionId,
    pub pwm: u64,
    pub created_tick: u64,
    pub last_active_tick: u64,
    pub expires_tick: u64,
    pub caps: [CapBits; 16],
    pub state: SessionState,
    pub pid: u32,
}

#[derive(Debug)]
pub struct SessionTable {
    pub records: [Option<Session>; MAX_SESSIONS],
    pub next_id: u32,
    pub active: u64,
    pub next_slot: usize,
}

impl Default for SessionTable {
    fn default() -> Self { Self::new() }
}

impl SessionTable {
    pub const fn new() -> Self {
        Self {
            records: [None; MAX_SESSIONS],
            next_id: 1,
            active: 0,
            next_slot: 0,
        }
    }

    pub fn create(
        &mut self,
        pwm: u64,
        caps: [CapBits; 16],
        current_tick: u64,
        expires_tick: u64,
        pid: u32,
    ) -> Result<SessionId, SessionError> {
        // PWM 限额: filter().count() 一次遍历, 显式 >= MAX_PWM_CONCURRENT 检测
        if self.records.iter().flatten()
            .filter(|s| s.pwm == pwm && matches!(s.state, SessionState::Active))
            .count() as u32 >= MAX_PWM_CONCURRENT
        {
            return Err(SessionError::TooManySessions);
        }
        // 查找空槽 (从 next_slot 起, 环形扫描)
        let idx = self.next_empty_slot(self.next_slot).ok_or(SessionError::TableFull)?;
        let id = SessionId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        if self.next_id == 0 { self.next_id = 1; }
        self.records[idx] = Some(Session {
            id,
            pwm,
            created_tick: current_tick,
            last_active_tick: current_tick,
            expires_tick,
            caps,
            state: SessionState::Active,
            pid,
        });
        self.next_slot = (idx + 1) % MAX_SESSIONS;
        self.active += 1;
        Ok(id)
    }

    /// 环形扫描 next_empty_slot, 复用避免重复逻辑
    pub fn next_empty_slot(&self, start: usize) -> Option<usize> {
        (0..MAX_SESSIONS)
            .map(|i| (start + i) % MAX_SESSIONS)
            .find(|&i| self.records[i].is_none())
    }

    pub fn get(&self, id: SessionId) -> Option<&Session> {
        self.records.iter().flatten().find(|s| s.id == id)
    }

    pub fn find_slot(&self, id: SessionId) -> Option<usize> {
        self.records.iter().position(|s| s.is_some_and(|s| s.id == id))
    }

    pub fn heartbeat(&mut self, id: SessionId, current_tick: u64) -> Result<(), SessionError> {
        match self.find_slot(id) {
            Some(i) => {
                let s = self.records[i].as_mut().expect("find_slot guarantees Some");
                if !matches!(s.state, SessionState::Active) {
                    return Err(SessionError::NotActive);
                }
                s.last_active_tick = current_tick;
                Ok(())
            }
            None => Err(SessionError::NotFound),
        }
    }

    pub fn end(&mut self, id: SessionId, target: SessionState) -> Result<Session, SessionError> {
        let i = self.find_slot(id).ok_or(SessionError::NotFound)?;
        let prev = self.records[i].take().expect("find_slot guarantees Some");
        self.active -= 1;
        let _ = target; // 仅保留用于审计
        Ok(prev)
    }

    pub fn end_all_for(&mut self, pwm: u64) -> usize {
        // 先用 take 释放, 再统计 (避免借用冲突)
        let mut count = 0;
        for slot in &mut self.records {
            if slot.as_ref().is_some_and(|s| s.pwm == pwm) {
                *slot = None;
                count += 1;
            }
        }
        if count > 0 {
            self.active -= count as u64;
        }
        count
    }

    pub fn gc_expired(&mut self, current_tick: u64) -> usize {
        let mut count = 0;
        for slot in &mut self.records {
            if slot.as_ref().is_some_and(|s| s.expires_tick != 0 && current_tick >= s.expires_tick) {
                *slot = None;
                count += 1;
            }
        }
        if count > 0 {
            self.active -= count as u64;
        }
        count
    }

    pub fn active_count(&self) -> u64 { self.active }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    TableFull,
    NotFound,
    NotActive,
    TooManySessions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginResult {
    Success(SessionId),
    Denied(LoginDeny),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginDeny {
    InvalidCredentials,
    CapabilityDenied,
    TooManySessions,
}

#[derive(Debug)]
pub struct SessionManager<'a> {
    pub table: &'a mut SessionTable,
    pub policy: &'a PolicyEngine,
}

impl<'a> SessionManager<'a> {
    pub fn new(table: &'a mut SessionTable, policy: &'a PolicyEngine) -> Self {
        Self { table, policy }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn login(
        &mut self,
        pwm: u64,
        password_ok: bool,
        required: CapBits,
        domain: CapDomain,
        current_tick: u64,
        expires_tick: u64,
        pid: u32,
        caps: [CapBits; 16],
    ) -> LoginResult {
        if !password_ok {
            return LoginResult::Denied(LoginDeny::InvalidCredentials);
        }
        let mut matrix_inner = CapMatrix::empty();
        // fill rows with zip
        for (row, &bits) in matrix_inner.rows.iter_mut().zip(caps.iter()) {
            *row = bits;
        }
        let matrix = InMemoryMatrix(matrix_inner);
        if !self.policy.check(&matrix, domain, required) {
            return LoginResult::Denied(LoginDeny::CapabilityDenied);
        }
        match self.table.create(pwm, caps, current_tick, expires_tick, pid) {
            Ok(id) => LoginResult::Success(id),
            Err(SessionError::TooManySessions | SessionError::TableFull) => {
                LoginResult::Denied(LoginDeny::TooManySessions)
            }
            Err(_) => LoginResult::Denied(LoginDeny::InvalidCredentials),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_caps() -> [CapBits; 16] {
        let mut c = [CapBits(0); 16];
        c[CapDomain::FS.0 as usize] = CapBits(0xFF);
        c
    }

    #[test]
    fn create_and_get() {
        let mut t = SessionTable::new();
        let id = t.create(1, default_caps(), 100, 0, 0).unwrap();
        let s = t.get(id).unwrap();
        assert_eq!(s.pwm, 1);
        assert_eq!(s.state, SessionState::Active);
        assert_eq!(t.active_count(), 1);
    }

    #[test]
    fn end_releases() {
        let mut t = SessionTable::new();
        let id = t.create(1, default_caps(), 100, 0, 0).unwrap();
        let prev = t.end(id, SessionState::LoggedOut).unwrap();
        assert_eq!(prev.id, id);
        assert_eq!(t.active_count(), 0);
        assert!(t.get(id).is_none());
    }

    #[test]
    fn heartbeat() {
        let mut t = SessionTable::new();
        let id = t.create(1, default_caps(), 100, 200, 0).unwrap();
        t.heartbeat(id, 150).unwrap();
        let s = t.get(id).unwrap();
        assert_eq!(s.last_active_tick, 150);
    }

    #[test]
    fn gc_expired() {
        let mut t = SessionTable::new();
        let id1 = t.create(1, default_caps(), 100, 200, 0).unwrap();
        let _id2 = t.create(2, default_caps(), 100, 500, 0).unwrap();
        let n = t.gc_expired(300);
        assert_eq!(n, 1);
        assert_eq!(t.active_count(), 1);
        assert!(t.get(id1).is_none());
    }

    #[test]
    fn end_all_for() {
        let mut t = SessionTable::new();
        for _ in 0..3 {
            t.create(1, default_caps(), 100, 0, 0).unwrap();
        }
        t.create(2, default_caps(), 100, 0, 0).unwrap();
        assert_eq!(t.active_count(), 4);
        let n = t.end_all_for(1);
        assert_eq!(n, 3);
        assert_eq!(t.active_count(), 1);
    }

    #[test]
    fn too_many_per_pwm() {
        let mut t = SessionTable::new();
        for _ in 0..MAX_PWM_CONCURRENT {
            t.create(1, default_caps(), 100, 0, 0).unwrap();
        }
        let r = t.create(1, default_caps(), 100, 0, 0);
        assert_eq!(r, Err(SessionError::TooManySessions));
    }

    #[test]
    fn table_full() {
        let mut t = SessionTable::new();
        for i in 0..MAX_SESSIONS {
            t.create(i as u64, default_caps(), 100, 0, 0).unwrap();
        }
        let r = t.create(999, default_caps(), 100, 0, 0);
        assert_eq!(r, Err(SessionError::TableFull));
    }

    #[test]
    fn login_invalid_creds() {
        let mut t = SessionTable::new();
        let p = PolicyEngine::new();
        let mut mgr = SessionManager::new(&mut t, &p);
        let r = mgr.login(1, false, CapBits(0), CapDomain::FS, 100, 0, 0, default_caps());
        assert_eq!(r, LoginResult::Denied(LoginDeny::InvalidCredentials));
    }

    #[test]
    fn login_capability_denied() {
        let mut t = SessionTable::new();
        let p = PolicyEngine::new();
        let mut mgr = SessionManager::new(&mut t, &p);
        let mut caps = default_caps();
        caps[CapDomain::FS.0 as usize] = CapBits(0); // 不给 FS 任何能力
        let r = mgr.login(1, true, CapBits(0b0001), CapDomain::FS, 100, 0, 0, caps);
        assert_eq!(r, LoginResult::Denied(LoginDeny::CapabilityDenied));
    }

    #[test]
    fn login_success() {
        let mut t = SessionTable::new();
        let p = PolicyEngine::new();
        let mut mgr = SessionManager::new(&mut t, &p);
        let r = mgr.login(1, true, CapBits(0b1000), CapDomain::FS, 100, 0, 0, default_caps());
        assert!(matches!(r, LoginResult::Success(_)));
        assert_eq!(t.active_count(), 1);
    }

    /// 完整生命周期: login → use → logout
    #[test]
    fn lifecycle_login_use_logout() {
        let mut t = SessionTable::new();
        let p = PolicyEngine::new();
        let mut mgr = SessionManager::new(&mut t, &p);
        let r = mgr.login(1, true, CapBits(0b0001), CapDomain::FS, 100, 200, 42, default_caps());
        let id = match r {
            LoginResult::Success(id) => id,
            _ => panic!("expected Success"),
        };
        let s = t.get(id).unwrap();
        assert_eq!(s.pid, 42);
        // 心跳
        t.heartbeat(id, 150).unwrap();
        // 登出
        let _prev = t.end(id, SessionState::LoggedOut).unwrap();
        assert_eq!(t.active_count(), 0);
    }

    /// 过期会话被 GC
    #[test]
    fn session_expired_via_gc() {
        let mut t = SessionTable::new();
        let id = t.create(1, default_caps(), 100, 200, 0).unwrap();
        assert_eq!(t.active_count(), 1);
        // tick=200 时, gc 应清理
        let n = t.gc_expired(200);
        assert_eq!(n, 1);
        assert!(t.get(id).is_none());
    }
}
