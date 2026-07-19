# 死代码消除工程计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除项目中全部 66 处 `#[allow(dead_code)]` 标记，恢复 Rust 编译器 dead_code lint 的完整效力

**Architecture:** 按处置策略分三类：(1) 冗余/冲突/孤立代码 → 直接删除；(2) 预留/扩展代码 → 实现对应功能激活；(3) 文件级抑制 → 收窄为项级或移除。每项独立验证，逐模块推进。

**Tech Stack:** Rust, QueenX Framekernel audit scripts, clippy

**子计划:**
- **D 区 (直接删除)** — 本文档 Task 1~3, 11a~11c, 12~14
- **A 区 (激活实现)** — [dead-code-activation.md](./dead-code-activation.md) Task A-01~A-14c

## Global Constraints

- 双架构编译 0 warning 0 error (`./ci/build.sh all`)
- 审计全部通过 (`ci/audit.sh`)
- host-tests 全部通过 (`make test-host`)
- 中文注释强制
- framework `unsafe` 块必须配 `// SAFETY:` 注释
- smoltcp vendored (第三方锁定) 的 dead_code 不在本次消除范围内

---

## 处置决策总表

### A. 直接删除 (34 项) — ✅ 已完成 (2026-07-18)

冗余、冲突、孤立或无调用者的代码，直接移除。

| # | 位置 | 项 | 理由 |
|---|------|----|------|
| D-01 | `framework/net/unix.rs` | 整个文件 (文件级 allow) | 空 re-export shim，已迁移至 services |
| D-02 | `framework/driver/storage/ata.rs:64` | `ATA_STATUS_DSC` | Seek Complete 位，无诊断代码读取 |
| D-03 | `framework/driver/storage/ata.rs:67` | `ATA_STATUS_CORR` | Corrected Data 位，无诊断代码 |
| D-04 | `framework/driver/storage/ata.rs:69` | `ATA_STATUS_IDX` | Index 位，已过时 |
| D-05 | `framework/driver/storage/ata.rs:85` | `ATA_TIMEOUT_ERR` | 错误码常量，零调用者 |
| D-06 | `framework/driver/input/keyboard.rs:37` | `PS2_STATUS_SYSTEM` | 无 PS/2 诊断代码 |
| D-07 | `framework/driver/input/keyboard.rs:42` | `KB_CMD_ECHO` | 无 echo 命令代码 |
| D-08 | `framework/driver/input/keyboard.rs:44` | `KB_CMD_SCANCODE` | 无扫描码集切换代码 |
| D-09 | `framework/driver/input/keyboard.rs:46` | `KB_CMD_IDENTIFY` | 无键盘识别代码 |
| D-10 | `framework/driver/char/serial.rs:60` | `LSR_TRANSMIT_IDLE` | `LSR_TRANSMIT_EMPTY` 已满足需求 |
| D-11 | `framework/driver/char/vga.rs:50` | `VGA_DATA_REGISTER` | 无 index+data 寄存器对使用 |
| D-12 | `framework/driver/display/font.rs:8` | `GLYPH_BYTES` | 与 `GLYPH_HEIGHT` 值重复 |
| D-13 | `framework/arch/x86_64/ioapic.rs:12` | `IOAPIC_ID` | 无多 IOAPIC 支持 |
| D-14 | `framework/arch/x86_64/ioapic.rs:15` | `IOAPIC_ARB` | 无 IOAPIC 仲裁 |
| D-15 | `framework/arch/x86_64/ioapic.rs:21` | `REDTBL_LOW_PRIORITY` | 高级路由模式，未使用 |
| D-16 | `framework/arch/x86_64/ioapic.rs:23` | `REDTBL_LOGICAL` | 高级路由模式，未使用 |
| D-17 | `framework/arch/x86_64/ioapic.rs:28` | `DELIVERY_SMI` | 无 SMI 路由 |
| D-18 | `framework/arch/x86_64/ioapic.rs:30` | `DELIVERY_NMI` | 无 NMI 路由 |
| D-19 | `framework/arch/x86_64/ioapic.rs:32` | `DELIVERY_EXTINT` | 无 ExtINT 路由 |
| D-20 | `framework/arch/x86_64/apic.rs:17,19,21` | `APIC_ISR_BASE`, `APIC_TMR_BASE`, `APIC_IRR_BASE` | 无 ISR/TMR/IRR 内省 |
| D-21 | `framework/timer/pit.rs:32,34` | `PIT_CHANNEL_1_DATA`, `PIT_CHANNEL_2_DATA` | DRAM 刷新/PC 扬声器，无用例 |
| D-22 | `framework/pci/mod.rs:85` | `REG_CLASS_CODE` | 无子类查询代码 |
| D-23 | `services/syscall/mod.rs:208` | `USER_ADDR_MAX` | 与 framework 副本冗余 |
| D-24 | `eash/.../fileops.rs:128` | `sync` 函数 | 未注册到命令 TABLE，不可达 |
| D-25 | `userland/` 整个 crate | 全部 4 个文件 | 孤立原型，已被 `userlib` 替代 |
| D-26 | `framework/net/init.rs` | `IPV4_NONE` | 零引用 |
| D-27 | `framework/net/init.rs` | `MAC_NONE` | 零引用 |
| D-29 | `framework/net/init.rs` | `qx_socket_register_syscalls` | 空 stub，零调用者 |
| D-30 | `framework/net/init.rs` | `E_PERM`, `E_NOENT`, `E_INTR`, `E_IO`, `E_ADDRNOTAVAIL` | 5 个 POSIX errno 常量未使用 |
| D-32 | `framework/net/init.rs` | `sm_alloc_fd` | 已被 `fd_alloc` 替代，过时残留 |
| D-34 | `framework/net/iface_trait.rs` | `Ipv4Addr::BROADCAST` | 零引用 |
| D-35 | `framework/syscall/mod.rs` | `SIGTERM`, `SIGKILL` | 仅在注释中出现 |
| D-36 | `framework/syscall/mod.rs` | `SIG_DFL_SYSCALL`, `SIG_IGN_SYSCALL` | 零引用 |
| D-37 | `framework/syscall/mod.rs` | `write_le16` | 零调用者 |

