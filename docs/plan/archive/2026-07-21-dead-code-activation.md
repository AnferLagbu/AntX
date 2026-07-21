# A 区死代码激活实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 激活全部 19 项 A 区预留代码，通过实现对应功能消除 `#[allow(dead_code)]` 标记

**Architecture:** 每项激活均需在现有代码路径中接入预留代码，不接受空壳 stub。复杂项 (A-01 MSI, A-02 IoPort) 拆分为独立子任务。

**Tech Stack:** Rust, QueenX Framekernel, smoltcp, PCI/MSI/MSI-X spec, ATA spec

**前置文档:** [dead-code-elimination.md](./dead-code-elimination.md) — 处置决策总表 + D 区 (直接删除) 实施计划

## Global Constraints

- 双架构编译 0 warning 0 error (`./ci/build.sh all`)
- 审计全部通过 (`ci/audit.sh`)
- host-tests 全部通过 (`make test-host`)
- 中文注释强制
- framework `unsafe` 块必须配 `// SAFETY:` 注释
- **不允许空壳 stub** — 所有实现必须是完整、可工作的代码

---

## 分组

按复杂度分三组:

| 组 | 任务 | 复杂度 | 说明 |
|----|------|--------|------|
| **简单移除** | A-08~A-10, A-14c | 低 | 已被使用/已有实装，仅移除 allow |
| **中等接线** | A-03~A-07, A-11~A-13, A-14a~A-14b | 中 | 预留代码存在，需接入现有路径 |
| **复杂集成** | A-01 MSI, A-02 IoPort | 高 | 需要多文件改动、新增基础设施 |

---

## Task A-08~A-10: 移除 e1000/xhci 已激活项的 allow (简单)

**覆盖:** A-08, A-09, A-10

**说明:** 经调研确认:
- `E1000_EERD`, `E1000_EERD_START`, `E1000_EERD_DONE` 已在 `#[cfg(feature = "e1000-real-hw")]` 代码路径中使用
- `e1000.rs` QEMU stub 的 `eeprom_read()` 已被 `read_mac_address()` 调用
- `PORT_ENABLED`, `PORT_POWER` 已在 xhci 测试代码中引用

**Files:**
- Modify: `src/kernel/framework/driver/net/e1000.rs` (移除 4 处 allow)
- Modify: `src/kernel/framework/driver/usb/xhci.rs` (移除 2 处 allow)

**Steps:**

- [x] **Step 1: 移除 e1000.rs 的 4 处 `#[allow(dead_code)]`**

删除以下注释行 (保留常量定义和函数本身):
- line ~52: `E1000_EERD` 的 allow
- line ~54: `E1000_EERD_START` 的 allow
- line ~56: `E1000_EERD_DONE` 的 allow
- line ~194: `eeprom_read()` QEMU stub 的 allow

- [x] **Step 2: 移除 xhci.rs 的 2 处 `#[allow(dead_code)]`**

删除以下注释行:
- line ~170: `PORT_ENABLED` 的 allow
- line ~173: `PORT_POWER` 的 allow

- [x] **Step 3: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: 0 error (这些项在 feature/test 路径下被引用)

- [x] **Step 4: Commit**

```bash
git add src/kernel/framework/driver/net/e1000.rs src/kernel/framework/driver/usb/xhci.rs
git commit -m "refactor(driver): 移除 e1000/xhci 已激活项的 dead_code 允许 (A-08~A-10)"
```

---

## Task A-14c: 删除 sockets_remove_helper 死代码 (简单)

**覆盖:** A-14c

**说明:** 经调研确认:
- `sm_close()` (line ~1539) 已直接调用 `sockets.remove(handle)` 完成 socket 关闭
- `raw::socket_close_stub()` (line ~2453) 也已实现相同的 `sockets.remove()` 逻辑
- `sockets_remove_helper` (line ~2458) 是 W4.2 阶段 1 的空 stub，始终返回 false，从未被调用

**Files:**
- Modify: `src/kernel/framework/net/init.rs` (删除 `sockets_remove_helper`)

**Steps:**

- [x] **Step 1: 删除 `sockets_remove_helper` 函数**

从 `src/kernel/framework/net/init.rs` 的 `raw` 模块中删除:
```rust
/// smoltcp SocketSet::remove 辅助 (W4.2 阶段 1 stub).
fn sockets_remove_helper(_smol_handle: smoltcp::iface::SocketHandle) -> bool {
    false
}
```

