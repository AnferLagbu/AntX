# 用户态 init 崩溃修复 - 实施完成报告

**日期**: 2026-05-07 | **状态**: ✅ 编译通过 | **严重程度**: 高

---

## 一、问题回顾

用户态 init 进程 (PID=3) 运行约 2 秒后跳转到内核 BSS 区地址 `0x9EA987`，触发 Page Fault。详见 `docs/issues/user-mode-init-crash.md`。

## 二、根因诊断

经过对 ISR 汇编代码、中断帧结构体定义、syscall 处理路径和调度器的系统性审查，发现以下三个独立 Bug：

### Bug 1 (严重): `interrupt_frame` 结构体字段顺序与 ISR push 顺序不匹配

**文件**: `src/include/idt.h`

ISR `isr_common_stub` 中的 push 顺序为：`rax, rbx, rcx, rdx, rbp, rsi, rdi, r8..r15`

但 `interrupt_frame` 结构体在 `r8` 之后的字段为：`rdi, rsi, rbp, rbx, rdx, rcx, rax`

导致映射错位：
| 结构体字段 | 期望读取 | 实际读到 |
|-----------|---------|---------|
| `frame->rbx` | rbx | **rdx** |
| `frame->rcx` | rcx | **rbx** |
| `frame->rdx` | rdx | **rcx** |

**影响**: `exception_handler` 和 `handle_page_fault` 读取的所有通用寄存器值都是错误的。异常处理逻辑（如判断用户态/内核态 Page Fault、恢复策略等）完全基于错误数据，会导致错误的故障恢复行为，进而引发二次崩溃（RIP 跳入 BSS 区域）。

### Bug 2 (严重): `syscall_handler` 丢失 `rdx`(arg3) 参数

**文件**: `src/kernel/isr.asm`

原代码：
```asm
mov r15, r10        ; 保存 arg4(r10)，而非 arg3(rdx)！
...
mov rcx, r15        ; 将 arg4 当作 arg3 传给 syscall_dispatch
```

**影响**: 所有需要 3 个或更多参数的系统调用（如 `read(fd, buf, count)`, `write(fd, buf, count)`, `open(path, flags, mode)`）的第三个参数都是垃圾值。`write` 可能写 0 字节或巨量字节导致缓冲区溢出，`open` 可能用了错误的 flags 导致文件状态异常。

### Bug 3 (中等): 调度器将 ZOMBIE 进程无限重入队

**文件**: `src/proc/scheduler.rs`

`schedule()` 中 MLFQ 队列对 ZOMBIE 进程执行 `queue.push_back(pid)` 重入队，RT 队列也不检查进程存活性。

**影响**: 当 `exception_handler` 中通过 `process_exit()` 将崩溃进程标记为 ZOMBIE 后，调度器会不断尝试运行该进程（每次 pop 出来发现是 ZOMBIE 又 push 回去），造成调度循环空转。

---

## 三、修复方案

### 修复 1: `interrupt_frame` 结构体字段重排

```diff
- uint64_t rdi, rsi, rbp, rbx, rdx, rcx, rax;
+ uint64_t rdi, rsi, rbp, rdx, rcx, rbx, rax;
```

确保结构体字段顺序与 ISR push 顺序完全一致。

### 修复 2: `syscall_handler` 参数传递修正

```diff
- mov r15, r10        ; 错误: 保存 arg4
- mov rbx, r8
+ mov r15, rdx        ; 正确: 保存 arg3

- mov rcx, r15        ; 错误: 传 arg4
- mov r8, rbx
+ mov rcx, r15        ; 正确: 传 arg3
+ mov r8, [rsp+40]    ; 正确: 从栈读取 arg4(r10)
```

**参数映射验证**:

| syscall_dispatch 参数 | 寄存器 | 值来源 | 
|----------------------|--------|--------|
| arg0 (syscall num) | rdi | r12 ← rax ✓ |
| arg1 | rsi | r13 ← rdi ✓ |
| arg2 | rdx | r14 ← rsi ✓ |
| **arg3** | **rcx** | **r15 ← rdx** ✓ **(修复)** |
| arg4 | r8 | [rsp+0x28] ← r10 ✓ **(修复)** |

### 修复 3: 调度器 ZOMBIE 处理

- MLFQ 队列：仅对 `Blocked` 进程重入队，`Zombie` 进程丢弃
- RT 队列：增加 `ProcessState::Zombie` 存活检查，ZOMBIE 任务跳过并继续查找

---

## 四、附加修复

编译过程中发现两个 C 文件缺少 Rust FFI 函数的外部声明：

| 文件 | 缺失声明 | 修复方式 |
|------|---------|---------|
| `src/kernel/timer.c` | `scheduler_tick()` | 添加 `extern void scheduler_tick(void)` |
| `src/ipc/ipc.c` | `process_get_current_pid()`, `process_get_by_pid()` | 添加 `extern` 声明 |

---

## 五、编译验证

```
✅ kernel.bin:   925,472 字节
✅ user/init.bin:  34,784 字节
✅ user/axsh.bin:  21,432 字节
✅ user/install.bin: 20,840 字节
✅ syscall_handler 符号正确导出
✅ 无反汇编错误
```

ISR 汇编验证（objdump）确认修复后的参数传递路径正确。

---

## 六、修改文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/include/idt.h` | 修改 | 重排 `interrupt_frame` 结构体字段: rbx/rdx/rcx → rdx/rcx/rbx |
| `src/kernel/isr.asm` | 修改 | syscall_handler 参数传递修正：保存 rdx(arg3)，从栈读取 r10(arg4) |
| `src/proc/scheduler.rs` | 修改 | MLFQ 队列 ZOMBIE 进程丢弃；RT 队列 ZOMBIE 存活检查 |
| `src/kernel/timer.c` | 修改 | 添加 `scheduler_tick()` extern 声明 |
| `src/ipc/ipc.c` | 修改 | 添加 `process_get_current_pid()` 和 `process_get_by_pid()` extern 声明 |

备份文件位置（以 `.backup` 后缀保存）：
- `src/include/idt.h.backup`
- `src/kernel/isr.asm.backup`
- `src/proc/scheduler.rs.backup`

---

## 七、建议后续验证

1. **QEMU 测试**: `make run-net` 验证网络栈初始化后不再崩溃
2. **单元测试**: `make test-unit` 验证回归
3. **检查用户态 syscall 调用**: 用 `objdump -d build/user/init.bin | grep "int.*0x80"` 确认 init 程序中的 syscall 调用点

---

**实施者**: AI Assistant (自主开发模式)
**实施时间**: 2026-05-07
**状态**: ✅ **READY FOR TESTING**