### B. 激活实现 (19 项) — 📋 实施计划见 [dead-code-activation.md](./dead-code-activation.md)

预留/扩展代码，通过实现对应功能来消除 dead_code。

| # | 位置 | 项 | 激活方式 |
|---|------|----|---------|
| A-01 | `framework/pci/msi.rs` | MSI/MSI-X 全模块 | 接入 virtio-net / NVMe 驱动 |
| A-02 | `framework/ioport.rs` | IoPort 安全抽象 | 驱动层迁移到 IoPort 替代 raw inb/outb |
| A-03 | `framework/driver/virtio/blk.rs:118` | `BLK_CONFIG_CAPACITY_HI` | virtio-blk 容量读取支持 >2TB |
| A-04 | `framework/driver/display/mod.rs:117` | `VBE_DISPI_MMIO_BASE` | 实现 Bochs VBE MMIO 模式 |
| A-05 | `framework/mm/kpti.rs:52` | `INVPCID_TYPE_SINGLE` | 实现 VMM COW/mprotect 细粒度 TLB 刷新 |
| A-06 | `framework/dma/engine.rs:442` | `cache_invalidate()` | 流式 DMA 读取路径接入 |
| A-07 | `framework/sync/lockdep.rs:417` | `any_in_irq()` | lockdep 中断安全检测接入 |
| A-08 | `framework/driver/net/e1000.rs:52,54,56` | EEPROM 常量 | e1000-real-hw feature 下已使用，仅移除 allow |
| A-09 | `framework/driver/net/e1000.rs:194` | `eeprom_read()` QEMU stub | QEMU 兼容路径已使用，仅移除 allow |
| A-10 | `framework/driver/usb/xhci.rs:170,173` | `PORT_ENABLED`, `PORT_POWER` | 测试已引用，仅移除 allow |
| A-11 | `user/fbterm/src/main.rs:93` | `clear_line()` | 接入行编辑/状态栏逻辑 |
| A-12 | `eash/.../pipeline.rs:19` | `StdinFile` variant | 修复 parser 设置 `redir_kind` |
| A-13 | `framework/driver/storage/ata.rs:57` | `ATA_CTRL_ALT_STATUS` | ATA 复位/轮询路径接入 |
| A-14 | `framework/net/init.rs` | 8 项删除 + 4 项评估激活 | 见 Task 11b 详细分析 |
| A-14a | `framework/net/init.rs` | `set_max_sockets` | 接入 sysctl/procfs 调整路径 |
| A-14b | `framework/net/init.rs` | `listen_endpoint_to_smol` | 修改 `sm_listen` 使用此翻译函数 |
| A-14c | `framework/net/init.rs` | `raw::sockets_remove_helper` | 实装 W4.2.2 socket 关闭路径 |
| A-15 | `framework/net/iface_trait.rs` | 文件级 allow 收窄 | 仅默认方法体保留 allow |
| A-16 | `framework/syscall/mod.rs` | 文件级 allow 收窄 | 识别具体未使用项，逐项标注 |

### C. 文件级抑制消除 (6 个文件级 allow → 0) — ✅ 已完成 (2026-07-18)

| # | 位置 | 死代码项数 | 处置方式 |
|---|------|-----------|---------|
| N-01 | `framework/syscall/futex.rs` | **0** | 直接移除 allow (所有项均被引用) |
| N-02 | `framework/fs/initramfs.rs` | **0** | 直接移除 allow (所有项均被引用) |
| N-03 | `framework/debug/mod.rs` | **0** | 直接移除 allow (所有 re-export 均被引用) |
| A-14 | `framework/net/init.rs` | **12** (8 删除 + 4 评估) | 见 Task 11b 详细分析 |
| A-15 | `framework/net/iface_trait.rs` | **1** | 删除 1 项死代码 + 移除 allow |
| A-16 | `framework/syscall/mod.rs` | **5** | 删除 5 项死代码 + 移除 allow |
| N-04 | `queenx-tests/src/lib.rs` | 待评估 | Task 13 处理 |
| N-05 | `user/eash/.../pipeline.rs` | 0 | A-12 处理后移除 |

---

## Task 1: 删除冗余硬件常量 (ATA/键盘/串口/VGA/字体)

**覆盖:** D-02 ~ D-12

**Files:**
- Modify: `src/kernel/framework/driver/storage/ata.rs` (移除 4 项)
- Modify: `src/kernel/framework/driver/input/keyboard.rs` (移除 4 项)
- Modify: `src/kernel/framework/driver/char/serial.rs` (移除 1 项)
- Modify: `src/kernel/framework/driver/char/vga.rs` (移除 1 项)
- Modify: `src/kernel/framework/driver/display/font.rs` (移除 1 项)

**Interfaces:**
- Consumes: 无
- Produces: 11 处 dead_code 标记消除

**Steps:**

- [ ] **Step 1: 删除 ATA 冗余常量**

从 `src/kernel/framework/driver/storage/ata.rs` 中删除以下 4 项及其 `#[allow(dead_code)]` 注释:
- `ATA_STATUS_DSC` (line ~64)
- `ATA_STATUS_CORR` (line ~67)
- `ATA_STATUS_IDX` (line ~69)
- `ATA_TIMEOUT_ERR` (line ~85)