- [x] **Step 2: 双架构编译验证**

```bash
./ci/build.sh all
```

- [x] **Step 3: Commit**

```bash
git add src/kernel/framework/net/init.rs
git commit -m "refactor(net): 删除 sockets_remove_helper 空 stub (A-14c)"
```

---

## Task A-03: 激活 virtio-blk >2TB 容量支持 (中等)

**覆盖:** A-03

**说明:** 经调研确认:
- `read_config64(BLK_CONFIG_CAPACITY_LO)` 已通过 `VirtioMmioDevice::read64()` 同时读取 low + high 32 位并组合为 u64
- 容量类型全链路为 `u64` (`capacity_sectors: u64` → `blk_total_sectors() -> u64`)
- `BLK_CONFIG_CAPACITY_HI` 常量本身未被引用，但功能已正确工作

**激活方式:** 将隐式的 +4 偏移替换为显式使用 `BLK_CONFIG_CAPACITY_HI` 常量，文档化意图。

**Files:**
- Modify: `src/kernel/framework/driver/virtio/blk.rs` (显式使用常量 + 移除 allow)
- Modify: `src/kernel/framework/driver/virtio/mod.rs` (如 read_config64 需要调整)

**Steps:**

- [x] **Step 1: 确认 read_config64 的实现**

读取 `src/kernel/framework/driver/virtio/mod.rs` 中 `read_config64` 和 `read64` 的实现，确认 high 偏移是否为 `low_off + 4`。

- [x] **Step 2: 修改 VirtioBlk 容量读取为显式双寄存器读取**

在 `blk.rs` 的 `VirtioBlk::new()` 中，将:
```rust
let capacity = device.read_config64(BLK_CONFIG_CAPACITY_LO);
```
替换为显式读取 (如 read_config64 内部已处理 high，则仅添加注释说明):
```rust
// virtio-blk 配置空间: offset 0x00 = capacity_lo, offset 0x04 = capacity_hi
// read_config64 内部组合为 u64, 支持 >2TB 容量
let capacity = device.read_config64(BLK_CONFIG_CAPACITY_LO);
```

若 `read_config64` 的 high 偏移硬编码为 `low_off + 4`，则 `BLK_CONFIG_CAPACITY_HI` 无需在运行时使用，仅需移除 allow 并添加文档注释说明与 `BLK_CONFIG_CAPACITY_LO` 的关系。

- [x] **Step 3: 移除 `#[allow(dead_code)]`**

删除 `BLK_CONFIG_CAPACITY_HI` 的 `#[allow(dead_code)]` 注释。

- [x] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

- [x] **Step 5: Commit**

```bash
git add src/kernel/framework/driver/virtio/blk.rs
git commit -m "feat(virtio-blk): 文档化 >2TB 容量支持，移除 dead_code 允许 (A-03)"
```

---

## Task A-05: 激活 INVPCID 单 PCID TLB 刷新 (中等)

**覆盖:** A-05

**说明:** 经调研确认:
- `invpcid(pcid, addr, typ)` 底层函数已存在 (line 63-77)，接受 `[u64; 2]` 描述符
- `INVPCID_TYPE_SINGLE = 0` 是 type 0 (按 PCID + 地址刷新单条 TLB)
- `INVPCID_TYPE_ALL_INCL_GLOBAL = 2` 已在 `invpcid_flush_all()` 中使用
- 无 `InvpcidDescriptor` 结构体，当前用 `[u64; 2]` 原始数组

**Files:**
- Modify: `src/kernel/framework/mm/kpti.rs` (添加 `invpcid_flush_single` 函数)

**Steps:**

- [x] **Step 1: 实现 `invpcid_flush_single` 函数**

在 `kpti.rs` 的 `invpcid_flush_all()` 附近添加:

