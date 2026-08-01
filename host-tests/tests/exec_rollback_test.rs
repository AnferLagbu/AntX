//! P0-I-31: execve transactional 模型语义测试
//!
//! 验证 proc_exec_replace 改造为 "先加载后销毁" 的事务性语义:
//! - 加载失败 (返回 -1) 时, 原进程应完整保留, PID 仍可被调度
//! - 加载成功时, 旧 PID 销毁, 新 PID 进入活动状态
//! - 不再出现"旧 PID 已释放但未补偿"的状态 (UAF)
//!
//! 不链接 queenx (host-tests 是 mock 层), 通过复刻 Process 状态机
//! 模拟 transactional 流程并断言状态转换.

/// 镜像 queenx 进程表 (简化版, 关注 PID 生命周期)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcState {
    Active,
    Loading,
    Destructed,
}

/// 镜像 queenx Process / UserProc 双源真相
#[derive(Debug, Clone)]
struct Process {
    pid: u32,
    state: ProcState,
    pwm: u64,
}

#[derive(Default)]
struct ProcessTable {
    map: std::collections::BTreeMap<u32, Process>,
}

impl ProcessTable {
    fn new_with(pid: u32, pwm: u64) -> Self {
        let mut t = Self::default();
        t.map.insert(pid, Process { pid, state: ProcState::Active, pwm });
        t
    }

    fn get(&self, pid: u32) -> Option<&Process> {
        self.map.get(&pid)
    }

    fn insert(&mut self, p: Process) {
        self.map.insert(p.pid, p);
    }

    fn remove(&mut self, pid: u32) -> Option<Process> {
        self.map.remove(&pid)
    }
}

/// 模拟 user_proc_load_elf: 加载成功返回新 PID, 失败返回 Err.
/// 失败原因覆盖: ELF 损坏 / OOM / 文件不可读.
fn mock_load_elf(table: &mut ProcessTable, pwm: u64, should_fail: bool) -> Result<u32, i32> {
    if should_fail {
        return Err(-1);
    }
    // 分配新 PID: 简化版 = 当前 max_pid + 1
    let new_pid = table.map.keys().max().copied().unwrap_or(0) + 1;
    table.insert(Process { pid: new_pid, state: ProcState::Active, pwm });
    Ok(new_pid)
}

/// P0-I-31 transactional 版本: 先加载, 失败则原进程保留
fn exec_replace_transactional(
    table: &mut ProcessTable,
    current_pid: u32,
    pwm: u64,
    should_fail: bool,
) -> i32 {
    if table.get(current_pid).is_none() {
        return -1;
    }

    // 阶段 1: 先加载新 ELF
    let load_result = mock_load_elf(table, pwm, should_fail);
    let new_pid = match load_result {
        Ok(pid) => pid,
        Err(e) => {
            // 加载失败 → 原进程完整保留
            return e;
        }
    };

    // 阶段 2: 加载成功, 现在销毁旧进程
    // 从表移除即销毁, old 出 scope 自动 drop (state 字段标记仅用于调试, 测试不读)
    if table.remove(current_pid).is_some() {
        // 已移除, 无需额外操作
    }

    new_pid as i32
}

/// 旧版 "先摧毁再加载" 流程: 加载失败时旧 PID 已成为悬挂引用
fn exec_replace_legacy(
    table: &mut ProcessTable,
    current_pid: u32,
    pwm: u64,
    should_fail: bool,
) -> i32 {
    if table.get(current_pid).is_none() {
        return -1;
    }

    // 阶段 1: 销毁旧 (旧版)
    // 从表移除即销毁, old 出 scope 自动 drop (旧版语义, state 标记仅用于调试)
    if table.remove(current_pid).is_some() {
        // 已移除, 无需额外操作
    }

    // 阶段 2: 加载新 (旧版)
    if should_fail {
        // UAF: 旧 PID 已释放, 但函数返回 -1
        // 调度器仍持有 current_pid → 访问已释放内存
        return -1;
    }
    let new_pid = table.map.keys().max().copied().unwrap_or(0) + 1;
    table.insert(Process { pid: new_pid, state: ProcState::Active, pwm });
    new_pid as i32
}

