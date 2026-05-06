# 用户态进程进入 — 稳健方案实施报告

**日期**: 2026-05-07 | **分支**: main | **提交**: f318bb1

---

## 概述

解决用户态进程进入（iretq → user mode）时的 Page Fault 问题。核心策略：将用户页映射进内核 CR3，设置全路径 U/S 标志位，避免 CR3 切换。

## 根本原因分析

```
内核 PML4 页表结构（boot.asm 创建）:
  PML4[0]    → PDPT[0]  (flags: 0x03 Present|Writable, U/S=0)
    PDPT[0]  → PD[0-511] (512 × 2MB huge pages, flags: 0x87)
      PD[n]  → 2MB frame at phys=n*2MB (PS=1, Present|Writable|User)

用户代码映射到用户CR3:
  PML4[0]    → 新的PDPT (由vmm_map_page_in_table创建)
    PDPT[0]  → 新的PD
      PD[2]  → 新的PT (4KB拆分)
        PT[n] → 用户代码页 (Present|User)

问题:
  1. PML4[0], PDPT[0], PD[n] 所有条目的U/S=0 (boot.asm设置0x03)
     用户态(CPL=3)访问 → Page Fault (Error Code 0x5 = Protection Violation)
  2. CR3切换后内核栈(低地址identity-mapped)不可达
  3. 2MB大页不能与4KB页面共存于同一地址范围
```

## 修复方案

### 1. 大页拆分 (split_2mb_page)
```rust
// mm/vmm.rs
pub fn split_2mb_page(&self, virt: u64) -> Result<(), &'static str>
```
- 找到目标地址所在的 2MB PD条目
- 分配新4KB页表 (PT)
- 填充512个4KB PTE条目 (每个映射phys+offset)
- 替换PD条目: 指向新PT, 清除PS标志

### 2. 全路径U/S设置 (ensure_path_user)
```rust
// mm/vmm.rs
pub fn ensure_path_user(&self, virt: u64)
```
- 遍历 PML4[?] → PDPT[?] → PD[?] 三层次
- 逐层设置 U/S=1
- 用户代码页的PTE条目由vmm_map_page设置U/S

### 3. 双映射策略
```rust
// user_proc.rs — 三个调用点:
// a) load_elf_from_memory (ELF加载)
// b) create_from_binary (二进制加载)
// c) create (用户栈分配)

// 每个调用点的模式:
vmm_map_page_in_table(user_cr3, vaddr, phys, flags);  // 用户CR3映射
vmm_split_2mb_page(vaddr);                             // 拆分内核大页
vmm_map_page(vaddr, phys, flags);                      // 内核PML4映射 + PAGE_USER
vmm_ensure_path_user(vaddr);                           // 设置路径U/S
```

### 4. enter()简化
```rust
// 移除: CR3切换, high-half RSP转换
// 保留: TSS RSP0设置, iretq帧构建
core::arch::asm!(
    "cli",
    "mov ds, dx", "mov es, dx", "mov fs, dx", "mov gs, dx",
    "push {ss}", "push {rsp}", "push {rflags}", "push {cs}", "push {rip}",
    "iretq",
    ...options(noreturn)
);
```

## 文件清单

| 文件 | 变更 | 说明 |
|------|------|------|
| `src/mm/vmm.rs` | +83行 | split_2mb_page + ensure_path_user |
| `src/mm/ffi.rs` | +27行 | FFI包装函数 |
| `src/proc/user_proc.rs` | +37/-14行 | 双映射 + enter简化 |
| `src/proc/ffi.rs` | -2行 | 移除调试标记 |

## 技术决策

| 决策 | 理由 | 底线检验 |
|------|------|----------|
| 共享CR3不切换 | 消除内核栈地址转换复杂性 | P2(必要): 解决真实PF问题 |
| 大页拆分为4KB | 允许用户页与内核映射共存 | P3(最简): ~50行实现 |
| 全路径U/S设置 | CPU检查所有层级U/S | P1(理解): 逐层walk直观 |
| 保留ISR CR3切换 | 安全冗余, 未来可能独立CR3 | P4(我的): 防患未然 |

## 验证结果

```
✅ 中断启用 (sti) — 0 GPF异常
✅ IDT正常工作
✅ 定时器 (100Hz IRQ 0)
✅ E1000网卡 (IRQ 11) — DHCP获取IP 10.0.2.15
✅ init进程 (PID=3) 加载成功
✅ 用户态进入: CS=0x1B(DPL=3), RIP=0x4013EF
✅ init_main执行2秒+ (syscall路径工作)
✅ 网络应用: HTTP/DNS/Ping/mDNS/MQTT/SNTP全部初始化

⚠ init_main崩溃于用户代码地址0x9EA987
   (独立问题: init_main访问未初始化内核指针)
```

## 已知限制

1. **共享CR3**: 用户进程间的页表隔离未实现
2. **大页拆分开销**: 每个2MB区域的首次映射需要split
3. **init_main崩溃**: 用户层代码需要单独调试