保留 `ATA_CTRL_ALT_STATUS` (line ~57)，该常量有明确激活路径。

- [ ] **Step 2: 删除 PS/2 键盘冗余常量**

从 `src/kernel/framework/driver/input/keyboard.rs` 中删除以下 4 项及其 `#[allow(dead_code)]` 注释:
- `PS2_STATUS_SYSTEM` (line ~37)
- `KB_CMD_ECHO` (line ~42)
- `KB_CMD_SCANCODE` (line ~44)
- `KB_CMD_IDENTIFY` (line ~46)

- [ ] **Step 3: 删除串口/VGA/字体冗余常量**

从以下文件各删除 1 项:
- `src/kernel/framework/driver/char/serial.rs`: 删除 `LSR_TRANSMIT_IDLE` (line ~60)
- `src/kernel/framework/driver/char/vga.rs`: 删除 `VGA_DATA_REGISTER` (line ~50)
- `src/kernel/framework/driver/display/font.rs`: 删除 `GLYPH_BYTES` (line ~8)

- [ ] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: 0 error / 0 warning

- [ ] **Step 5: Commit**

```bash
git add src/kernel/framework/driver/storage/ata.rs \
        src/kernel/framework/driver/input/keyboard.rs \
        src/kernel/framework/driver/char/serial.rs \
        src/kernel/framework/driver/char/vga.rs \
        src/kernel/framework/driver/display/font.rs
git commit -m "refactor(driver): 删除 11 处冗余硬件规范常量 (D-02~D-12)"
```

---

## Task 2: 删除 IOAPIC/APIC/PIT/PCI 冗余常量

**覆盖:** D-13 ~ D-22

**Files:**
- Modify: `src/kernel/framework/arch/x86_64/ioapic.rs` (移除 7 项)
- Modify: `src/kernel/framework/arch/x86_64/apic.rs` (移除 3 项)
- Modify: `src/kernel/framework/timer/pit.rs` (移除 2 项)
- Modify: `src/kernel/framework/pci/mod.rs` (移除 1 项)

**Interfaces:**
- Consumes: 无
- Produces: 13 处 dead_code 标记消除

**Steps:**

- [ ] **Step 1: 删除 IOAPIC 冗余常量**

从 `src/kernel/framework/arch/x86_64/ioapic.rs` 中删除以下 7 项:
- `IOAPIC_ID` (line ~12)
- `IOAPIC_ARB` (line ~15)
- `REDTBL_LOW_PRIORITY` (line ~21)
- `REDTBL_LOGICAL` (line ~23)
- `DELIVERY_SMI` (line ~28)
- `DELIVERY_NMI` (line ~30)
- `DELIVERY_EXTINT` (line ~32)

- [ ] **Step 2: 删除 Local APIC 冗余常量**

从 `src/kernel/framework/arch/x86_64/apic.rs` 中删除以下 3 项:
- `APIC_ISR_BASE` (line ~17)
- `APIC_TMR_BASE` (line ~19)
- `APIC_IRR_BASE` (line ~21)

- [ ] **Step 3: 删除 PIT/PCI 冗余常量**

从以下文件各删除:
- `src/kernel/framework/timer/pit.rs`: 删除 `PIT_CHANNEL_1_DATA` (line ~32) 和 `PIT_CHANNEL_2_DATA` (line ~34)
- `src/kernel/framework/pci/mod.rs`: 删除 `REG_CLASS_CODE` (line ~85)

- [ ] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: 0 error / 0 warning

- [ ] **Step 5: Commit**

```bash
git add src/kernel/framework/arch/x86_64/ioapic.rs \
        src/kernel/framework/arch/x86_64/apic.rs \
        src/kernel/framework/timer/pit.rs \
        src/kernel/framework/pci/mod.rs
git commit -m "refactor(arch): 删除 13 处冗余架构/定时器/PCI 常量 (D-13~D-22)"
```

---

## Task 3: 删除 services/userland/eash 孤立死代码

**覆盖:** D-23 ~ D-25

**Files:**
- Modify: `src/kernel/services/syscall/mod.rs` (移除 `USER_ADDR_MAX` 及其测试)
- Delete: `src/userland/` 整个目录
- Modify: `src/user/eash/src/commands/fileops.rs` (移除 `sync` 函数)

**Interfaces:**
- Consumes: 无
- Produces: 3 处 dead_code 标记消除 + 移除孤立 crate

**Steps:**

- [ ] **Step 1: 确认 userland crate 确实无依赖者**

```bash
grep -r "queenx_userland\|userland" src/user/Cargo.toml src/user/*/Cargo.toml src/rust/Cargo.toml 2>/dev/null
```

预期: 无匹配。确认后执行删除。

- [ ] **Step 2: 删除 userland crate**

```bash
rm -rf src/userland/
```

该 crate 不在任何 workspace 中，无任何依赖者。已被 `src/user/lib/` (userlib) 完全替代。

- [ ] **Step 3: 删除 services/syscall 中的冗余 USER_ADDR_MAX**

从 `src/kernel/services/syscall/mod.rs` 中:
- 删除 `USER_ADDR_MAX` 常量定义 (line ~208) 及其 `#[allow(dead_code)]` 注释
- 更新引用该常量的测试 (lines ~337, ~347)，改用 framework 层的 `USER_ADDR_MAX` 或直接使用字面值

- [ ] **Step 4: 删除 eash 中未注册的 sync 函数**

从 `src/user/eash/src/commands/fileops.rs` 中删除 `sync` 函数 (line ~128) 及其 `#[allow(dead_code)]` 注释。该函数未注册到命令 TABLE，不可达。