```rust
/// 按 PCID + 虚拟地址刷新单条 TLB 条目
///
/// 用于 VMM COW/mprotect 的细粒度 TLB 失效, 避免全量刷新的性能损失.
///
/// # Safety
/// - `pcid` 必须是有效的 PCID (0-4095)
/// - `vaddr` 必须是页对齐的虚拟地址
/// - 调用方保证 vaddr 属于当前地址空间或已通过 CR3 切换访问
#[inline(always)]
pub unsafe fn invpcid_flush_single(pcid: u16, vaddr: u64) {
    // SAFETY: INVPCID type 0 (by individual address + PCID).
    // 前提: pcid 有效 (0-4095), vaddr 页对齐.
    // 调用方保证: vaddr 属于调用方地址空间.
    // 硬件契约: INVPCID 指令在支持的 CPU 上原子刷新单条 TLB.
    let desc: [u64; 2] = [pcid as u64, vaddr];
    unsafe {
        core::arch::asm!(
            "invpcid {typ}, [{desc}]",
            typ = in(reg) INVPCID_TYPE_SINGLE,
            desc = in(reg) desc.as_ptr(),
            options(nostack, preserves_flags, readonly),
        );
    }
}
```

- [x] **Step 2: 移除 `#[allow(dead_code)]`**

删除 `INVPCID_TYPE_SINGLE` 的 `#[allow(dead_code)]` 注释 (line ~52)。

- [x] **Step 3: 双架构编译验证**

```bash
./ci/build.sh all
```

预期: aarch64 下 INVPCID 代码被 `#[cfg(target_arch = "x86_64")]` 门控。

- [x] **Step 4: Commit**

```bash
git add src/kernel/framework/mm/kpti.rs
git commit -m "feat(mm): 实现 invpcid_flush_single 单 PCID TLB 刷新 (A-05)"
```

---

## Task A-06: 激活 DMA cache_invalidate (中等)

**覆盖:** A-06

**说明:** 经调研确认:
- `cache_invalidate()` (line 442) 实现已完整: x86_64 用 fence, aarch64 用 `dc ivac`
- `cache_flush()` (line 366) 是写入侧对应函数，已被 `alloc_coherent()` 使用
- `sync_for_cpu()` (line 308) 是 DMA 读取前的同步点，当前仅调用 `barrier_cpu()`
- DMA 读取路径: Device 写入 DMA buffer → `sync_for_cpu()` → CPU 读取

**Files:**
- Modify: `src/kernel/framework/dma/engine.rs` (在 sync_for_cpu 中接入 cache_invalidate)

**Steps:**

- [x] **Step 1: 在 sync_for_cpu 中接入 cache_invalidate**

修改 `sync_for_cpu()` 方法:
```rust
fn sync_for_cpu(&self) {
    // 设备写入完成后, 使 CPU 缓存失效以读取最新数据
    self.cache_invalidate(self.virt_addr, self.size);
    Self::barrier_cpu();
}
```

- [x] **Step 2: 移除 `#[allow(dead_code)]` 和 `#[allow(unused_variables)]`**

删除 `cache_invalidate` 方法上的两个 allow 注释 (line ~441-442)。

- [x] **Step 3: 双架构编译验证**

```bash
./ci/build.sh all
```

- [x] **Step 4: Commit**

```bash
git add src/kernel/framework/dma/engine.rs
git commit -m "feat(dma): 接入 cache_invalidate 到 DMA 读取同步路径 (A-06)"
```

---

## Task A-07: 激活 lockdep any_in_irq 检测 (中等)

**覆盖:** A-07

**说明:** 经调研确认:
- `any_in_irq()` (line 417) 检查当前线程持有的所有锁中是否有在 IRQ 上下文获取的
- `HeldLockEntry.in_irq` 字段在每次 `acquire()` 时通过 `push(class_id, in_irq)` 记录
- `acquire()` (line 476) 已有 3 项检查: IRQ+sleep lock、递归锁、AB-BA 死锁
- 缺失的检查: 持有 IRQ 锁时获取 sleep lock 的潜在死锁

**Files:**
- Modify: `src/kernel/framework/sync/lockdep.rs` (在 acquire 中接入 any_in_irq)

**Steps:**

- [x] **Step 1: 在 acquire 中添加 IRQ 锁持有检查**

在 `acquire()` 函数的 Check 2 (递归锁检测) 之后、Check 3 (AB-BA 检测) 之前添加:

```rust
// Check 2.5: 持有 IRQ 上下文锁时获取 sleep lock → 潜在死锁
if !irq_context && k.may_sleep() && stack.any_in_irq() {
    lockdep_log!(
        "LOCKDEP WARNING: acquiring sleep lock '{}' while holding IRQ-context lock",
        map.class_name(class_id)
    );
    // 警告但不阻塞 (生产环境可配置为 panic)
}
```

