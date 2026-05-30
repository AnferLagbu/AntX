use core::sync::atomic::{AtomicU32, Ordering};

use super::types::*;
use super::domain::RecoveryDomain;

pub static ROLLBACK_LOG: spin::Mutex<[Option<RollbackEvent>; MAX_ROLLBACK_LOG]> =
    spin::Mutex::new([None; MAX_ROLLBACK_LOG]);
static ROLLBACK_LOG_IDX: AtomicU32 = AtomicU32::new(0);

fn log_rollback_event(event: RollbackEvent) {
    let mut log = ROLLBACK_LOG.lock();
    let idx = ROLLBACK_LOG_IDX.fetch_add(1, Ordering::SeqCst) as usize;
    log[idx % MAX_ROLLBACK_LOG] = Some(event);

    crate::klog_ffi!(klog_ffi_error,
        "[BARRIER] Rollback: dom={} gen={}->{} entries={} fp=0x{:X} depth={} result={}",
        event.domain_id, event.generation_from, event.generation_to,
        event.entries_rolled_back, event.crash_fingerprint,
        event.cascade_depth, event.result
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanicDomainMapping {
    pub prefix: &'static [u8],
    pub domain_id: u64,
}

const PANIC_DOMAIN_MAP: [PanicDomainMapping; 6] = [
    PanicDomainMapping { prefix: b"PMM",   domain_id: 3 },
    PanicDomainMapping { prefix: b"PROC",  domain_id: 4 },
    PanicDomainMapping { prefix: b"NET",   domain_id: 5 },
    PanicDomainMapping { prefix: b"VFS",   domain_id: 2 },
    PanicDomainMapping { prefix: b"HvFS",  domain_id: 2 },
    PanicDomainMapping { prefix: b"RAMFS", domain_id: 2 },
];

pub struct RecoveryManager {
    pub domains: [Option<&'static RecoveryDomain>; MAX_RECOVERY_DOMAINS],
    pub direct_map: [Option<&'static RecoveryDomain>; DIRECT_MAP_SIZE],
    pub count: AtomicU32,
}

impl RecoveryManager {
    pub const fn new() -> Self {
        const NONE: Option<&'static RecoveryDomain> = None;
        Self {
            domains: [NONE; MAX_RECOVERY_DOMAINS],
            direct_map: [NONE; DIRECT_MAP_SIZE],
            count: AtomicU32::new(0),
        }
    }

    pub fn register(&mut self, domain: &'static RecoveryDomain) -> Option<u64> {
        let idx = self.count.load(Ordering::SeqCst) as usize;
        if idx >= MAX_RECOVERY_DOMAINS { return None; }
        let id = domain.id as usize;
        if id < DIRECT_MAP_SIZE {
            self.direct_map[id] = Some(domain);
        }
        self.domains[idx] = Some(domain);
        self.count.fetch_add(1, Ordering::SeqCst);
        Some(domain.id)
    }

    pub fn tick(&self, current_tick: u64) {
        if self.count.load(Ordering::Relaxed) == 0 { return; }
        let count = self.count.load(Ordering::SeqCst) as usize;
        for i in 0..count {
            if let Some(dom) = self.domains[i] {
                if dom.is_active() && current_tick >= dom.next_barrier_tick.load(Ordering::SeqCst) {
                    dom.barrier_generation.fetch_add(1, Ordering::SeqCst);
                    dom.next_barrier_tick.store(
                        current_tick + dom.barrier_interval_ticks, Ordering::SeqCst,
                    );
                    dom.push_barrier_snapshot(current_tick);
                    if let Some(cb) = *dom.capture_cb.lock() {
                        unsafe { cb(); }
                    }
                }
                // Health monitoring: escalate to BSR if heartbeat lost
                if !dom.check_health(current_tick) {
                    let gap = current_tick.saturating_sub(
                        dom.last_heartbeat.load(Ordering::SeqCst));
                    crate::klog_ffi!(klog_ffi_warn,
                        "[BARRIER] domain {} heartbeat lost ({gap} ticks)", dom.id);

                    dom.consecutive_failures.fetch_add(1, Ordering::SeqCst);
                    let failures = dom.consecutive_failures.load(Ordering::SeqCst);

                    if failures <= 2 {
                        crate::klog_ffi!(klog_ffi_info,
                            "[BARRIER] domain {} attempt domain-level rollback (failures={})", dom.id, failures);
                        if dom.try_rollback(current_tick, 0) {
                            let (entries, _, _, _) = self.rollback_domain(
                                dom, current_tick, 0, 0,
                            );
                            crate::klog_ffi!(klog_ffi_info,
                                "[BARRIER] domain {} rollback recovered (entries={})", dom.id, entries);
                            dom.mark_recovered();
                            dom.consecutive_failures.store(0, Ordering::SeqCst);
                            continue;
                        }
                    }

                    if failures >= 3 {
                        crate::klog_ffi!(klog_ffi_warn,
                            "[BARRIER] domain {} persistent failures ({failures}), escalating to BSR", dom.id);
                        crate::kernel::barrier::NEED_BSR_ESCALATION.store(true, Ordering::SeqCst);
                        return;
                    }
                }
                // Reset CPU quota per period
                if dom.cpu_quota_period > 0
                    && current_tick.is_multiple_of(dom.cpu_quota_period)
                {
                    dom.reset_quota();
                }
            }
        }
    }

    pub fn find(&self, id: u64) -> Option<&'static RecoveryDomain> {
        let idx = id as usize;
        if idx < DIRECT_MAP_SIZE {
            self.direct_map[idx]
        } else {
            (0..self.count.load(Ordering::SeqCst) as usize)
                .find_map(|i| self.domains[i].filter(|d| d.id == id))
        }
    }

    pub fn check_boot_fingerprints(&self) {
        let count = self.count.load(Ordering::SeqCst) as usize;
        for i in 0..count {
            if let Some(dom) = self.domains[i] {
                let fp = dom.load_boot_fingerprint();
                if fp != 0 && dom.consecutive_failures.load(Ordering::SeqCst) >= 3 {
                    crate::klog_ffi!(klog_ffi_warn,
                        "[BARRIER] domain {} has persistent crash fingerprint 0x{:X} ({} failures), starting in degraded mode",
                        dom.id, fp, dom.consecutive_failures.load(Ordering::SeqCst));
                    dom.set_state(super::types::DomainState::Degraded, Ordering::SeqCst);
                    dom.clear_boot_fingerprint();
                }
            }
        }
    }

    pub fn locate_domain_by_addr(&self, fault_rip: u64) -> Option<u64> {
        let count = self.count.load(Ordering::SeqCst) as usize;
        for i in 0..count {
            if let Some(dom) = self.domains[i] {
                if dom.contains_addr(fault_rip) {
                    return Some(dom.id);
                }
            }
        }
        None
    }

    pub fn locate_domain_by_panic_msg(&self) -> Option<u64> {
        let msg = super::PANIC_MSG.lock();
        let count = self.count.load(Ordering::SeqCst) as usize;

        for mapping in &PANIC_DOMAIN_MAP {
            if msg.len() >= mapping.prefix.len() && msg[..mapping.prefix.len()] == *mapping.prefix {
                for i in 0..count {
                    if let Some(dom) = self.domains[i] {
                        if dom.id == mapping.domain_id {
                            return Some(mapping.domain_id);
                        }
                    }
                }
            }
        }
        None
    }

    pub fn rollback_domain(&self, dom: &RecoveryDomain, tick: u64, fingerprint: u64, depth: usize) -> (usize, u64, u64, i32) {
        let gen_from = dom.barrier_generation.load(Ordering::SeqCst);
        let target_gen = if depth > 0 {
            dom.get_rollback_generation(depth as u32)
        } else {
            gen_from
        };

        dom.set_state(DomainState::RollingBack, Ordering::SeqCst);

        let entries_rolled_back;
        {
            let mut undo = dom.undo.lock();
            undo.current_generation = dom.barrier_generation.load(Ordering::SeqCst);
            entries_rolled_back = undo.rollback_to(target_gen);
        }

        if let Some(cb) = *dom.rollback_cb.lock() {
            unsafe { cb(); }
        }

        let result = if entries_rolled_back > 0 { 0 } else { -1 };
        dom.set_state(DomainState::Active, Ordering::SeqCst);
        dom.mark_recovered();

        log_rollback_event(RollbackEvent {
            tick,
            domain_id: dom.id,
            generation_from: gen_from,
            generation_to: target_gen,
            entries_rolled_back,
            crash_fingerprint: fingerprint,
            cascade_depth: depth,
            result,
        });

        (entries_rolled_back, gen_from, target_gen, result)
    }

    pub fn cascade_rollback(&self, domain_id: u64, tick: u64, fingerprint: u64) -> usize {
        let count = self.count.load(Ordering::SeqCst) as usize;
        let mut queue: [u64; MAX_RECOVERY_DOMAINS] = [0; MAX_RECOVERY_DOMAINS];
        let mut queue_head = 0usize;
        let mut queue_tail = 0usize;
        let mut visited = [false; MAX_RECOVERY_DOMAINS];
        let mut rolled_back = 0usize;

        let target_idx = (0..count).find(|&i| {
            self.domains[i].map_or(false, |d| d.id == domain_id)
        });

        if let Some(idx) = target_idx {
            visited[idx] = true;
            queue[queue_tail] = domain_id;
            queue_tail += 1;
        }

        while queue_head < queue_tail {
            let current_id = queue[queue_head];
            queue_head += 1;

            if let Some(dom) = self.find(current_id) {
                if dom.try_rollback(tick, fingerprint) {
                    let (entries, _, _, _) = self.rollback_domain(
                        dom, tick, fingerprint, rolled_back,
                    );
                    if entries > 0 {
                        rolled_back += 1;
                    }

                    let depended_by = dom.depended_by.lock();
                    for slot in depended_by.iter() {
                        if let Some(dep_id) = *slot {
                            // BFS queue overflow guard
                            if queue_tail >= MAX_RECOVERY_DOMAINS {
                                crate::klog_ffi!(klog_ffi_warn,
                                    "[BARRIER] cascade BFS queue overflow at dom={}", dep_id);
                                break;
                            }
                            let dep_idx = (0..count).find(|&i| {
                                self.domains[i].map_or(false, |d| d.id == dep_id)
                            });
                            if let Some(di) = dep_idx {
                                if !visited[di] {
                                    visited[di] = true;
                                    queue[queue_tail] = dep_id;
                                    queue_tail += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        rolled_back
    }
}