- [ ] **Step 5: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: 0 error / 0 warning

- [ ] **Step 6: host-tests 验证**

```bash
make test-host
```

预期: 全部通过

- [ ] **Step 7: Commit**

```bash
git add src/kernel/services/syscall/mod.rs \
        src/user/eash/src/commands/fileops.rs
git rm -r src/userland/
git commit -m "refactor: 删除孤立 userland crate + services/eash 死代码 (D-23~D-25)"
```

---

## Task 4: 激活 MSI/MSI-X 子系统 (A-01)

**覆盖:** A-01

**Files:**
- Modify: `src/kernel/framework/pci/msi.rs` (移除 `#![allow(dead_code)]`，添加 pub re-export)
- Modify: `src/kernel/framework/pci/mod.rs` (pub use msi 模块)
- Modify: `src/kernel/framework/driver/virtio/net.rs` (接入 MSI 分配)
- Modify: `src/kernel/framework/driver/storage/nvme.rs` (接入 MSI-X 分配)

**Interfaces:**
- Consumes: `pci::PciDevice`, `msi_alloc_vector()`, `msi_enable()`
- Produces: 驱动通过 MSI/MSI-X 接收中断

**Steps:**

- [ ] **Step 1: 读取 msi.rs 确认公开 API**

确认 `msi_alloc_vector()`, `msi_free_vector()`, `msi_enable()`, `msix_enable()` 的签名和安全要求。

- [ ] **Step 2: 在 pci/mod.rs 中 re-export msi 模块**

```rust
pub mod msi;
```

确保 msi 模块通过 `framework::pci::msi` 可达。

- [ ] **Step 3: 移除 msi.rs 的文件级 allow**

删除 `#![allow(dead_code)]` (line 50)，改为对确实未使用的项逐项标注。

- [ ] **Step 4: 在 virtio-net 驱动中接入 MSI**

在 virtio-net 初始化路径中调用 `msi_alloc_vector()` + `msi_enable()` 分配 MSI 中断向量。

- [ ] **Step 5: 在 NVMe 驱动中接入 MSI-X**

在 NVMe 初始化路径中调用 `msi_alloc_vector()` + `msix_enable()` 分配 MSI-X 中断向量。

- [ ] **Step 6: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: 0 error / 0 warning

- [ ] **Step 7: Commit**

```bash
git add src/kernel/framework/pci/msi.rs \
        src/kernel/framework/pci/mod.rs \
        src/kernel/framework/driver/virtio/net.rs \
        src/kernel/framework/driver/storage/nvme.rs
git commit -m "feat(pci): 激活 MSI/MSI-X 子系统，接入 virtio-net 和 NVMe 驱动 (A-01)"
```

---

## Task 5: 激活 IoPort 安全抽象 (A-02)

**覆盖:** A-02

**Files:**
- Modify: `src/kernel/framework/ioport.rs` (移除文件级 `#![allow(dead_code)]`)
- Modify: 各驱动文件 (迁移 raw inb/outb 到 IoPort)

**Interfaces:**
- Consumes: `IoPort::new()`, `IoPort::read_u8/u16/u32()`, `IoPort::write_u8/u16/u32()`
- Produces: 驱动通过安全代理访问 PIO 端口

**Steps:**

- [ ] **Step 1: 识别使用 raw inb/outb 的驱动文件**

```bash
grep -rn "unsafe.*\b\(inb\|outb\|inw\|outw\|inl\|outl\)\b" src/kernel/framework/driver/
```

列出所有使用原始 PIO 指令的驱动文件。

- [ ] **Step 2: 逐步迁移驱动到 IoPort**

对每个驱动文件:
1. 将 `inb(port)` 替换为 `ioport.read_u8(offset)`
2. 将 `outb(port, val)` 替换为 `ioport.write_u8(offset, val)`
3. 在驱动初始化时创建 `IoPort` 实例

优先迁移: serial.rs, vga.rs, ata.rs (最常用的 PIO 驱动)

- [ ] **Step 3: 移除 ioport.rs 文件级 allow**

删除 `#![allow(dead_code)]` (line 19)。

- [ ] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: 0 error / 0 warning

- [ ] **Step 5: Commit**

```bash
git add src/kernel/framework/ioport.rs src/kernel/framework/driver/
git commit -m "feat(driver): 激活 IoPort 安全抽象，迁移驱动层 PIO 访问 (A-02)"
```

---

## Task 6: 激活 virtio-blk >2TB 支持 (A-03)

**覆盖:** A-03

**Files:**
- Modify: `src/kernel/framework/driver/virtio/blk.rs` (移除 allow，读取高 32 位容量)

**Steps:**

- [ ] **Step 1: 修改容量读取逻辑**

在 virtio-blk 初始化路径中，将容量从 32 位扩展为 64 位:

```rust
// 之前: let capacity = read_config_u32(BLK_CONFIG_CAPACITY_LO) as u64;
// 之后:
let cap_lo = read_config_u32(BLK_CONFIG_CAPACITY_LO) as u64;
let cap_hi = read_config_u32(BLK_CONFIG_CAPACITY_HI) as u64;
let capacity = cap_lo | (cap_hi << 32);
```

- [ ] **Step 2: 移除 `#[allow(dead_code)]`**

删除 `BLK_CONFIG_CAPACITY_HI` 的 `#[allow(dead_code)]` 注释 (line ~118)。

- [ ] **Step 3: 双架构编译验证**