- [x] **Step 2: 移除 `#[allow(dead_code)]`**

删除 `any_in_irq()` 的 `#[allow(dead_code)]` 注释 (line ~417)。

- [x] **Step 3: 双架构编译验证**

```bash
./ci/build.sh all
```

- [x] **Step 4: Commit**

```bash
git add src/kernel/framework/sync/lockdep.rs
git commit -m "feat(sync): 接入 lockdep any_in_irq 中断安全检测 (A-07)"
```

---

## Task A-11: 激活 fbterm clear_line (中等)

**覆盖:** A-11

**说明:** 经调研确认:
- `clear_line(row)` (line 93) 逐列绘制空格字形清行，功能完整
- `clear_screen()` 使用 `fill_rect` 直接像素填充，效率更高
- `scroll_up_one()` (line 101) 在滚动后用 `fill_rect` 清除最后一行
- 输入处理中 Backspace (line 252) 手动绘制空格覆盖删除字符

**激活方式:** 在 `scroll_up_one()` 中用 `clear_line` 替代 `fill_rect` 清除最后一行，保持一致性。

**Files:**
- Modify: `src/user/fbterm/src/main.rs` (接入 clear_line + 移除 allow)

**Steps:**

- [x] **Step 1: 优化 clear_line 实现**

当前 `clear_line` 逐列调用 `draw_glyph` 效率较低。改为使用 `fill_rect`:
```rust
fn clear_line(&mut self, row: u32) {
    if row >= self.rows { return; }
    let y0 = row * GLYPH_H;
    self.fill_rect(0, y0, self.width, GLYPH_H, 20, 20, 28);
}
```

- [x] **Step 2: 在 scroll_up_one 中使用 clear_line**

修改 `scroll_up_one()` (line ~108):
```rust
// 原: self.fill_rect(0, y0, self.width, GLYPH_H, 20, 20, 28);
self.clear_line(last_row);
```

- [x] **Step 3: 移除 `#[allow(dead_code)]`**

删除 `clear_line` 的 `#[allow(dead_code)]` 注释 (line ~93)。

- [x] **Step 4: 编译验证**

```bash
cargo build --release  # 用户态程序
```

- [x] **Step 5: Commit**

```bash
git add src/user/fbterm/src/main.rs
git commit -m "feat(fbterm): 激活 clear_line，优化滚动清行逻辑 (A-11)"
```

---

## Task A-12: 激活 eash StdinFile (中等)

**覆盖:** A-12

**说明:** 经调研确认:
- `RedirKind::StdinFile` (line 20) 声明但从未赋值
- `<` 重定向解析 (line 115-124) 设置 `redir_in` 但未设置 `redir_kind`
- 重定向执行 (line 217-227) 通过 `redir_in` 字段工作，与 `redir_kind` 无关
- 输出重定向使用 `redir_kind` 区分 `>` 和 `>>`

**激活方式:** 在 `<` 解析路径中设置 `redir_kind = RedirKind::StdinFile`，保持枚举语义完整。

**Files:**
- Modify: `src/user/eash/src/commands/pipeline.rs` (设置 redir_kind + 移除 allow)

**Steps:**

- [x] **Step 1: 在 `<` 解析路径中设置 redir_kind**

在 `pipeline.rs` line ~121 (`seg.redir_in = Some(...)`) 之后添加:
```rust
seg.redir_kind = RedirKind::StdinFile;
```

- [x] **Step 2: 移除 `#[allow(dead_code)]`**

删除 `StdinFile` variant 的 `#[allow(dead_code)]` 注释 (line ~19)。

- [x] **Step 3: 编译验证**

```bash
cargo build --release  # 用户态程序
```

- [x] **Step 4: Commit**

```bash
git add src/user/eash/src/commands/pipeline.rs
git commit -m "feat(eash): 激活 RedirKind::StdinFile 重定向枚举 (A-12)"
```

---

## Task A-13: 激活 ATA 备用状态寄存器 (中等)

**覆盖:** A-13

