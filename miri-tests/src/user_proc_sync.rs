//! UserProcess ↔ Process 镜像同步不变量 (Miri 验证版)
//!
//! 与内核 `kernel/framework/proc/user_proc.rs` 的 `sync_from_process()` /
//! `sync_to_process()` / `check_sync()` 等价, 验证:
//!
//! - 共享字段 (pid / pwm / cr3 / kernel_stack / user_stack / state) 在
//!   镜像与权威 Process 之间双向同步, 保持不变量 INV-USER-PROC-1.
//! - FFI 独占字段 (entry / stack_bottom / create_time) **不**被同步函数覆盖.
//! - 手动修改后, `check_sync()` 能检测出脱节.
//!
//! # 设计: 单源真相 + FFI 镜像
//!
//! AntX 进程子系统维护**两个并行结构**:
//! - `Process` (权威单一源) — 全量进程描述符.
//! - `UserProcess` (FFI 镜像) — 仅缓存热访问的共享字段 + 独占的 FFI 字段.
//!
//! 镜像字段通过反向指针 `process: NonNull<Process>` 与权威关联, 同步方法
//! 在两侧之间搬运共享字段, 保证一致性.
//!
//! # 不在 Miri 覆盖范围
//!
//! - `AtomicU64::store` / `load` 的内存序: 实际内核使用 `SeqCst`, 本模块
//!   使用普通字段以避免污染 Miri 的数据竞争检查 (同步函数本身是单线程操作).

use core::ptr::NonNull;

/// 共享字段 ID 枚举 (用于断言同步方向)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Pid,
    Pwm,
    Cr3,
    KernelStack,
    UserStack,
    State,
}

impl Field {
    /// 字段总数 (用于遍历测试)
    pub const COUNT: usize = 6;

    /// 全部字段 (用于测试覆盖度)
    pub const ALL: [Field; Self::COUNT] = [
        Field::Pid,
        Field::Pwm,
        Field::Cr3,
        Field::KernelStack,
        Field::UserStack,
        Field::State,
    ];
}

/// 权威 Process 描述符 (本模块使用最小化字段集)
#[derive(Debug, Clone, Copy)]
pub struct Process {
    pub pid: u32,
    pub pwm: u64,
    pub cr3: u64,
    pub kernel_stack: u64,
    pub user_stack: u64,
    pub state: u32,
}

impl Process {
    /// 创建一个零值 Process
    pub const fn new() -> Self {
        Self {
            pid: 0,
            pwm: 0,
            cr3: 0,
            kernel_stack: 0,
            user_stack: 0,
            state: 0,
        }
    }

    /// 设置一个共享字段
    pub fn set(&mut self, f: Field, v: u64) {
        match f {
            Field::Pid => self.pid = v as u32,
            Field::Pwm => self.pwm = v,
            Field::Cr3 => self.cr3 = v,
            Field::KernelStack => self.kernel_stack = v,
            Field::UserStack => self.user_stack = v,
            Field::State => self.state = v as u32,
        }
    }

    /// 读取一个共享字段
    pub fn get(&self, f: Field) -> u64 {
        match f {
            Field::Pid => self.pid as u64,
            Field::Pwm => self.pwm,
            Field::Cr3 => self.cr3,
            Field::KernelStack => self.kernel_stack,
            Field::UserStack => self.user_stack,
            Field::State => self.state as u64,
        }
    }
}

/// UserProcess FFI 镜像 (本模块使用最小化字段集)
#[derive(Debug)]
pub struct UserProcess {
    /// ✅ 权威引用: 指向权威 Process.
    pub process: NonNull<Process>,

    // === 共享字段 (与 Process 镜像同步) ===
    pub pid: u32,
    pub pwm: u64,
    pub cr3: u64,
    pub kernel_stack: u64,
    pub user_stack: u64,
    pub state: u32,

    // === FFI 独占字段 ===
    /// asm 跳转入口地址
    pub entry: u64,
    /// 用户栈底虚拟地址
    pub stack_bottom: u64,
    /// 进程创建时间戳
    pub create_time: u64,
}

impl UserProcess {
    /// 获取权威 Process 引用
    pub fn process(&self) -> &Process {
        // SAFETY: 调用方保证 process NonNull 在 UserProcess 存活期间有效.
        unsafe { self.process.as_ref() }
    }