```bash
./ci/build.sh all
```

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/driver/virtio/blk.rs
git commit -m "feat(virtio-blk): 激活 >2TB 块设备容量支持 (A-03)"
```

---

## Task 7: 激活 KPTI 单 PCID TLB 刷新 (A-05)

**覆盖:** A-05

**Files:**
- Modify: `src/kernel/framework/mm/kpti.rs` (移除 allow，添加 flush_single_pcid)

**Steps:**

- [ ] **Step 1: 实现单 PCID TLB 刷新函数**

```rust
/// 按 PCID 刷新单条 TLB 条目 (COW/mprotect 细粒度刷新)
pub fn invpcid_flush_single(pcid: u16, vaddr: u64) {
    // SAFETY: INVPCID type 0 (by individual address + PCID)
    // 前提: pcid 有效 (0-4095), vaddr 页对齐
    // 调用方保证: vaddr 属于调用方地址空间
    // 硬件契约: INVPCID 指令在支持的 CPU 上原子刷新单条 TLB
    unsafe {
        core::arch::asm!(
            "invpcid {rsp}, [{addr}]",
            rsp = inout(reg) 0u64,
            addr = in(reg) &InvpcidDescriptor { pcid: pcid as u64, vaddr },
            options(nostack)
        );
    }
}
```

- [ ] **Step 2: 移除 `#[allow(dead_code)]`**

删除 `INVPCID_TYPE_SINGLE` 的 `#[allow(dead_code)]` 注释 (line ~52)。

- [ ] **Step 3: 在 VMM COW 路径中接入**

在 page fault handler 的 COW 分裂路径中，调用 `invpcid_flush_single()` 替代全量刷新。

- [ ] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

- [ ] **Step 5: Commit**

```bash
git add src/kernel/framework/mm/kpti.rs src/kernel/framework/mm/
git commit -m "feat(mm): 激活 INVPCID 单 PCID 刷新，支持 COW 细粒度 TLB 失效 (A-05)"
```

---

## Task 8: 激活 DMA cache_invalidate + lockdep any_in_irq (A-06, A-07)

**覆盖:** A-06, A-07

**Files:**
- Modify: `src/kernel/framework/dma/engine.rs` (移除 allow，接入读取路径)
- Modify: `src/kernel/framework/sync/lockdep.rs` (移除 allow，接入检测路径)

**Steps:**

- [ ] **Step 1: 在 DMA 读取路径接入 cache_invalidate**

在 `DmaStream` 的读取准备阶段调用 `cache_invalidate()`:
```rust
// 在 DMA 读取前调用
self.cache_invalidate(self.dma_addr, self.size);
```

删除 `#[allow(dead_code)]` (line ~442)。

- [ ] **Step 2: 在 lockdep 检测路径接入 any_in_irq**

在 `lockdep_check()` 或等效入口调用 `any_in_irq()`:
```rust
if lock_graph.any_in_irq() {
    klog_warn!("潜在死锁: 当前线程持有 IRQ 上下文获取的锁");
}
```

删除 `#[allow(dead_code)]` (line ~417)。

- [ ] **Step 3: 双架构编译验证**

```bash
./ci/build.sh all
```

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/dma/engine.rs src/kernel/framework/sync/lockdep.rs
git commit -m "feat: 激活 DMA cache_invalidate 和 lockdep any_in_irq 检测 (A-06, A-07)"
```

---

## Task 9: 激活 e1000 EEPROM 常量 + xHCI 端口常量 (A-08~A-10)

**覆盖:** A-08, A-09, A-10

**Files:**
- Modify: `src/kernel/framework/driver/net/e1000.rs` (移除 4 处 allow)
- Modify: `src/kernel/framework/driver/usb/xhci.rs` (移除 2 处 allow)

**说明:** 这些项已被代码引用 (feature-gated 或测试)，仅需移除 `#[allow(dead_code)]` 标记。

**Steps:**

- [ ] **Step 1: 移除 e1000.rs 的 allow 标记**

删除以下 4 处 `#[allow(dead_code)]` 注释 (保留常量定义本身):
- `E1000_EERD` (line ~52)
- `E1000_EERD_START` (line ~54)
- `E1000_EERD_DONE` (line ~56)
- `eeprom_read()` QEMU stub (line ~194)

- [ ] **Step 2: 移除 xhci.rs 的 allow 标记**

删除以下 2 处 `#[allow(dead_code)]` 注释:
- `PORT_ENABLED` (line ~170)
- `PORT_POWER` (line ~173)

- [ ] **Step 3: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: 这些项在各自 feature/test 路径下被引用，移除 allow 后应无 dead_code 警告。若仍有警告，需确认 feature gate 配置正确。

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/driver/net/e1000.rs \
        src/kernel/framework/driver/usb/xhci.rs
git commit -m "refactor(driver): 移除 e1000/xhci 已激活项的 dead_code 允许 (A-08~A-10)"
```

---

## Task 10: 激活 fbterm clear_line + eash StdinFile (A-11, A-12)

**覆盖:** A-11, A-12

**Files:**
- Modify: `src/user/fbterm/src/main.rs` (移除 allow，接入 clear_line)
- Modify: `src/user/eash/src/commands/pipeline.rs` (修复 parser 设置 StdinFile)

**Steps:**

- [ ] **Step 1: 在 fbterm 中接入 clear_line**

在行编辑或屏幕刷新路径中调用 `clear_line(row)`:
```rust
// 在输入行重绘时
self.clear_line(cursor_row);
```

删除 `#[allow(dead_code)]` (line ~93)。

- [ ] **Step 2: 修复 eash parser 设置 StdinFile**

在 `pipeline.rs` 的 `<` 重定向解析路径 (line ~121) 中添加:
```rust
seg.redir_kind = RedirKind::StdinFile;
```

删除 `StdinFile` variant 的 `#[allow(dead_code)]` (line ~19)。

- [ ] **Step 3: 编译验证**

```bash
cargo build --release  # 用户态程序
```