**说明:** 经调研确认:
- `ATA_CTRL_ALT_STATUS = 0` 是控制寄存器块的备用状态偏移
- `init()` (line 309) 执行软复位 (写 SRST → delay → 清 SRST → delay) 但未轮询 alt status 确认复位完成
- `ata_delay()` 已通过 `inb(ctrl)` 读取控制端口作为延迟
- ATA 规范 9.2 要求: SRST 后应通过 alt status 轮询 BSY 清除

**Files:**
- Modify: `src/kernel/framework/driver/storage/ata.rs` (接入 alt status 轮询 + 移除 allow)

**Steps:**

- [x] **Step 1: 添加 alt status 读取辅助函数**

在 `ata_delay` 附近添加:
```rust
/// 读取备用状态寄存器 (不清除中断)
#[inline]
fn read_alt_status(ctrl: u16) -> u8 {
    // SAFETY: ctrl 是已验证的 ATA 控制端口地址 (0x3F6 或 0x176).
    // ATA_CTRL_ALT_STATUS (=0) 表示控制寄存器块的偏移 0 即为备用状态.
    unsafe { inb(ctrl + ATA_CTRL_ALT_STATUS as u16) }
}
```

- [x] **Step 2: 在软复位后轮询 alt status 等待 BSY 清除**

修改 `init()` 中的软复位路径 (line ~321-324):
```rust
// Software Reset
outb(ATA_PRIMARY_CTRL, 0x04);   // Set SRST bit
ata_delay(ATA_PRIMARY_CTRL);
outb(ATA_PRIMARY_CTRL, 0x00);   // Clear SRST bit

// ATA 规范 9.2: 轮询备用状态等待 BSY 清除 (400ns minimum)
for _ in 0..1000 {
    if read_alt_status(ATA_PRIMARY_CTRL) & ATA_STATUS_BSY == 0 {
        break;
    }
    core::hint::spin_loop();
}
```

对 Secondary 通道 (line ~356-359) 做相同修改。

- [x] **Step 3: 移除 `#[allow(dead_code)]`**

删除 `ATA_CTRL_ALT_STATUS` 的 `#[allow(dead_code)]` 注释 (line ~57)。

- [x] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

- [x] **Step 5: Commit**

```bash
git add src/kernel/framework/driver/storage/ata.rs
git commit -m "feat(ata): 接入备用状态寄存器轮询，符合 ATA 规范 9.2 (A-13)"
```

---

## Task A-14a: 激活 set_max_sockets 运行时配置 (中等)

**覆盖:** A-14a

**说明:** 经调研确认:
- `set_max_sockets(n)` (line 129) 已完整实现: 参数校验 + 原子存储
- `configure_max_sockets()` 在启动期设置初始值
- `get_max_sockets()` 在 `sm_socket()` 中使用
- 缺失: sysctl/procfs 路径未接入 `set_max_sockets`

**激活方式:** 在 `configure_max_sockets` 中注册 sysctl 处理器，使运行时可通过 sysctl 调整。

**Files:**
- Modify: `src/kernel/framework/net/init.rs` (注册 sysctl)
- Modify: `src/kernel/services/config/` (确认 sysctl 注册 API)

**Steps:**

- [x] **Step 1: 确认 sysctl 注册 API**

读取 `src/kernel/services/config/` 目录，找到 sysctl 注册和写入处理的 API。

- [x] **Step 2: 在 configure_max_sockets 中注册 sysctl**

```rust
pub fn configure_max_sockets() {
    let initial = if DEFAULT_MAX_SOCKETS > MAX_SOCKETS {
        MAX_SOCKETS
    } else if DEFAULT_MAX_SOCKETS == 0 {
        1
    } else {
        DEFAULT_MAX_SOCKETS
    };
    G_MAX_SOCKETS.store(initial, Ordering::Release);

    // 注册 sysctl: net.core.somaxconn
    // 使运行时可通过 sysctl 调整 socket 上限
    // (具体 API 取决于 services/config 的 sysctl 实现)
}
```

- [x] **Step 3: 移除注释中的 dead_code 引用**

`set_max_sockets` 本身无 `#[allow(dead_code)]`，但 line ~95 和 ~916 的注释引用了它。确认注释准确。

- [x] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

- [x] **Step 5: Commit**

```bash
git add src/kernel/framework/net/init.rs
git commit -m "feat(net): 接入 set_max_sockets sysctl 运行时配置 (A-14a)"
```

---