#[test]
fn transactional_load_failure_preserves_original_process() {
    let mut table = ProcessTable::new_with(7, 0xABCD);
    let ret = exec_replace_transactional(&mut table, 7, 0xABCD, true);
    assert_eq!(ret, -1, "加载失败应返回 -1");
    // 关键断言: 旧进程完整保留
    assert!(table.get(7).is_some(), "P0-I-31 修复: 加载失败时原 PID 必须保留");
    let proc = table.get(7).unwrap();
    assert_eq!(proc.state, ProcState::Active, "原进程状态必须是 Active");
    assert_eq!(proc.pwm, 0xABCD, "原进程凭证不变");
}

#[test]
fn transactional_load_success_replaces_pid() {
    let mut table = ProcessTable::new_with(7, 0xABCD);
    let ret = exec_replace_transactional(&mut table, 7, 0xABCD, false);
    assert!(ret >= 0, "加载成功应返回新 PID");
    let new_pid = ret as u32;
    assert_ne!(new_pid, 7, "新 PID 应与旧不同");
    // 旧 PID 已销毁
    assert!(table.get(7).is_none(), "旧 PID 必须已销毁");
    // 新 PID 处于 Active
    let proc = table.get(new_pid).expect("新 PID 必须存在");
    assert_eq!(proc.state, ProcState::Active);
}

#[test]
fn legacy_load_failure_leaves_orphan() {
    // 反向回归测试: 旧版在加载失败时, 原 PID 已被释放 (UAF 隐患)
    let mut table = ProcessTable::new_with(7, 0xABCD);
    let ret = exec_replace_legacy(&mut table, 7, 0xABCD, true);
    assert_eq!(ret, -1);
    // 旧版下, 7 已不在表中 → 调度器指向 dangling PID
    assert!(table.get(7).is_none(), "旧版遗留: 加载失败但 PID 已释放");
}

#[test]
fn transactional_does_not_leak_failed_load() {
    // 验证: 加载失败时, mock_load_elf 不会留下半成品
    let mut table = ProcessTable::new_with(7, 0xABCD);
    let size_before = table.map.len();
    let _ = exec_replace_transactional(&mut table, 7, 0xABCD, true);
    let size_after = table.map.len();
    assert_eq!(size_before, size_after, "加载失败时进程表大小应不变");
}

#[test]
fn transactional_keeps_process_invariant() {
    // 完整流程: 多次 execve, 每次失败都保留旧进程
    let mut table = ProcessTable::new_with(1, 100);
    let mut current = 1u32;
    for i in 1..=5 {
        let should_fail = i % 2 == 0;
        let ret = exec_replace_transactional(&mut table, current, 100, should_fail);
        if should_fail {
            assert_eq!(ret, -1, "iter {i} 失败应返回 -1");
            assert!(table.get(current).is_some(), "iter {i} 失败时 PID {current} 必保留");
        } else {
            assert!(ret >= 0, "iter {i} 成功应返回新 PID");
            current = ret as u32;
        }
    }
}

#[test]
fn proc_state_transitions_cover_all_variants() {
    // 镜像 queenx 进程状态机: Active → Loading → Destructed
    // 验证所有变体可被构造且互不相等 (供后续扩展加载/销毁用例使用)
    let active = ProcState::Active;
    let loading = ProcState::Loading;
    let destructed = ProcState::Destructed;
    assert_ne!(active, loading, "Active 与 Loading 必须可区分");
    assert_ne!(loading, destructed, "Loading 与 Destructed 必须可区分");
    assert_ne!(active, destructed, "Active 与 Destructed 必须可区分");

    // 验证 Process 可携带所有状态变体
    let p_loading = Process { pid: 1, state: ProcState::Loading, pwm: 0 };
    let p_destructed = Process { pid: 2, state: ProcState::Destructed, pwm: 0 };
    assert_eq!(p_loading.state, ProcState::Loading);
    assert_eq!(p_destructed.state, ProcState::Destructed);
}