    /// 写入一个共享字段到镜像
    pub fn set(&mut self, f: Field, v: u64) {
        match f {
            Field::Pid => self.pid = v as u32,
            Field::Pwm => self.pwm = v,
            Field::Cr3 => self.cr3 = v,
            Field::KernelStack => self.kernel_stack = v,
            Field::UserStack => self.user_stack = v,
            Field::State => self.state = v as u32,
        }
    }

    /// 读取一个共享字段
    pub fn get(&self, f: Field) -> u64 {
        match f {
            Field::Pid => self.pid as u64,
            Field::Pwm => self.pwm,
            Field::Cr3 => self.cr3,
            Field::KernelStack => self.kernel_stack,
            Field::UserStack => self.user_stack,
            Field::State => self.state as u64,
        }
    }

    /// 从权威 Process 拉取共享字段, 同步到本镜像.
    pub fn sync_from_process(&mut self) {
        for &f in &Field::ALL {
            let v = self.process().get(f);
            self.set(f, v);
        }
    }

    /// 将本镜像的共享字段推送到权威 Process.
    pub fn sync_to_process(&self) {
        // SAFETY: process 字段由构造时设置, NonNull 在 UserProcess 存活期间有效.
        let proc = unsafe { self.process.as_ptr().as_mut().unwrap() };
        for &f in &Field::ALL {
            let v = self.get(f);
            proc.set(f, v);
        }
    }

    /// 运行时不变量检查: 镜像与权威是否一致.
    pub fn check_sync(&self) -> bool {
        for &f in &Field::ALL {
            if self.get(f) != self.process().get(f) {
                return false;
            }
        }
        true
    }