## Task A-14b: 修复 sm_listen 使用 listen_endpoint_to_smol (中等)

**覆盖:** A-14b

**说明:** 经调研确认:
- `sm_listen()` (line 1099) 硬编码 `IpListenEndpoint { addr: None, port: 0 }`
- 这丢弃了 `sm_bind()` 设置的地址/端口
- `listen_endpoint_to_smol()` (line 969) 是正确的翻译函数，但未被 `sm_listen` 使用
- `sm_bind()` 已正确使用 `endpoint_to_smol()` 翻译地址

**Files:**
- Modify: `src/kernel/framework/net/init.rs` (修改 sm_listen 使用 listen_endpoint_to_smol)

**Steps:**

- [x] **Step 1: 修改 sm_listen 使用绑定地址**

将 `sm_listen()` 中的:
```rust
let local = IpListenEndpoint {
    addr: None,
    port: 0,
};
```
替换为从已绑定地址读取 (需要从 SOCKET_TABLE 或 socket 状态中获取 bind 地址)。

具体方案取决于 smoltcp 的 socket 状态 API。需要确认:
1. smoltcp `tcp::Socket` 是否暴露 `local_endpoint()` 方法
2. 若有，使用 `listen_endpoint_to_smol()` 翻译

- [x] **Step 2: 双架构编译验证**

```bash
./ci/build.sh all
```

- [x] **Step 3: Commit**

```bash
git add src/kernel/framework/net/init.rs
git commit -m "fix(net): sm_listen 使用绑定地址而非硬编码 0:0 (A-14b)"
```

---

## Task A-04: 激活 Bochs VBE MMIO 模式 (中等)

**覆盖:** A-04

**说明:** 经调研确认:
- `VBE_DISPI_MMIO_BASE = 0x500` 是 BAR0 上的 MMIO 偏移
- 当前 `read_bochs_disp_mode()` 通过 port I/O (0x01CE/0x01CF) 读取
- `probe_vga_fb_via_pci()` 获取 BAR0 地址后使用 port I/O
- MMIO 模式可避免 port I/O 开销

**Files:**
- Modify: `src/kernel/framework/driver/display/mod.rs` (实现 MMIO 路径)

**Steps:**

- [x] **Step 1: 实现 MMIO 读取函数**

```rust
/// 通过 MMIO 读取 Bochs DISPI 寄存器 (替代 port I/O)
///
/// # Safety
/// - `mmio_base` 必须是有效的 VGA BAR0 映射地址
/// - 偏移计算: VBE_DISPI_MMIO_BASE + reg * 2 (每寄存器 2 字节间距)
#[cfg(target_arch = "x86_64")]
unsafe fn read_bochs_disp_mode_mmio(mmio_base: u64) -> Option<(u32, u32, u8)> {
    unsafe {
        let base = mmio_base + VBE_DISPI_MMIO_BASE;
        let read_reg = |reg: u16| -> u16 {
            core::ptr::read_volatile((base + reg as u64 * 2) as *const u16)
        };
        let id = read_reg(VBE_DISPI_INDEX_ID);
        if id < VBE_DISPI_ID5 { return None; }
        let enabled = read_reg(VBE_DISPI_INDEX_ENABLE);
        if enabled == 0 { return None; }
        let xres = read_reg(VBE_DISPI_INDEX_XRES) as u32;
        let yres = read_reg(VBE_DISPI_INDEX_YRES) as u32;
        let bpp = read_reg(VBE_DISPI_INDEX_BPP) as u8;
        if xres == 0 || yres == 0 || bpp == 0 { return None; }
        Some((xres, yres, bpp))
    }
}
```

- [x] **Step 2: 在 probe_vga_fb_via_pci 中优先使用 MMIO**

```rust
let mode = if bar0.base_addr != 0 {
    unsafe { read_bochs_disp_mode_mmio(bar0.base_addr) }
} else {
    None
};
let (width, height, bpp) = mode
    .or_else(|| read_bochs_disp_mode())
    .unwrap_or((1024, 768, 32));
```

- [x] **Step 3: 移除 `#[allow(dead_code)]`**

- [x] **Step 4: 双架构编译验证**

```bash
./ci/build.sh all
```

- [x] **Step 5: Commit**