- [ ] **Step 4: Commit**

```bash
git add src/user/fbterm/src/main.rs src/user/eash/src/commands/pipeline.rs
git commit -m "feat(user): 激活 fbterm clear_line 和 eash StdinFile 重定向 (A-11, A-12)"
```

---

## Task 10b: 激活 Bochs VBE MMIO 模式 (A-04)

**覆盖:** A-04

**Files:**
- Modify: `src/kernel/framework/driver/display/mod.rs` (实现 MMIO 访问路径，移除 allow)

**背景:** 当前 `read_bochs_disp_mode()` 通过 port I/O (0x01CE/0x01CF) 读取 Bochs DISPI 寄存器。`VBE_DISPI_MMIO_BASE` (0x500) 是 BAR0 上的 MMIO 偏移，可替代 port I/O 避免端口访问开销。MMIO 模式在 QEMU/Bochs 中支持，且与 `probe_vga_fb_via_pci()` 的 BAR0 地址配合使用。

**Steps:**

- [ ] **Step 1: 实现 MMIO 读取路径**

在 `display/mod.rs` 中添加 MMIO 模式的寄存器读取函数:

```rust
/// 通过 MMIO 读取 Bochs DISPI 寄存器 (替代 port I/O)
///
/// # Safety
/// - `mmio_base` 必须是有效的 VGA BAR0 映射地址
/// - `offset` 必须在 BAR0 范围内 (< bar0.size)
#[cfg(target_arch = "x86_64")]
unsafe fn read_bochs_disp_mode_mmio(mmio_base: u64) -> Option<(u32, u32, u8)> {
    // SAFETY: 调用方保证 mmio_base 是有效的 VGA BAR0 映射,
    // offset 在 BAR0 范围内, volatile 访问对 MMIO 寄存器是必需的.
    unsafe {
        let base = mmio_base + VBE_DISPI_MMIO_BASE;
        let id = core::ptr::read_volatile((base + VBE_DISPI_INDEX_ID as u64 * 2) as *const u16);
        if id < VBE_DISPI_ID5 {
            return None;
        }
        let enabled = core::ptr::read_volatile((base + VBE_DISPI_INDEX_ENABLE as u64 * 2) as *const u16);
        if enabled == 0 {
            return None;
        }
        let xres = core::ptr::read_volatile((base + VBE_DISPI_INDEX_XRES as u64 * 2) as *const u16) as u32;
        let yres = core::ptr::read_volatile((base + VBE_DISPI_INDEX_YRES as u64 * 2) as *const u16) as u32;
        let bpp = core::ptr::read_volatile((base + VBE_DISPI_INDEX_BPP as u64 * 2) as *const u16) as u8;
        if xres == 0 || yres == 0 || bpp == 0 {
            return None;
        }
        Some((xres, yres, bpp))
    }
}
```

- [ ] **Step 2: 在 probe_vga_fb_via_pci 中优先使用 MMIO**

修改 `probe_vga_fb_via_pci()` (line ~183)，在获取 BAR0 地址后优先尝试 MMIO 路径:

```rust
// 优先 MMIO 路径 (避免 port I/O 开销)
let mode = if bar0.base_addr != 0 {
    // SAFETY: bar0.base_addr 是 PCI BAR0 物理地址, 已通过 PCI 枚举验证
    unsafe { read_bochs_disp_mode_mmio(bar0.base_addr) }
} else {
    None
};
let (width, height, bpp) = mode
    .or_else(|| read_bochs_disp_mode())  // 回退到 port I/O
    .unwrap_or((1024, 768, 32));         // 最终回退默认值
```

- [ ] **Step 3: 移除 `#[allow(dead_code)]`**

删除 `VBE_DISPI_MMIO_BASE` 的 `#[allow(dead_code)]` 注释 (line ~117)，因为 MMIO 路径现已使用该常量。

- [ ] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: 0 error / 0 warning (aarch64 下 `#[cfg(target_arch = "x86_64")]` 门控，不影响)

- [ ] **Step 5: Commit**

```bash
git add src/kernel/framework/driver/display/mod.rs
git commit -m "feat(display): 激活 Bochs VBE MMIO 模式，优先 MMIO 替代 port I/O (A-04)"
```

---

## Task 11a: 移除零死代码文件的文件级 allow (N-01~N-03)

**覆盖:** N-01, N-02, N-03

**Files:**
- Modify: `src/kernel/framework/syscall/futex.rs` (移除文件级 allow)
- Modify: `src/kernel/framework/fs/initramfs.rs` (移除文件级 allow)
- Modify: `src/kernel/framework/debug/mod.rs` (移除文件级 allow)

**说明:** 经逐项 grep 验证，这 3 个文件内所有项均被引用，无真正死代码。`#![allow(dead_code)]` 是历史遗留，直接删除即可。

| 文件 | 所有项均被引用的证据 |
|------|---------------------|
| `syscall/futex.rs` | `sys_futex()` 被 `services/sync/futex.rs:127` 调用；`register_futex_tests` 被 `framework/tests/mod.rs:391` 调用；所有内部类型 (`FutexWaiter`, `FutexBucket`, `FutexHashTable`, `SimpleSpinLock`) 均被 `sys_futex()` 使用 |
| `fs/initramfs.rs` | `unpack()` 被 `framework/fs/mod.rs:19` re-export 并在 boot 时调用；`register_initramfs_tests` 被 `framework/tests/mod.rs:390` 调用；所有内部 helper (`CpioEntry`, `parse_hex_field`, `align4`, `parse_next_entry`) 均被 `unpack()` 使用 |
| `debug/mod.rs` | `sys_bpf` 被 `syscall/mod.rs:381` 调用；`bpf_init` 被 `proc/api.rs:892` 调用；所有 `pub use` re-export 被 `services/debug/mod.rs` 使用 |

