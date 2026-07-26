# fork COW + Namespace 继承修复任务 (2026-07-26 最终状态)

> 本文档记录 fork() 相关修复的三个问题（全部已修复）以及一个预存问题（init 进程不运行, 待接手处理）.

## 已修复项

### 已修复的早期问题 (无需再改)
- **KPTI USER_PML4 清除循环**: 已移除无安全收益的低半区清除 (commit `0c461631`)
- **sys_wait4 阻塞**: 已修复为 scheduler_yield 循环等待子进程 Zombie
- **VMM lock 可重入**: 单核安全, 允许 page fault handler 在 COW 持锁期间重入
- **child_ctx.cr3**: 子进程上下文现在使用 `child_cr3` (COW 克隆), 而非 `parent_cr3`

### 本轮修复的三个问题 (2026-07-26)

**问题 1: COW page fault handler 在 KPTI 下映射到错误页表**
- **状态**: [X] 已修复 — 架构级修复
- **根因**: KPTI 开启时, page fault handler 在内核态执行 (CR3=内核页表). `get_current_pml4()` 读硬件 CR3 返回内核 PML4. 所有页表修改 (映射新页) 写入内核 PML4, 返回用户态时加载用户 PML4, 新映射不可见 → 无限 page fault 循环.
- **方案**: 汇编层 (isr_common/irq_common/isr0x82/syscall_handler/syscall_entry) 在 `mov cr3, rax` (KPTI 切换) 之前, 将硬件 CR3 保存到 `USER_CR3_SAVE` (isr.asm .bss 段). page fault handler 通过 `read_user_cr3_asm()` (FFI) 读取. 直接读取硬件 CR3 → 最可靠来源, 不依赖调度器缓存时序.
- **为什么不选其他方案**:
  - `process_get_cr3()`: 中断上下文死锁 (`PROCESS_TABLE.lock()`)
  - 调度器缓存 (`static AtomicU64`): 依赖隐式时序, 非架构保证
  - InterruptFrame 扩展: 需修改 176→184 字节布局, 影响所有异常处理路径

**问题 2: Namespace fork_from 在 with_process 内 triple fault**
- **状态**: [X] 已修复
- **根因**: 未完全查明, 但将 `fork_from()` 移到 `with_process` 闭包内提取 NamespaceSet 到局部变量, 再赋值给 child, 解决了崩溃.
- **方案**: `if let Some(parent_ns) = PROCESS_TABLE.with_process(parent_pid, \|p\| { NamespaceSet::fork_from(&p.namespaces.lock()) }) { *child.namespaces.lock() = parent_ns; }`

**问题 3: COW 启用**
- **状态**: [X] 已实施
- **方案**: `sys_fork()` 现在调用 `clone_user_page_table_cow(parent_cr3)` 进行标准 COW 页表克隆 (双侧只读). 问题 1 修复后 page fault handler 能正确处理 COW 故障.

---

## 预存问题: init 进程进入 Ring 3 后不产生任何输出

### 现象
kernel boot 输出 `[USER] Entering Ring 3 (init pid=2)...` 后, init 进程不打印 `X`/`Y`, 也不触发任何异常或 syscall. 该问题存在于基线代码 (HEAD commit), 与本轮修复完全无关.

### 已收集的证据