```bash
git add src/kernel/framework/driver/display/mod.rs
git commit -m "feat(display): 实现 Bochs VBE MMIO 读取路径 (A-04)"
```

---

## Task A-02: 迁移 ATA/Keyboard 驱动到 IoPort (复杂)

**覆盖:** A-02

**说明:** 经调研确认:
- Serial 和 VGA 已完全迁移到 IoPort
- ATA 驱动有 ~45-50 个 raw `inb`/`outb` 调用 + ~256 个 `inw`/`outw` (per sector)
- Keyboard 驱动有 6 个 raw port I/O 调用点
- IoPort 支持 ISR 上下文 (无锁/无分配)
- IoPort 偏移语义: `read_u8(offset)` → 读取 `base + offset`

**Files:**
- Modify: `src/kernel/framework/driver/storage/ata.rs` (迁移到 IoPort)
- Modify: `src/kernel/framework/driver/input/keyboard.rs` (迁移到 IoPort)
- Modify: `src/kernel/framework/ioport.rs` (移除文件级 allow)

**Steps:**

- [x] **Step 1: 为 ATA 驱动创建 IoPort 实例**

在 `AtaController` 结构体中添加 IoPort 字段:
```rust
use crate::kernel::framework::ioport::IoPort;

struct AtaController {
    io: IoPort,      // I/O 端口块 (0x1F0-0x1F7 或 0x170-0x177)
    ctrl: IoPort,    // 控制端口 (0x3F6 或 0x176)
    // ... 其他字段
}
```

在初始化时创建:
```rust
// SAFETY: ATA I/O 端口地址由 PCI 枚举或固定映射确定, 无重叠.
let io = unsafe { IoPort::new(0x1F0, 8, "ata-primary")? };
let ctrl = unsafe { IoPort::new(0x3F6, 1, "ata-primary-ctrl")? };
```

- [x] **Step 2: 逐步替换 ATA 驱动中的 raw port I/O**

按函数逐个替换:
1. `ata_delay()`: `inb(ctrl)` → `ctrl.read_u8(0)`
2. `wait_bsy()`: `inb(io + ATA_STATUS)` → `io.read_u8(ATA_STATUS as u16)`
3. `select_drive()`: `outb(io + ATA_DRIVE_HEAD, ...)` → `io.write_u8(ATA_DRIVE_HEAD as u16, ...)`
4. `detect_drive()`: 所有 inb/outb/inw/outw 替换
5. `read_sector()` / `write_sector()`: 所有 inw/outw 替换

对 Secondary 通道创建第二组 IoPort 实例。

- [x] **Step 3: 为 Keyboard 驱动创建 IoPort 实例**

```rust
struct Ps2Controller {
    data: IoPort,    // 0x60
    cmd: IoPort,     // 0x64
}
```

替换 6 个 raw port I/O 调用点。

- [x] **Step 4: 移除 ioport.rs 文件级 allow**

删除 `#![allow(dead_code)]` (line 19)。

- [x] **Step 5: 双架构编译验证**

```bash
./ci/build.sh all
```

- [x] **Step 6: host-tests 验证**

```bash
make test-host
```

- [x] **Step 7: Commit**

```bash
git add src/kernel/framework/driver/storage/ata.rs \
        src/kernel/framework/driver/input/keyboard.rs \
        src/kernel/framework/ioport.rs
git commit -m "refactor(driver): 迁移 ATA/Keyboard 到 IoPort 安全抽象 (A-02)"
```

---

## Task A-01: 激活 MSI/MSI-X 子系统 (复杂)

**覆盖:** A-01

**说明:** 经调研发现重大缺口:
- `msi_enable()` 写入 PCI config 但**未注册 IDT handler** for 分配的 vector
- `IdtManager::register_irq()` 仅支持 IRQ 0-15 (vector 0x20-0x2F)
- MSI vector 0x40-0x7F **无 ISR stub** — CPU 跳转后无处理代码
- `IrqLine` 类型存在但**零调用者** — 未被任何驱动使用
- NVMe 驱动存储了 `PciDevice` 引用但未调用 `msi_enable()`
- VirtIO net 使用 MMIO transport (非 PCI)，MSI 不适用

**激活需要的基础设施改动:**

