# AntX 维护工程文档 (2026-06-11 旧版)

> 本文档为 2026-06-11 创建的早期维护工程文档 (6 阶段 46 项计划). 已被 [maintenance-cycle-2026-06-19.md](../maintenance-cycle-2026-06-19.md) 替代. 本归档版本保留历史决策记录, 供回溯参考. 创建于 2026-06-11, 2026-06-26 归档重写.

## 文档元信息
- **元信息条目**
  - 描述: 文档基础信息
  - 方案: 起始日期 2026-06-11; 关联审计 deep-audit-2026-06-11.md; 关联路线图 kernel-roadmap.md; 关联进度 engineering-progress.md; 关联规范 AGENTS.md/CLAUDE.md; 阶段入口 Phase 0 → 1 → 2 → 3 → 4 → 5; 必修项 4 (Phase 0)
  - 状态: [X]

## 阶段化策略
- **6 阶段总览**
  - 描述: 6 阶段工程划分
  - 方案: Phase 0 必修关键 (4 项, 修完才能继续开发) / Phase 1 高优安全与稳定 (4 项, 修完可发内部测试版) / Phase 2 正确性与并发 (10 项, 修完可发技术预览版) / Phase 3 性能与架构改进 (6 项, 修完可发公测版) / Phase 4 维护性与代码质量 (11 项, 修完可标记 Beta) / Phase 5 文档与工具链 (11 项, 修完可标记 RC) / Phase 6 长期改进 (延后, 不阻塞发版); 总计 46 项 (其余 8 项为 I-03/I-07/I-09/I-11/I-12/I-14/I-21/I-22 的延后/合入/已知条目, 归入 Phase 6)
  - 状态: [X]

## Phase 0 必修关键 (4 项)
- **I-26 [严重] Demand paging 整条路径未激活**
  - 描述: Demand paging 路径未激活
  - 方案: 已通过 B2 Page Cache + 文件 mmap 工程完成 (详见 maintenance-cycle-2026-06-19.md §B2); 合并 I-23 Page Fault trait 双路径
  - 状态: [X]
- **I-31 [严重] execve 失败时进程不可恢复**
  - 描述: execve 失败时进程不可恢复
  - 方案: 已通过 A3 execve + 用户态 ASLR 工程完成
  - 状态: [X]
- **I-29 [高] TEST_PWM fallback 绕过访问控制**
  - 描述: TEST_PWM fallback 安全漏洞
  - 方案: 已通过 fix/I-29-remove-test-pwm-fallback 分支完成
  - 状态: [X]
- **I-36/37/38 [高] exception table 缺失: 3 处内核写用户空间**
  - 描述: 3 处内核写用户空间缺 exception table
  - 方案: 已通过 fix/I-36-37-38-exception-table 分支完成; 后续用户态 Stack Canary 工程 (madvise/mlock/canary) 进一步强化
  - 状态: [X]

## Phase 1 高优安全与稳定 (4 项)
- **I-15 [高] HvFS ZIL 日志回放 11 处 unwrap()**
  - 描述: HvFS ZIL 日志回放 11 处 unwrap()
  - 方案: 已通过 fix/I-15-zil-replay-panic 分支完成; 后续 QUAL-1 非 test 代码 unwrap() 消除工程完成
  - 状态: [X]
- **I-17 [中] framework 15 模块使用第三方 spin::Mutex 不参与 Lockdep**
  - 描述: 15 模块使用 spin::Mutex 不参与 Lockdep
  - 方案: 已通过 fix/I-17-spin-mutex-migration 分支完成; 后续 Lockdep 死锁检测工程 (C6 Phase C) 集成
  - 状态: [X]
- **I-01 [高] TCB 占比远超星绽基线 (~87% vs 14%)**
  - 描述: TCB 占比 87% 远超星绽基线
  - 方案: 首批提取 D8 FdTable → services/proc/fd_table.rs (详见 tcb-reduction-plan.md 已完成提取 -10,400+ LoC)
  - 状态: [X]
- **I-02 [高] usermode.rs Ring 3 切换占位实现**
  - 描述: Ring 3 切换占位实现
  - 方案: 已通过 fix/I-02-ring3-wiring 分支完成; 后续 Phase A1-A4 用户态可启动工程完成
  - 状态: [X]

## Phase 2 正确性与并发 (10 项)
- **I-23 [中] Page Fault trait + 直接方法双路径**
  - 描述: Page Fault trait + 直接方法双路径
  - 方案: 合并入 I-26 ✅ 随 I-26 合并 (2026-06-11)
  - 状态: [X]
- **I-27 [中] handle_simple_fault 硬编码 WRITABLE+USER flags**
  - 描述: handle_simple_fault 硬编码
  - 方案: 已修复 2026-06-12
  - 状态: [X]
- **I-28 [中] kmalloc/kmalloc_slab 自旋锁未 disable interrupts**
  - 描述: kmalloc 自旋锁未 disable interrupts
  - 方案: 已通过 fix/I-28-kmalloc-disable-irqs 分支完成
  - 状态: [X]
