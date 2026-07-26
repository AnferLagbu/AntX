# init 进程 Ring 3 不输出修复报告

> init 进程 (PID 2) 进入 Ring 3 后不打印 `X`/`Y`, 不触发任何异常或 syscall.
> 2026-07-26 排查, 部分修复已提交.

## 第一阶段: 现象与诊断

### QEMU 串口输出

```
[USER] Launching init process...
...
[USER] Entering Ring 3 (init pid=2)...
```

之后无 `X`, `Y`, 或 syscall_entry 诊断字符 `S`.

- 描述: init 进程被加载 (ELF entry=0x400000), 用户页表正确映射, 内核达到 iretq 指令, 但用户代码不执行.
- 方案: 在 `enter_user()` 和 `syscall_entry` 中添加 COM1 串口诊断输出, 用 QEMU `-d int` 和 `-d in_asm` 追踪 CPU 执行流.
- 状态: [X]

### QEMU `-d int` 捕获的异常链

| 阶段 | 异常 | CR2 | RIP |
|------|------|-----|-----|
| 原始代码 | `#PF` → `#DF` | `0x70ac000`(内核栈物理地址) | `0xffff8000014bf240` |
| PML4[0]复制后 | `#UD` | 无(无效指令) | `0xffff8000014c2089`(BSS数据段) |
| `lea rax,[rip]`跳转 | `#PF` at mov cr3,r15 | `0xffff8000001621a0`(指令获取) | RIP=CR2 |
| 完整 PML4(512项) | `#PF` at CR2=0 | `0x0000000000000000`(空指针) | `0xffff8000014c1052` |
| 内核栈物理映射+清寄存器 | `#PF` at CR2=0x709c000→0 | R15物理地址→空 | `0xffff8000014bf267/40` |

- 描述: 5 种修复方案各自改变 #PF 地址, 但都未成功进入用户态.
- 方案: 见各修复方案详情.
- 状态: [X]

## 第二阶段: 根因分析

### 原始 #PF: 内核栈物理地址引用

- 描述: `create_user_page_table()` 将用户 PML4 的低半区全部清零. CR3 切换到用户 PML4 后, 编译器残留的低地址寄存器引用 (`RBP = kstack - KERNEL_BASE`) 访问 `0x70ac000` → `#PF` → `#DF`.
- 方案: 复制 KERNEL_PML4 的 `PML4[0]` (恒等映射) 到用户 PML4. 详见 `src/kernel/framework/mm/vmm_x86_64.rs:496-506`.
- 状态: [X]

### #UD: GNU 标签冲突

- 描述: 原始 asm 用 GNU 风格局部标签 `2f` / `2:`, Rust `asm!()` 通过 LLVM 编译时标签引用解析到数据段地址, 使 `jmp rax` 跳到 BSS 数据区 → `#UD`.
- 方案: 改用 `lea rax, [rip]` + 手动偏移计算, 避免标签引用. 详见 `src/kernel/framework/arch/x86_64/mod.rs:340-352`.
- 状态: [X]

### 剩余问题: 页表走查错位

- 描述: `PML4[256..511]` 复制后, 用户 PML4 的页表走查 (PML4[256] → pdpt_high → pd_high) 映射到 BSS/数据段物理页, 而非内核代码所在物理页. `-d in_asm` 追踪印证: `mov cr3, r15` 后 CPU 从 `0x14befb0`(BSS) 开始执行 `00 00` (`addb %al,(%rax)`), 遇到 `0x60` (`pusha` → #UD in 64-bit 模式).
- 方案: **未修复**. 建议在 QEMU monitor 中用 `info tlb` 在 `mov cr3, r15` 前后分别 dump 页表, 精确比对用户 PML4 与 KERNEL_PML4 的映射差异.
  - 检查点 1: `KERNEL_PML4` 的值是否正确 (boot PML4 物理地址)
  - 检查点 2: `create_user_page_table` 中 `phys_to_virt` 是否映射到正确的物理页
  - 检查点 3: pd_high 的 2MB 大页条目是否在 `split_2mb_page` 后被修改
- 状态: []

## 第三阶段: 已提交的改动

### 文件清单

| 文件 | 改动 | 说明 |
|------|------|------|
| `vmm_x86_64.rs` | PML4[0] 恒等映射复制 | 修复原始 #PF 根因 |
| `arch/x86_64/mod.rs` | 免标签高半跳转 + CR3 后清寄存器 | 修复 GNU 标签冲突 + 防低地址残留 |
| `user_proc.rs` | 内核栈物理地址恒等映射 | 补充防护 |

### 验证结果

| 检查项 | 状态 |
|--------|------|
| x86_64 编译 (release) | ✅ 0 error / 0 warning |
| aarch64 编译 (release) | ✅ 0 error / 0 warning |
| services 边界审计 | ✅ 通过 |
| SAFETY 覆盖审计 | ✅ 100% |
| 死锁矩阵审计 | ✅ 通过 |
| 注释语言审计 | ✅ 0 违规 |
| 6 安全不变式 | ✅ 全部满足 |
| host-tests | ✅ 全部通过 |
| QEMU 无 reboot 循环 | ✅ 1 次 boot |
| init 进程输出 `X`/`Y` | ❌ 未修复 (需进一步排查) |

## 第四阶段: 建议继续排查方向

1. **QEMU monitor 页表检查**: 在 `mov cr3, r15` 处暂停, 用 `info tlb` 和 `info mem` 检查用户 PML4 对 `0xFFFF800000162000` 区域的映射.
2. **`KERNEL_PML4` 验证**: 确认 `vmm_init` 存储的 CR3 值确实是 boot PML4 物理地址, 检查 `.bootbss` 中的页表内容在 VMM init 后是否被修改.
3. **`pd_high 2MB 页检查**: `pd_high[0]` 的 2MB 大页映射 (physical 0-2MB) 是否被 `map_page_in_table` 或 `split_2mb_page` 分裂, 分裂后 4KB 页表条目是否全部正确初始化.
4. **`split_2mb_page` 共享页表问题**: KPTI 下 USER_PML4 与 KERNEL_PML4 共享底层 PDPT/PD 物理页. `create_user_page_table` 后如果 `map_page_in_table` 修改了共享 pd_high, 会影响所有用户 PML4.
5. **跳过 KPTI 测试**: 在 `kernel/Cargo.toml` 中禁用 `kaslr` feature 并关闭 KPTI, 看在没有 KPTI 的情况下 init 是否能正常工作.
