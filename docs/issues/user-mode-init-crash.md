# 问题: 用户态 init_main 执行 2 秒后跳入内核 BSS 区 (0x9EA987)

**日期**: 2026-05-07 | **状态**: ✅ 已修复 | **严重程度**: 高

---

## 一、问题现象

用户态 init 进程 (PID=3) 通过 `iretq` 成功进入用户模式 (CS=0x1B DPL=3)，`init_main` 正常运行约 2 秒，完成了网络栈初始化 (DHCP/Ping/DNS/HTTP 等)，之后突然跳转到地址 **0x9EA987** 导致 Page Fault。

```
0.999839148 [INFO] Init process loaded with PID: 3
1.001243586 [INFO] Init process started with PID: 3

...（2 秒内正常执行网络初始化）...

4.126765781 [INFO] Ping reply from 10.0.2.2 seq=3         ← 最后一条正常日志
3.375166810 [CRIT] EXCEPTION: Page Fault                    ← 时间戳回跳！(4.12→3.37)
3.376716778 [INFO]   RIP: 0x9ea987                         ← 跳入内核 BSS 区
3.377069683 [INFO]   CS:  0x1b (DPL=3)                     ← 仍在用户态
3.384517375 [INFO]   Fault Address (CR2): 0xffffe000       ← 栈顶不可达
```

## 二、关键异常点

### 2.1 时间戳跳变
正常日志时间戳单调递增到 ~4.1 秒，但 PF 日志突然回跳到 `3.375166810`。QEMU 使用 `-nographic`，时间戳来自内核 `timer_get_ticks()`。跳变可能原因：
- 定时器中断触发异常处理 → 嵌套中断打乱了日志顺序
- 或者日志缓冲区未刷新导致乱序

### 2.2 RIP=0x9EA987 的来源
`0x9EA987` 不在用户 ELF 的任何映射范围内（用户段: `0x400000-0x405000`, `0x403000-0x404800`, `0x405000-0x406664`）。这是**内核 BSS 区域**的地址——即内核的未初始化全局变量区。

可能成因：
- **函数指针为 NULL/野指针**: `call *0x0` 或 `call *(未初始化指针)` 跳入 BSS
- **栈帧损坏**: 某个 `ret` 指令从栈上弹出了错误的返回地址
- **syscall 返回路径错误**: `iretq` 恢复了错误的 RIP

### 2.3 CR2=0xFFFFE000 的问题
`0xFFFFE000` 是用户栈顶 `0x7FFFFFFFE000` 的低 32 位截断 (日志用 `0x%x`)。`init_main` 执行期间触碰了已经`process_exit` 释放的栈，说明进程在被 kill 后（第一个 PF），内核尝试恢复执行但栈映射已失效。

### 2.4 寄存器值的可疑模式
反复 PF 的寄存器快照始终相同：

| 寄存器 | 值 | 猜测含义 |
|--------|-----|----------|
| RAX | 0x4013ef | ? 旧版 `_start` 地址 |
| RBX | 0x23 | 用户 SS |
| RCX | 0xab2d40 | 内核地址 (malloc 分配?) |
| RDX | 0xffffe000 | 用户 RSP 低 32 位 |
| RSI | 0x1b | 用户 CS |
| RDI | 0x3202 | RFLAGS (IF+IOPL=3) |
| RBP | 0x116fe0 | 内核栈帧地址 |

**高度怀疑**: 这些寄存器值是 `iretq` 帧的 5 个字段（SS, RSP, RFLAGS, CS, RIP）**被错误地当作通用寄存器读回**。说明 ISR stub 中的 `push` 寄存器顺序与 `struct interrupt_frame` 的字段布局不匹配。

## 三、内核基础设施状态

| 组件 | 状态 | 备注 |
|------|------|------|
| IDT / 异常处理 | ✅ 正常 | 0-31 异常正确分发 |
| 中断启用 (sti) | ✅ 正常 | 无 GPF |
| 定时器 IRQ 0 | ✅ 正常 | 100Hz |
| E1000 IRQ 11 | ✅ 正常 | DHCP/Ping 成功 |
| 用户态进入 (iretq) | ✅ 正常 | CS=0x1B DPL=3 |
| 用户页映射 (内核 CR3) | ✅ 正常 | 大页拆分 + U/S 标志 |
| syscall (int 0x80) | ⚠ 待验证 | init_main 调用了 syscall |
| 用户栈 | ⚠ 待验证 | RSP=0x7FFFFFFFE000 |

## 四、求助方向

1. **函数指针初始化**: `init_main` → `user_install_check_needed` → `user_install_run` 链中，是否有通过函数指针调用的路径？建议用 `objdump -d build/user/init.bin | grep "call \*"` 排查间接调用。

2. **syscall 返回栈帧**: `int $0x80` 时 CPU 自动压栈 (SS, RSP, RFLAGS, CS, RIP)，`iretq` 恢复。中间 `syscall_handler` 是否修改了栈上的 RIP/CS 字段？

3. **ISR 寄存器保存顺序**: 异常日志中的寄存器值与 `struct interrupt_frame` 的偏移是否匹配？CFI (Call Frame Information) 可以用 `readelf -wF build/kernel.bin` 验证。

4. **时间戳跳变**: `timer_get_ticks()` 或 `klog` 在嵌套中断场景是否有竞争条件？

## 五、环境信息

```
QEMU: qemu-system-x86_64 (flat binary, -kernel)
内核: 构建于 2026-05-07, commit f318bb1 / 511fd8c
用户 ELF: entry=0x4012fc, _start=0x4012fc
页表: 内核 CR3 共享, 用户页通过 vmm_map_page + ensure_path_user 映射
进程: PID=3, 入口 0x4012fc, 栈顶 0x7FFFFFFFE000
```

## 六、诊断建议

```bash
# 1. 查看所有间接调用
objdump -d build/user/init.bin | grep -E "call\s+\*|jmp\s+\*"

# 2. QEMU 带调试输出 (抓取每次异常)
qemu-system-x86_64 -d int,cpu_reset -D /tmp/qemu_trace.log ...

# 3. 检查 sys_fs_write 的 syscall 路径
objdump -d build/user/init.bin | grep -A5 "sys_fs_write>:"

# 4. 验证 ISR 帧布局
readelf -wF build/kernel.bin | grep -A50 "isr_common_stub"
```