| 改动 | 说明 | 风险 |
|------|------|------|
| 1. 为 vector 0x40-0x7F 生成 ISR stub | 在 idt/mod.rs 的 isr_table 中扩展 | 中 |
| 2. 修改 IDT dispatch 支持 MSI vector | `handle_irq` 或新增 `handle_msi_irq` | 高 |
| 3. 在 `msi_enable()` 中注册 IDT handler | 调用 `IrqLine::on_interrupt()` | 中 |
| 4. NVMe 驱动调用 `msi_enable()` | 存储 PciDevice + MsiConfig | 低 |

**Files:**
- Modify: `src/kernel/framework/idt/mod.rs` (扩展 ISR stub + dispatch)
- Modify: `src/kernel/framework/idt/irqline.rs` (确认 IrqLine 可用于 MSI vector)
- Modify: `src/kernel/framework/pci/msi.rs` (在 msi_enable 中注册 handler)
- Modify: `src/kernel/framework/driver/storage/nvme.rs` (接入 MSI)

**Steps:**

- [x] **Step 1: 为 MSI vector 0x40-0x7F 生成 ISR stub**

在 `idt/mod.rs` 的 ISR stub 生成逻辑中，为 vector 0x40-0x7F 添加 stub:
```rust
// MSI vector stubs (0x40-0x7F)
for i in 0x40..=0x7F {
    // 生成对应的 ISR stub 入口
}
```

- [x] **Step 2: 修改 IRQ dispatch 支持 MSI vector**

在 `handle_irq` 或 `dispatch_irq` 中，扩展 vector 范围检查:
```rust
// 原: 仅处理 IRQ 0-15 (vector 0x20-0x2F)
// 新: 也处理 MSI vector 0x40-0x7F
if vector >= 0x40 && vector <= 0x7F {
    // 通过 ISR_TABLE dispatch
    irqline::dispatch_irq(vector);
}
```

- [x] **Step 3: 在 msi_enable 中注册 IDT handler**

修改 `msi_enable()` 返回前:
```rust
// 注册 IDT handler for MSI vector
let mut irq = unsafe { IrqLine::new(0, config.vector) };
irq.on_interrupt(handler)?;
irq.enable();
```

这要求 `msi_enable()` 接受一个 `InterruptHandler` 参数。

- [x] **Step 4: NVMe 驱动接入 MSI**

在 NVMe 的 `init()` 中:
```rust
if let Some(ref pci_dev) = self.pci_device {
    if let Some(msi_cfg) = msi_enable(pci_dev) {
        self.msi_config = Some(msi_cfg);
        // 注册中断处理函数
    }
}
```

- [x] **Step 5: 移除 msi.rs 文件级 allow**

- [x] **Step 6: 双架构编译验证**

```bash
./ci/build.sh all
```

- [x] **Step 7: host-tests 验证**

```bash
make test-host
```

- [x] **Step 8: Commit**

```bash
git add src/kernel/framework/idt/mod.rs \
        src/kernel/framework/idt/irqline.rs \
        src/kernel/framework/pci/msi.rs \
        src/kernel/framework/driver/storage/nvme.rs
git commit -m "feat(pci): 完整激活 MSI/MSI-X，含 ISR stub + IDT dispatch + NVMe 接入 (A-01)"
```

---

## Task A-13 续: queenx-tests dead_code 评估 (N-04)

**覆盖:** N-04

**Files:**
- Modify: `src/rust/queenx-tests/src/lib.rs`

**Steps:**

- [x] **Step 1: 读取 queenx-tests 确认用途**

确认该 crate 的测试是否被主测试流程引用。

- [x] **Step 2: 评估是否可移除文件级 allow**

若所有测试函数均被 `#[test]` 标注且有调用者，可移除 `#![allow(dead_code)]`。

- [x] **Step 3: Commit (如需要)**

---

## Task 全量验证

**Steps:**

- [x] **Step 1: 双架构编译**

```bash
./ci/build.sh all
```

- [x] **Step 2: 全量审计**

```bash
ci/audit.sh full
```

- [x] **Step 3: host-tests**

```bash
make test-host
```

- [x] **Step 4: grep 确认 dead_code 消除进度**

```bash
grep -rn "#\[allow(dead_code)\]" src/kernel/framework/ src/kernel/services/ src/user/ | grep -v smoltcp | grep -v "//.*allow(dead_code)"
```

预期: 仅剩 queenx-tests (N-04 待评估) 的 allow。