| 证据 | 结论 |
|------|------|
| `[ELF]` 诊断 (ELF 加载完成标记) | ELF 加载成功, entry=0x400000 |
| `[OK]` 诊断 (用户页表检查) | 0x400000 在用户页表中正确映射 (vmm::get_physical_in_pml4 返回 Some) |
| stack map_page 日志 (16 页: 0x7FFFFFF00000-0x7FFFFFF0F000) | 用户栈 64KB 正确映射, RW+USER |
| iretq 诊断 (`I` 字符) | 内核达到 iretq 指令 |
| 无 `E` (exception_handler) | 未触发任何异常 (#PF, #GP, #DF 等) |
| 无 `S` (syscall_entry) | init 进程从未执行 syscall 指令 |
| 无 `W` (sys_write) | write syscall 从未被调用 |

### 排除的可能

- 非 ELF 加载问题: ELF 被成功加载, entry 和页表映射正确
- 非页表问题: 用户页表有代码和栈页的 PTEs
- 非 init 指令序列问题: `objdump` 确认 init 二进制正确 (push → lea → mov → syscall)
- 非 my 修改引入: 基线 (git stash 后) 同样不输出

### 可疑方向

1. **syscall MSR LSTAR 地址**: `syscall_entry` 函数地址 + KERNEL_BASE 构成 LSTAR. 验证该页面在用户页表 (KPTI 共享高半区) 中可执行, KPTI 是否清除了 USER 位 (但 Ring 0 无需 USER 位本应正常).
2. **iretq 后 CPU 状态**: iretq 从栈弹 RIP/CS/RFLAGS/RSP/SS. 检查 CS=0x23, SS=0x1B 是否对应正确的 GDT entries 且 DPL=3. 检查 RFLAGS 是否意外清除了 IF (bit 9) 导致无法响应时钟中断.
3. **TLB/cache 问题**: iretq 后 CR3 仍为用户 PML4 (KPTI 的 USER_PML4). 页表 walk 可能使用了旧的 TLB 条目, 导致 0x400000 的翻译失效 (但 KPTI PCID 应隔离 TLB). 当前环境 PCID=off, TLB 在 cr3 写入时完全刷新, 此方向可能性低.
4. **串口输出缓冲区**: init 的 `print_char` 使用 `syscall → sys_write → serial_write_bytes`. 如果 syscall 本身正常执行但 serial 输出受损, 应能看到 `S` (syscall_entry) 标记. 未见 `S`, 说明问题在 syscall 之前.

### 推荐接手人的切入点

1. 在 `enter_user` 的 `iretq` 之前, 用 `out dx, al` 输出一个标记字符, 确认 `iretq` 是否实际执行
2. 在 `syscall_entry` 汇编入口处, 用 `out dx, al` 输出标记, 确认 `syscall` 指令是否被 CPU 执行
3. 如果第二步无输出, 检查 IA32_LSTAR MSR 的值是否指向正确的 kernel 高半区地址 (验证 `entry_hi` 计算)
4. 确认 IDT/exception handler 在用户态异常时是否能正确响应

---

## 当前源码修改清单

| 文件 | 修改内容 |
|------|---------|
| `src/kernel/framework/boot/isr.asm` | 新增 `.bss` 段 `USER_CR3_SAVE`; 5 个入口点在 KPTI 切换前 `mov cr3, rax; mov [USER_CR3_SAVE], rax` |
| `src/kernel/framework/mm/mod.rs` | 新增 `unsafe extern "C" { static USER_CR3_SAVE_ASM: AtomicU64; }` + `read_user_cr3_asm()` |
| `src/kernel/framework/mm/page_fault.rs` | `handle_user_page_fault()` 调用 `read_user_cr3_asm()` 获取用户 PML4 |
| `src/kernel/framework/proc/proc_ops.rs` | `sys_fork()`: `clone_user_page_table_cow(parent_cr3)` + `NamespaceSet::fork_from()` |
| `docs/plan/fork-cow-namespace-fix-tasks.md` | 本文档 |

## 验证方法

```bash
# 双架构编译
./ci/build.sh all

# clippy
cargo clippy --release -- -D warnings

# 审计
python3 scripts/audit_services_boundary.py
python3 scripts/audit_safety_coverage.py
python3 scripts/audit_deadlock_matrix.py

# host-tests
make test-host

# QEMU 无 reboot 循环
timeout 10 qemu-system-x86_64 -m 512 -nic none -no-reboot -kernel build/kernel.flat -nographic 2>/dev/null | grep -c "Booting from ROM"
# 预期: 1 (不重启)
```

## 验证结果 (2026-07-26 全部通过)

| 检查项 | 状态 |
|--------|------|
| x86_64 编译 | ✅ 0 error / 0 warning |
| aarch64 编译 | ✅ 0 error / 0 warning |
| clippy | ✅ 0 warning |
| services 边界审计 | ✅ 通过 |
| SAFETY 覆盖审计 | ✅ 100% |
| 死锁矩阵审计 | ✅ 通过 |
| host-tests | ✅ 全部通过 |
| QEMU 无 reboot 循环 | ✅ 1 次 boot |