- **I-30 [中] Session Manager UnsafeCell 全局单例 → per-process 化**
  - 描述: Session Manager per-process 化
  - 方案: 已通过 refactor/I-30-session-per-process 分支完成; 后续进程组/会话/控制终端工程完成
  - 状态: [X]
- **I-32 [中] ELF loader RacyCell 静态分配器非线程安全**
  - 描述: ELF loader RacyCell 非线程安全
  - 方案: 已通过 fix/I-32-elf-loader-racy-cell 分支完成; 后续 refactor/I-33-elf-verify-unify 进一步强化
  - 状态: [X]
- **I-39 [中] sys_ioctl stub 返回 0 而非 ENOSYS**
  - 描述: sys_ioctl stub 返回值错误
  - 方案: 已通过 fix/I-39-ioctl-enosys 分支完成
  - 状态: [X]
- **I-40 [中] sigreturn trampoline 仅 x86_64 机器码**
  - 描述: sigreturn trampoline 仅 x86_64
  - 方案: 已通过 fix/I-40-sigreturn-trampoline-dual-arch 分支完成; 后续用户态 Stack Canary 工程 (双架构)
  - 状态: [X]
- **I-41 [中] socket 自旋持锁剥夺 ISR 锁**
  - 描述: socket 自旋持锁
  - 方案: 已通过 refactor/I-41-socket-wait-queue 分支完成; 后续 C2 CPU 亲和性 + C1 epoll 进一步优化
  - 状态: [X]
- **I-45 [中] 信号栈帧未检查 sigaltstack 替代栈**
  - 描述: sigaltstack 替代栈未检查
  - 方案: 已通过 fix/I-45-sigaltstack 分支完成
  - 状态: [X]
- **I-18 [中] FileSystem trait 缺少 fs_sync 方法**
  - 描述: FileSystem trait 缺 fs_sync
  - 方案: 已通过 feature/P3-I-18-fs-sync-trait 分支完成; 详见 vfs-policy-extraction.md
  - 状态: [X]

## Phase 3 性能与架构改进 (6 项)
- **预存 [低] framework SAFETY 注释覆盖 (33 处遗留)**
  - 描述: framework SAFETY 注释覆盖
  - 方案: 已修复; 后续 QUAL-3 unsafe impl Send/Sync 补 SAFETY 注释工程完成
  - 状态: [X]
- **I-19 [中] vfs_pread_inode 绕过 trait 分发**
  - 描述: vfs_pread_inode 绕过 trait 分发
  - 方案: 已修复 2026-06-11
  - 状态: [X]
- **I-20 [中] 错误处理风格不统一 (Result/Errno/return -1)**
  - 描述: 错误处理风格不统一
  - 方案: 第一阶段修复 2026-06-11; 后续 B-04 错误类型统一 (KernelError) 工程完成
  - 状态: [X]
- **I-42 [中] virtio-blk 忙等自旋而非中断驱动**
  - 描述: virtio-blk 中断驱动
  - 方案: 第一阶段修复 2026-06-11 (I-42-1 ISR acknowledge + I-42-2 多 outstanding I/O + I-42-3 多实例支持)
  - 状态: [X]
- **I-43 [中] 块设备存在 BlockDevice trait 和 Chitin proto_block 双重抽象**
  - 描述: 双重抽象
  - 方案: 单一桥接不变式 2026-06-11; 详见 LEGACY-4 BlockOps thunk 移除
  - 状态: [X]
- **I-44 [中] 网络恢复 net_save 为 no-op**
  - 描述: net_save 为 no-op
  - 方案: 已通过 feature/P2-I-44-net-save 分支完成
  - 状态: [X]
- **I-50 [低] hrtimer 未集成到 tick handler**
  - 描述: hrtimer 集成
  - 方案: 已修复 2026-06-11; 后续 A1 hrtimer 高精度定时器框架工程完成
  - 状态: [X]

## 预存问题修复记录
- **预存问题清单**
  - 描述: 2026-06-11 修复预存问题
  - 方案: 预存-1 net::init 模块在 kernel_test feature 下缺失已修复 / 预存-2 e1000.rs 的 IoMem 误标 cfg gate (E0425) 已修复; 详细列表见原文档 (本归档版本精简)
  - 状态: [X]

## 关联文档
- **新版维护文档**
  - 描述: 已替代本文档
  - 方案: [maintenance-cycle-2026-06-19.md](../maintenance-cycle-2026-06-19.md) 为当前维护周期主文档, 涵盖本文所有 46 项的状态更新与 [X] 标记; 配套 [engineering-progress.md](../engineering-progress.md) 跟踪主线工程进度; 详见 kernel-roadmap.md Phase A-D 路线图
  - 状态: [X]
- **审计源文档**
  - 描述: 关联审计
  - 方案: [deep-audit-2026-06-11.md](./deep-audit-2026-06-11.md) 54 项审计源, 已完成 23/23 审计项
  - 状态: [X]

