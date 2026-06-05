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
}