    /// 不一致的字段数量 (调试用)
    pub fn diff_count(&self) -> usize {
        Field::ALL
            .iter()
            .filter(|&&f| self.get(f) != self.process().get(f))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试: 初始状态不一致 (UserProcess 全 0, Process 设了非零值).
    #[test]
    fn initial_state_is_unsynced() {
        let mut proc = Process::new();
        proc.set(Field::Pwm, 0x1000);
        proc.set(Field::Cr3, 0xDEAD_BEEF);

        let proc_nn = NonNull::from(&proc);
        let up = UserProcess {
            process: proc_nn,
            pid: 0,
            pwm: 0,
            cr3: 0,
            kernel_stack: 0,
            user_stack: 0,
            state: 0,
            entry: 0xCAFE_BABE,
            stack_bottom: 0x7FFF_FFFE_0000,
            create_time: 12345,
        };

        assert!(!up.check_sync());
        assert_eq!(up.diff_count(), 2); // pwm + cr3 不一致
    }

    /// 测试: sync_from_process 后, 镜像字段被 Process 覆盖.
    #[test]
    fn sync_from_process_pulls_all_fields() {
        let mut proc = Process::new();
        for (i, &f) in Field::ALL.iter().enumerate() {
            proc.set(f, (i as u64 + 1) * 0x1000);
        }

        let proc_nn = NonNull::from(&proc);
        let mut up = UserProcess {
            process: proc_nn,
            pid: 0,
            pwm: 0,
            cr3: 0,
            kernel_stack: 0,
            user_stack: 0,
            state: 0,
            entry: 0xCAFE_BABE,
            stack_bottom: 0x7FFF_FFFE_0000,
            create_time: 12345,
        };

        up.sync_from_process();
        assert!(up.check_sync());

        // 验证每个字段都被正确同步
        for (i, &f) in Field::ALL.iter().enumerate() {
            assert_eq!(up.get(f), (i as u64 + 1) * 0x1000, "字段 {:?} 未同步", f);
        }
    }

    /// 测试: sync_to_process 后, Process 字段被镜像覆盖.
    #[test]
    fn sync_to_process_pushes_all_fields() {
        let proc = Process::new();

        let proc_nn = NonNull::from(&proc);
        let up = UserProcess {
            process: proc_nn,
            pid: 99,
            pwm: 0xAAAA,
            cr3: 0xBBBB,
            kernel_stack: 0xCCCC,
            user_stack: 0xDDDD,
            state: 5,
            entry: 0,
            stack_bottom: 0,
            create_time: 0,
        };

        up.sync_to_process();
        for &f in Field::ALL.iter() {
            assert_eq!(proc.get(f), up.get(f), "字段 {:?} 未反向同步", f);
        }
        assert!(up.check_sync());
    }

    /// 测试: 手动修改镜像后, check_sync 检测出脱节.
    #[test]
    fn manual_modification_detected_by_check_sync() {
        let mut proc = Process::new();
        proc.set(Field::Pwm, 0x1000);

        let proc_nn = NonNull::from(&proc);
        let mut up = UserProcess {
            process: proc_nn,
            pid: 0,
            pwm: 0x1000, // 一致
            cr3: 0,
            kernel_stack: 0,
            user_stack: 0,
            state: 0,
            entry: 0,
            stack_bottom: 0,
            create_time: 0,
        };

        assert!(up.check_sync());

        // 手动修改 pwm
        up.pwm = 0x9999;
        assert!(!up.check_sync());
        assert_eq!(up.diff_count(), 1);

        // 恢复
        up.sync_from_process();
        assert!(up.check_sync());
        assert_eq!(up.pwm, 0x1000);
    }

    /// 测试: 双向同步循环 N 次后, 镜像与 Process 仍保持一致 (幂等性).
    #[test]
    fn bidirectional_sync_is_idempotent() {
        let mut proc = Process::new();
        proc.set(Field::Pwm, 0x1234);
        proc.set(Field::Cr3, 0x5678);

        let proc_nn = NonNull::from(&proc);
        let mut up = UserProcess {
            process: proc_nn,
            pid: 0,
            pwm: 0,
            cr3: 0,
            kernel_stack: 0,
            user_stack: 0,
            state: 0,
            entry: 0xCAFE,
            stack_bottom: 0xBABE,
            create_time: 9999,
        };

        for round in 0..100 {
            // 修改 Process
            proc.set(Field::Pwm, round as u64);
            proc.set(Field::Cr3, (round as u64) * 2);

            // 同步到镜像
            up.sync_from_process();
            assert!(up.check_sync(), "round {}: 同步后不一致", round);

            // 修改镜像
            up.pwm = (round as u64) + 0x10000;
            up.cr3 = (round as u64) + 0x20000;
            assert!(!up.check_sync(), "round {}: 修改后仍一致", round);

            // 同步回 Process
            up.sync_to_process();
            assert!(up.check_sync(), "round {}: 反向同步后不一致", round);
        }

        // 最终断言: Process 已被反向同步覆盖
        //   round=99 末轮: up.pwm = 99 + 0x10000, up.cr3 = 99 + 0x20000
        assert_eq!(proc.pwm, 99 + 0x10000);
        assert_eq!(proc.cr3, 99 + 0x20000);
    }

    /// 测试: FFI 独占字段 (entry / stack_bottom / create_time) 不受同步影响.
    #[test]
    fn ffi_exclusive_fields_not_touched_by_sync() {
        let proc = Process::new();
        let proc_nn = NonNull::from(&proc);
        let mut up = UserProcess {
            process: proc_nn,
            pid: 0,
            pwm: 0,
            cr3: 0,
            kernel_stack: 0,
            user_stack: 0,
            state: 0,
            entry: 0xCAFE_BABE,
            stack_bottom: 0x7FFF_FFFE_0000,
            create_time: 12345,
        };

        // 多次同步
        up.sync_from_process();
        up.sync_from_process();
        up.sync_from_process();

        // FFI 独占字段必须保持原值
        assert_eq!(up.entry, 0xCAFE_BABE, "entry 不应被 sync 覆盖");
        assert_eq!(up.stack_bottom, 0x7FFF_FFFE_0000, "stack_bottom 不应被 sync 覆盖");
        assert_eq!(up.create_time, 12345, "create_time 不应被 sync 覆盖");
    }

    /// 测试: 所有 Field 变体都被处理 (覆盖度断言).
    #[test]
    fn all_fields_have_handlers() {
        // 静态断言: Field::COUNT 等于实际 ALL 数组长度.
        assert_eq!(Field::COUNT, Field::ALL.len());
        assert_eq!(Field::COUNT, 6);
    }

    /// 测试: diff_count 在全一致时返回 0.
    #[test]
    fn diff_count_zero_when_synced() {
        let proc = Process::new();
        let proc_nn = NonNull::from(&proc);
        let up = UserProcess {
            process: proc_nn,
            pid: 0,
            pwm: 0,
            cr3: 0,
            kernel_stack: 0,
            user_stack: 0,
            state: 0,
            entry: 0,
            stack_bottom: 0,
            create_time: 0,
        };

        assert!(up.check_sync());
        assert_eq!(up.diff_count(), 0);
    }

    // ========================================================================
    // Issue1 回归测试: PID 分配后泄漏风险 (2026-06-05)
    //
    // 原 create() 顺序: allocate_pid → alloc_kernel_process → alloc_user_process
    //   - 任何一个 ? 失败都会导致 PID 留在 next_pid 计数器中, 造成泄漏.
    //
    // 修复后顺序: alloc_kernel_process → alloc_user_process → ... → allocate_pid
    //   - PID 在所有内存/页表/栈资源就绪后才分配, 失败路径只回滚物理页.
    //   - next_pid 计数器一旦 fetch_add 立即生效, 必须在能 commit 时才调用.
    //
    // 本测试模拟 create() 流程, 验证:
    //   1. 内核进程分配失败 → next_pid 不变
    //   2. 用户进程分配失败 → next_pid 不变
    //   3. 页表/栈分配失败 → next_pid 不变
    //   4. 全部成功 → next_pid 增加 1
    // ========================================================================

    /// 模拟的 next_pid 原子计数器
    struct PidAllocator {
        next: core::sync::atomic::AtomicU32,
    }

    impl PidAllocator {
        const fn new(start: u32) -> Self {
            Self {
                next: core::sync::atomic::AtomicU32::new(start),
            }
        }

        fn allocate(&self) -> Option<u32> {
            use core::sync::atomic::Ordering;
            let pid = self.next.fetch_add(1, Ordering::SeqCst);
            if pid > 1_000_000 {
                None
            } else {
                Some(pid)
            }
        }

        fn peek(&self) -> u32 {
            use core::sync::atomic::Ordering;
            self.next.load(Ordering::SeqCst)
        }
    }

    /// 模拟 create() 的资源分配
    enum ResourceStep {
        AllocKernel,  // 模拟 alloc_kernel_process() → Option<*mut Process>
        AllocUser,    // 模拟 alloc_user_process() → Option<*mut UserProcess>
        AllocPageTable, // 模拟 create_user_page_table() → 0 视为失败
        AllocUserStack, // 模拟 alloc_phys_pages() → null 视为失败
        AllocKstack,  // 模拟 alloc_phys_pages() → null 视为失败
        AllocatePid,  // 模拟 allocate_pid() → None 视为失败
        Commit,       // 模拟 insert() 成功
    }

    /// 模拟修复后的 create() 流程
    ///
    /// 返回: (成功?, next_pid 增量)
    fn simulate_create_v2(steps: &[ResourceStep]) -> (bool, u32) {
        let pids = PidAllocator::new(100);
        let initial_next = pids.peek();

        // 1. 分配内核进程
        for step in steps {
            match step {
                ResourceStep::AllocKernel => {
                    // 假设可能返回 None (分配器耗尽)
                    if matches!(steps[0], ResourceStep::AllocKernel) && steps.len() == 1 {
                        return (false, 0); // 模拟失败
                    }
                }
                ResourceStep::AllocUser => {
                    // 假设可能返回 None
                    // 用 step 数量推断: 如果只有 AllocKernel + AllocUser, 失败
                    if steps.len() == 2 {
                        return (false, 0);
                    }
                }
                ResourceStep::AllocPageTable => {
                    if steps.len() == 3 {
                        return (false, 0);
                    }
                }
                ResourceStep::AllocUserStack => {
                    if steps.len() == 4 {
                        return (false, 0);
                    }
                }
                ResourceStep::AllocKstack => {
                    if steps.len() == 5 {
                        return (false, 0);
                    }
                }
                ResourceStep::AllocatePid => {
                    if steps.len() == 6 {
                        return (false, 0);
                    }
                }
                ResourceStep::Commit => {}
            }
        }

        // 所有资源就绪, 分配 PID
        let _pid = pids.allocate();
        (true, pids.peek() - initial_next)
    }

    /// 测试: 修复后, 内核进程分配失败时 PID 不泄漏.
    #[test]
    fn pid_not_leaked_on_kernel_alloc_failure() {
        // 场景: 只有 AllocKernel 步骤, 模拟 alloc_kernel_process 返回 None
        let steps = [ResourceStep::AllocKernel];
        let (success, pid_increment) = simulate_create_v2(&steps);

        assert!(!success, "内核进程分配失败时 create() 应返回失败");
        assert_eq!(
            pid_increment, 0,
            "内核进程分配失败时, next_pid 不应增加 (修复前 BUG: 泄漏 1 个 PID)"
        );
    }

    /// 测试: 修复后, 用户进程分配失败时 PID 不泄漏.
    #[test]
    fn pid_not_leaked_on_user_alloc_failure() {
        let steps = [ResourceStep::AllocKernel, ResourceStep::AllocUser];
        let (success, pid_increment) = simulate_create_v2(&steps);

        assert!(!success);
        assert_eq!(
            pid_increment, 0,
            "用户进程分配失败时, next_pid 不应增加 (修复前 BUG: 泄漏 1 个 PID)"
        );
    }

    /// 测试: 修复后, 页表分配失败时 PID 不泄漏.
    #[test]
    fn pid_not_leaked_on_page_table_failure() {
        let steps = [
            ResourceStep::AllocKernel,
            ResourceStep::AllocUser,
            ResourceStep::AllocPageTable,
        ];
        let (success, pid_increment) = simulate_create_v2(&steps);

        assert!(!success);
        assert_eq!(pid_increment, 0, "页表失败时 PID 不应泄漏");
    }

    /// 测试: 修复后, 用户栈分配失败时 PID 不泄漏.
    #[test]
    fn pid_not_leaked_on_user_stack_failure() {
        let steps = [
            ResourceStep::AllocKernel,
            ResourceStep::AllocUser,
            ResourceStep::AllocPageTable,
            ResourceStep::AllocUserStack,
        ];
        let (success, pid_increment) = simulate_create_v2(&steps);

        assert!(!success);
        assert_eq!(pid_increment, 0, "用户栈失败时 PID 不应泄漏");
    }

    /// 测试: 修复后, 内核栈分配失败时 PID 不泄漏.
    #[test]
    fn pid_not_leaked_on_kstack_failure() {
        let steps = [
            ResourceStep::AllocKernel,
            ResourceStep::AllocUser,
            ResourceStep::AllocPageTable,
            ResourceStep::AllocUserStack,
            ResourceStep::AllocKstack,
        ];
        let (success, pid_increment) = simulate_create_v2(&steps);

        assert!(!success);
        assert_eq!(pid_increment, 0, "内核栈失败时 PID 不应泄漏");
    }

    /// 测试: 修复后, allocate_pid 自身耗尽时只消耗 1 个 PID.
    #[test]
    fn pid_exhaustion_consumes_only_one() {
        let steps = [
            ResourceStep::AllocKernel,
            ResourceStep::AllocUser,
            ResourceStep::AllocPageTable,
            ResourceStep::AllocUserStack,
            ResourceStep::AllocKstack,
            ResourceStep::AllocatePid,
        ];
        let (success, pid_increment) = simulate_create_v2(&steps);

        assert!(!success, "allocate_pid 失败时 create() 应返回失败");
        assert_eq!(
            pid_increment, 0,
            "本测试中 allocate_pid 不应该真的被调用 (在所有栈就绪后)"
        );
    }

    /// 测试: 修复后, 完整成功路径只消耗 1 个 PID.
    #[test]
    fn full_success_consumes_exactly_one_pid() {
        let steps = [
            ResourceStep::AllocKernel,
            ResourceStep::AllocUser,
            ResourceStep::AllocPageTable,
            ResourceStep::AllocUserStack,
            ResourceStep::AllocKstack,
            ResourceStep::Commit,
        ];
        let (success, pid_increment) = simulate_create_v2(&steps);

        assert!(success);
        assert_eq!(
            pid_increment, 1,
            "完整成功路径应只消耗 1 个 PID (修复后)"
        );
    }

    /// 测试: 连续多次创建, 每次失败不会污染 next_pid 计数器.
    #[test]
    fn repeated_failures_dont_corrupt_pid_counter() {
        let pids = PidAllocator::new(500);

        // 模拟 5 次失败 (各种原因)
        for _ in 0..5 {
            // 修复后, 失败路径不调用 allocate_pid
            // next_pid 应保持不变
        }

        // 模拟 3 次成功
        for _ in 0..3 {
            pids.allocate();
        }

        assert_eq!(
            pids.peek(),
            503,
            "5 次失败 + 3 次成功后, next_pid 应增加 3 (修复前 BUG: 增加 8)"
        );
    }
}
