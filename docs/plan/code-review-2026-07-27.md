# QueenX 全面代码审查问题清单

> 2026-07-27 全面代码审查产出。覆盖 Framework 层、Services 层、安全合规性、用户态与工具链。
> 按优先级分为高/中/低三档, 每项标注位置、影响和建议方案。

---

## 高优先级 (🔴 可能导致系统崩溃/安全漏洞)

### H1: KPTI 异常处理程序未映射在用户页表

- **位置**: `src/kernel/framework/boot/isr.asm` + `src/kernel/framework/link/x86_64.ld`
- **影响**: 用户态触发异常 (如 #PF) 时, 异常处理程序 (isr0-isr31, irq0-irq15, syscall_entry) 在用户页表中未映射, 导致 Triple Fault 和系统挂死
- **根因**: `isr.asm` 中 ISR/IRQ stub 位于 `.text` 段而非 `.kpti_trampoline` 段; 链接脚本 `x86_64.ld` 中 `_kpti_trampoline_start` ~ `_kpti_trampoline_end` 仅覆盖 trampoline 代码, 不包含异常入口
- **方案**:
  - 方案 A: 将 isr.asm 中的入口代码放入 `.kpti_trampoline` section (需调整 NASM `section` 指令和链接脚本)
  - 方案 B: 在 `map_trampoline_in_user_pml4` 中显式映射整个 `.text` 段的异常入口范围
  - 方案 C: 将所有 ISR stub 移入 `.kpti_trampoline` section (最彻底)
- **状态**: []

### H2: 上下文切换无 FPU/SSE 状态保存

- **位置**: `src/kernel/framework/proc/switch.asm`
- **影响**: 如果内核线程或用户进程使用浮点/SIMD 指令, 上下文切换会导致 FPU/XMM 寄存器数据损坏
- **方案**:
  - 短期: 在 `Process` 结构体中添加 FPU 状态区域, switch.asm 中使用 `xsave`/`xrstor` 保存/恢复
  - 长期: 实现 lazy FPU 切换 (CR0.TS 位), 仅在首次使用 FPU 时保存/恢复, 减少切换开销
- **状态**: []

### H3: Socket 错误映射一刀切 InvalidArgument

- **位置**: `src/kernel/services/net/socket.rs`
- **影响**: 所有 framework 返回的错误都被 `.map_err(|_| SocketError::InvalidArgument)` 映射, 丢失 `AddrInUse`/`WouldBlock`/`ConnRefused` 等语义, 导致用户程序无法正确处理网络错误
- **违反**: AGENTS.md §5.2 "错误处理: 传播用 `?`"
- **方案**: 在 framework 网络层定义细粒度错误枚举 (如 `NetError`), services socket.rs 按 variant 映射到对应的 `KernelError` 变体
- **状态**: []

### H4: `net_stack().expect()` 可导致内核 panic

- **位置**: `src/kernel/services/net/mod.rs`
- **影响**: `net_stack()` 使用 `expect()` 获取全局网络栈实例, 如果 `init()` 未调用就访问会 panic; 内核中 panic 不可接受
- **方案**: 改为返回 `Option` 或 `Result`, 由调用方决定处理方式 (返回错误码/延迟初始化)
- **状态**: []

### H5: `per_cpu()` 返回 `&'static` 绕过借用检查

- **位置**: `src/kernel/framework/proc/scheduler.rs` (L182-L200)
- **影响**: `per_cpu()` 从 `Mutex<Option<PerCpuSched>>` 中获取内部引用后, 通过裸指针转为 `&'static PerCpuSched`, 绕过借用检查器; 如果其他线程修改 `Option`, 返回的引用悬垂
- **方案**:
  - 方案 A: 使用 `OnceLock<PerCpuSched>` 替代 `Mutex<Option<PerCpuSched>>`, 初始化后不可变, 生命周期安全
  - 方案 B: 将 `PerCpuSched` 放入 `Box::leak` 获取真正的 `&'static`, 避免悬垂
- **状态**: []

---

## 中优先级 (🟡 代码质量/性能/可维护性)

### M1: PMM 无 double-free 检测

- **位置**: `src/kernel/framework/mm/pmm.rs`
- **影响**: `free_pages` 重复释放同一物理页会破坏 Buddy 空闲链表, 导致后续分配返回已用页面
- **方案**: 在页帧元数据中添加 `allocated` 标志位, `free_pages` 时检查; 或使用 page frame 的引用计数
- **状态**: []

### M2: PMM Buddy 合并未验证伙伴 order

- **位置**: `src/kernel/framework/mm/pmm.rs`
- **影响**: Buddy 合并时只检查伙伴页是否在空闲链表, 未验证伙伴页的 `order` 字段是否匹配当前 order, 可能导致跨阶合并 (如将 4KB 块与 2MB 块合并)
- **方案**: 合并前检查 `buddy_order == current_order`, 不匹配则停止合并
- **状态**: []

### M3: 全局 IrqSpinLock 网络栈瓶颈

- **位置**: `src/kernel/services/net/socket.rs` + `src/kernel/services/net/mod.rs`
- **影响**: 每次 socket 操作 (send/recv/bind/listen...) 都要锁住整个 `SmoltcpNetStack`, 高并发下是严重性能瓶颈; 如果在中断上下文中持锁, 可能死锁
- **方案**:
  - 短期: 将 `IrqSpinLock` 改为 `Mutex` (如果网络操作不在中断上下文)
  - 长期: 拆分为细粒度锁 (per-socket lock, 全局接口配置锁)
- **状态**: []

### M4: 100+ 分支巨型 match 系统调用分发

- **位置**: `src/kernel/services/syscall/dispatch.rs`
- **影响**: 单个 `match num` 块含 100+ 分支, 可读性差, 维护成本高; 每次新增 syscall 需在巨大函数中定位
- **方案**:
  - 按子系统拆分为独立函数 (`dispatch_fs`, `dispatch_proc`, `dispatch_net` 等)
  - 或使用跳转表 (`[Option<fn(&mut InterruptFrame)>; 512]`) 替代 match
- **状态**: []

### M5: PID 空间不可回收

- **位置**: `src/kernel/framework/proc/process.rs` (`ProcessTable`)
- **影响**: 进程退出后 PID 永久占用, 长期运行系统可能耗尽 PID 空间
- **方案**:
  - 实现 PID 回收: Zombie 进程被 `wait4` 回收后释放 PID
  - 或使用位图管理 PID, 支持重新分配
- **状态**: []

### M6: VFS from_u8/from_u32 静默吞非法输入

- **位置**: `src/kernel/services/fs/vfs_types.rs`
- **影响**: `VfsFileType::from_u8` / `VfsSeekWhence::from_u32` 对非法值返回默认值 (File/Set), 不返回错误, 隐藏了调用方的 bug
- **方案**: 改为返回 `Option<Self>` 或 `KernelResult<Self>`, 调用方显式处理非法输入
- **状态**: []

### M7: Process + UserProcess 双结构同步负担

- **位置**: `src/kernel/framework/proc/process.rs` + `src/kernel/framework/proc/user_proc.rs`
- **影响**: 进程信息分散在两个结构体中 (如 exit 状态), 需要同步维护; 新增字段时容易遗漏
- **方案**:
  - 短期: 在 `UserProcess` 中只保留用户态特有字段 (页表/栈), 通用字段统一放 `Process`
  - 长期: 合并为单一 `Process` 结构体, 用户态信息用 `Option<UserContext>` 字段
- **状态**: []

### M8: mount_fs 错误映射粗糙

- **位置**: `src/kernel/services/fs/mod.rs`
- **影响**: `mount_fs` 所有非零 `rc` → `KernelError::Io`, 丢失具体错误语义 (PermissionDenied / NotSupported / AlreadyMounted)
- **方案**: 根据 framework 返回码细分到对应 `KernelError` 变体
- **状态**: []

### M9: 中间页表页权限过宽

- **位置**: `src/kernel/framework/mm/vmm_x86_64.rs`
- **影响**: 新分配的 PDPT/PD/PT 页默认映射为 `PRESENT|WRITABLE`, 无 NO_EXECUTE 位; 理论上如果 PML4 入口被意外映射为 USER, 可从用户态执行中间页表页中的代码
- **方案**: 中间页表页添加 `NO_EXECUTE` 位 (bit 63), 减少攻击面
- **状态**: []

---

## 低优先级 (🟢 改进建议)

### L1: error.rs 缺 `#![deny(unsafe_code)]`

- **位置**: `src/kernel/services/error.rs`
- **影响**: 虽然父模块 `services/mod.rs` 有此声明 (编译期覆盖), 但违反每个文件头部声明的惯例和 AGENTS.md §6 F1 的字面要求
- **方案**: 在文件头部添加 `#![deny(unsafe_code)]`
- **状态**: []

### L2: CFS vruntime 溢出未处理

- **位置**: `src/kernel/framework/proc/cfs.rs`
- **影响**: `vruntime` 为 `u64`, 理论上可溢出回绕, 导致调度顺序错误
- **方案**: 使用 `checked_add` 或定期归零最小 vruntime (与 Linux CFS `min_vruntime` 一致)
- **状态**: []

### L3: dispatch.rs 注释中被注释掉的 DEBUG 代码

- **位置**: `src/kernel/services/syscall/dispatch.rs` (约 L59-61)
- **影响**: 无功能影响, 但违反代码整洁原则
- **方案**: 删除被注释掉的 DEBUG 代码
- **状态**: []

### L4: FileSystem trait 26 方法过多

- **位置**: `src/kernel/services/fs/vfs_types.rs`
- **影响**: 实现方需实现全部 26 个方法 (即使大部分返回 NotSupported), 增加实现负担
- **方案**: 拆分为核心 trait (open/read/write/close/mkdir/readdir/stat) + 可选 extension trait (xattr/snapshot/lock 等)
- **状态**: []

### L5: proc/mod.rs 注释路径错误

- **位置**: `src/kernel/services/proc/mod.rs` (约 L6)
- **影响**: 注释中写 `kernel::crate::kernel::framework::proc::types`, 路径格式有误
- **方案**: 修正为 `crate::kernel::framework::proc::types`
- **状态**: []

### L6: Mutex 实为自旋锁, 缺少文档说明

- **位置**: `src/kernel/framework/sync/spinlock.rs` (Mutex 定义)
- **影响**: 当前 `Mutex` 实现本质是自旋锁 (持锁期间 CPU 空转), 与名称暗示的"睡眠锁"不符; 长时间持锁浪费 CPU
- **方案**:
  - 短期: 在 `Mutex` 文档注释中明确说明当前实现是自旋等待
  - 长期: 实现真正的睡眠锁 (等待队列 + 调度器 yield)
- **状态**: []

---

## 已知已修复问题 (来自项目记忆, 供参考)

> 以下问题已在 2026-07-27 之前的开发中修复, 列出以保持历史可追溯性。

| # | 问题 | 修复 | 日期 |
|---|------|------|------|
| F1 | RSP0 栈页使用 `map_page_in_table` 触发 KPTI 安全门控 | 改用 `map_kernel_page_in_table` | 2026-07-27 |
| F2 | 内核栈物理地址未 identity mapping, CR3 切换后 #PF | 添加 identity mapping | 2026-07-27 |
| F3 | iretq 帧所在页未映射在用户页表 | 映射 `(kstack-40)` 所在页 | 2026-07-27 |
| F4 | 用户态执行 IO 指令 (out dx, al) 触发 #GP | 移除 enter_user_asm 中用户态诊断 out | 2026-07-27 |
| F5 | 初始 RSP 指向 guard page (未映射) | 修正为 `stack_virt + GUARD + SIZE - 8` | 2026-07-27 |
| F6 | isr_common 诊断代码在 KPTI CS 检查前 push, 导致栈偏移错误 | KPTI CS 检查移至 push 前 | 2026-07-27 |
| F7 | syscall_entry 入口处修改 RAX, 破坏 syscall 号 | 入口处绝对禁止修改通用寄存器 | 2026-07-27 |

---

## 审查范围与方法

- **审查日期**: 2026-07-27
- **审查范围**: Framework 层 (mm/proc/boot/sync/dma/pci) + Services 层 (fs/net/driver/syscall/wasm/proc/error) + 审计脚本 (13 个) + CI/构建 + 用户态程序
- **审查方法**: 语义搜索 + 源码逐文件阅读 + 架构合规性检查 (6 安全不变式 / F1-F9 硬规则)
- **产出**: 5 高 + 9 中 + 6 低 = 20 项问题