**Steps:**

- [ ] **Step 1: 删除 futex.rs 文件级 allow**

删除 `src/kernel/framework/syscall/futex.rs` 第 25 行的 `#![allow(dead_code)]`。不改动其他代码。

- [ ] **Step 2: 删除 initramfs.rs 文件级 allow**

删除 `src/kernel/framework/fs/initramfs.rs` 第 41 行的 `#![allow(dead_code)]`。不改动其他代码。

- [ ] **Step 3: 删除 debug/mod.rs 文件级 allow**

删除 `src/kernel/framework/debug/mod.rs` 第 21 行的 `#![allow(dead_code)]`。不改动其他代码。

- [ ] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: 0 error / 0 warning (所有项均有调用者，不会产生 dead_code 警告)

- [ ] **Step 5: Commit**

```bash
git add src/kernel/framework/syscall/futex.rs \
        src/kernel/framework/fs/initramfs.rs \
        src/kernel/framework/debug/mod.rs
git commit -m "refactor: 移除 3 个零死代码文件的文件级 allow (N-01~N-03)"
```

---

## Task 11b: net/init.rs 死代码消除 — 8 项删除 + 4 项评估 (A-14)

**覆盖:** A-14

**Files:**
- Modify: `src/kernel/framework/net/init.rs` (删除纯死代码 + 评估预留代码)

**死代码性质分析 (经逐项 grep 全代码库 + 源码上下文确认):**

### 纯死代码 — 直接删除 (8 项)

| # | 项 | 类型 | 代码 | 理由 |
|---|-----|------|------|------|
| 1 | `IPV4_NONE` | const | `const IPV4_NONE: [u8; 4] = [0; 4];` | 零值常量，网络栈用 `AtomicU32::new(0)` 直接表示"无"，不需要此常量 |
| 2 | `MAC_NONE` | const | `const MAC_NONE: [u8; 6] = [0; 6];` | 同上 |
| 3 | `qx_socket_register_syscalls` | pub extern "C" fn | `fn ... -> i32 { 0 }` | **空 stub**，函数体仅 `return 0`，零调用者 |
| 4 | `E_PERM` | const | `const E_PERM: i32 = 1;` | errno 常量，零引用 |
| 5 | `E_NOENT` | const | `const E_NOENT: i32 = 2;` | 同上 |
| 6 | `E_INTR` | const | `const E_INTR: i32 = 4;` | 同上 |
| 7 | `E_IO` | const | `const E_IO: i32 = 5;` | 同上 |
| 8 | `E_ADDRNOTAVAIL` | const | `const E_ADDRNOTAVAIL: i32 = 99;` | 同上 |

### 预留未接线 — 有设计意图但未接入调用 (4 项)

| # | 项 | 类型 | 代码摘要 | 理由 | 处置 |
|---|-----|------|---------|------|------|
| 9 | `set_max_sockets` | pub fn | 运行时调整 socket 上限 | 注释写"运行时可通过 `set_max_sockets` 调整"，但 sysctl/procfs 路径未接入调用。属于"实现好了但没接线" | **激活**: 接入 sysctl 或 procfs 调整路径 |
| 10 | `listen_endpoint_to_smol` | pub(crate) fn | `NetListenEndpoint → IpListenEndpoint` 翻译 | `sm_bind`/`sm_connect` 用了 `endpoint_to_smol`，但 `sm_listen` 内部直接构造 `IpListenEndpoint` 而未调用此函数。属于"写好了但 `sm_listen` 没用上" | **激活**: 修改 `sm_listen` 使用此翻译函数 |
| 11 | `sm_alloc_fd` | unsafe fn | FD 分配，遍历 SOCKET_TABLE | 注释写"TD-02 V3: 通过 `fd_alloc` 集中计算 FD 编号"。新的 `fd_alloc` 路径已取代此函数 | **删除**: 已被 `fd_alloc` 替代，属于过时残留 |
| 12 | `raw::sockets_remove_helper` | fn | `fn ... -> bool { false }` | 注释写"W4.2 阶段 1: 0 逻辑, 返回 false (未实现)"。socket 关闭路径的占位 stub | **激活**: 实装 W4.2.2 socket 关闭路径 (`sockets.remove(handle)`) |

**Steps:**

- [ ] **Step 1: 删除 8 项纯死代码**

从 `src/kernel/framework/net/init.rs` 中逐项删除:
- `IPV4_NONE` 常量定义 (line ~49)
- `MAC_NONE` 常量定义 (line ~50)
- `qx_socket_register_syscalls` 函数 (line ~874)
- `E_PERM`, `E_NOENT`, `E_INTR`, `E_IO`, `E_ADDRNOTAVAIL` 常量 (lines ~883-896)
- `sm_alloc_fd` 函数 (line ~1856) — 已被 `fd_alloc` 替代

同时删除对应的注释和 `#[allow(dead_code)]`。

- [ ] **Step 2: 评估 set_max_sockets 激活可行性**

读取 `sm_listen` (line ~1085 附近) 确认:
- `sm_listen` 是否直接构造 `IpListenEndpoint` 而绕过 `listen_endpoint_to_smol`?
- 若是，修改 `sm_listen` 调用 `listen_endpoint_to_smol` 并删除 `set_max_sockets` 的 `#[allow(dead_code)]`
- 同时评估 `set_max_sockets` 是否应接入 sysctl/procfs (若成本低则激活，否则降级为 DELETE)

- [ ] **Step 3: 评估 sockets_remove_helper 激活可行性**

读取 `sm_close` (line ~1280 附近) 确认:
- 当前 socket 关闭路径是否绕过了 `sockets_remove_helper`?
- 若是，在 `sm_close` 中接入 `sockets_remove_helper`，将 stub 替换为 `sockets.remove(handle)` 实装

- [ ] **Step 4: 移除文件级 allow**

删除 `#![allow(dead_code)]` (line 4)。对仍被 feature-gated 的项保留项级 `#[allow(dead_code)]`。

- [ ] **Step 5: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: 0 error / 0 warning

- [ ] **Step 6: Commit**

```bash
git add src/kernel/framework/net/init.rs
git commit -m "refactor(net): 删除 init.rs 中 8 项纯死代码 + 评估 4 项预留代码 (A-14)"
```

---

## Task 11c: 删除 iface_trait.rs + syscall/mod.rs 中 6 项死代码 (A-15, A-16)

**覆盖:** A-15, A-16

**Files:**
- Modify: `src/kernel/framework/net/iface_trait.rs` (删除 1 项 + 移除文件级 allow)
- Modify: `src/kernel/framework/syscall/mod.rs` (删除 5 项 + 移除文件级 allow)

**死代码清单 (经逐项 grep 全代码库确认):**

### iface_trait.rs — 1 项

| # | 项 | 类型 | 理由 |
|---|-----|------|------|
| 1 | `Ipv4Addr::BROADCAST` | const (line ~615) | 零引用 (含测试) |

### syscall/mod.rs — 5 项

| # | 项 | 类型 | 理由 |
|---|-----|------|------|
| 1 | `SIGTERM` | const (line ~865) | 仅出现在注释中，从未作为值使用 |
| 2 | `SIGKILL` | const (line ~866) | 仅出现在注释中，从未作为值使用 |
| 3 | `SIG_DFL_SYSCALL` | const (line ~890) | 零引用 |
| 4 | `SIG_IGN_SYSCALL` | const (line ~891) | 零引用 |
| 5 | `write_le16` | fn (line ~729) | 零调用者 |

**Steps:**

- [ ] **Step 1: 删除 iface_trait.rs 死代码**

从 `src/kernel/framework/net/iface_trait.rs` 中删除:
- `Ipv4Addr::BROADCAST` 常量 (line ~615)

删除文件级 `#![allow(dead_code)]` (line 44)。

- [ ] **Step 2: 删除 syscall/mod.rs 死代码**

从 `src/kernel/framework/syscall/mod.rs` 中删除:
- `SIGTERM` 常量 (line ~865)
- `SIGKILL` 常量 (line ~866)
- `SIG_DFL_SYSCALL` 常量 (line ~890)
- `SIG_IGN_SYSCALL` 常量 (line ~891)
- `write_le16` 函数 (line ~729)

删除文件级 `#![allow(dead_code)]` (line 7)。

- [ ] **Step 3: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: 0 error / 0 warning

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/net/iface_trait.rs \
        src/kernel/framework/syscall/mod.rs
git commit -m "refactor: 删除 iface_trait + syscall 中 6 项死代码，移除文件级 allow (A-15, A-16)"
```

---

## Task 12: ATA_CTRL_ALT_STATUS 激活评估 (A-13)

**覆盖:** A-13

**Files:**
- Modify: `src/kernel/framework/driver/storage/ata.rs` (接入 ATA 复位路径)

**说明:** `ATA_CTRL_ALT_STATUS` 是 ATA 备用状态寄存器偏移 (0x00，与基址相同)。ATA 标准要求通过备用控制寄存器执行软复位。当前 ATA 驱动的复位路径可能未使用此常量。

**Steps:**

- [ ] **Step 1: 读取 ata.rs 确认复位路径**

确认当前 ATA 初始化/复位代码是否需要通过备用状态寄存器轮询。

- [ ] **Step 2: 接入复位路径 (如需要)**

若复位路径确实需要轮询备用状态寄存器，接入 `ATA_CTRL_ALT_STATUS`。否则降级为 DELETE。

- [ ] **Step 3: 双架构编译验证**

```bash
./ci/build.sh all
```

- [ ] **Step 4: Commit**

```bash
git add src/kernel/framework/driver/storage/ata.rs
git commit -m "feat(driver): 激活 ATA 备用状态寄存器 (A-13)"
```

---

## Task 13: queenx-tests dead_code 评估 (N-04)

**覆盖:** N-04

**Files:**
- Modify: `src/rust/queenx-tests/src/lib.rs`

**Steps:**

- [ ] **Step 1: 读取 queenx-tests 确认用途**

确认该 crate 的测试是否被主测试流程引用。

- [ ] **Step 2: 评估是否可移除文件级 allow**

若所有测试函数均被 `#[test]` 标注且有调用者，可移除 `#![allow(dead_code)]`。否则保留项级标注。

- [ ] **Step 3: Commit (如需要)**

---

## Task 14: 全量验证

**覆盖:** 所有任务完成后

**Steps:**

- [ ] **Step 1: 双架构编译**

```bash
./ci/build.sh all
```

预期: 0 error / 0 warning

- [ ] **Step 2: 全量审计**

```bash
ci/audit.sh full
```

预期: 全部通过

- [ ] **Step 3: host-tests**

```bash
make test-host
```

预期: 全部通过

- [ ] **Step 4: grep 确认 dead_code 消除进度**

```bash
grep -rn "#\[allow(dead_code)\]" src/kernel/framework/ src/kernel/services/ src/user/ | grep -v smoltcp | grep -v "//.*allow(dead_code)"
```

预期: 仅剩必要的项级 allow (已激活但 feature-gated 的项)。

- [ ] **Step 5: 最终 Commit (如需要)**

如有遗漏的 dead_code 标记，在此步骤补充清理。
