# 死代码降级消除计划

> 2026-07-17: 消除通过 `#[allow(dead_code)]` 降级掩盖的 58 处死代码.
>
> **前提**: 此前 dead code 消除工程将违规数从 139 降至 0 (通过降级掩盖).
> 本文档规划真正消除这些降级死代码的方案.

---

## 一、分类总览

| 分类 | 数量 | 处理方式 | 说明 |
|------|------|---------|------|
| 硬件规范常量 (未使用) | 47 | 删除 | 寄存器偏移/标志位, 当前无使用路径 |
| 未使用函数 | 8 | 删除 | 实现了但从未被调用 |
| cfg 条件编译项 | 3 | 删除 | 被 `#[cfg]` 包裹但实际未启用 |

**总计**: 58 项

---

## 二、详细清单

### Group A: 硬件规范常量 (47 项)

#### A1: GIC 寄存器 (gic.rs, 10 项)

| 常量 | 值 | 说明 |
|------|-----|------|
| GICD_TYPER | 0x0008 | GIC 类型寄存器 |
| GICD_IIDR | 0x000C | GIC 实现者 ID |
| GICD_ISENABLER | 0x0100 | 中断 Set-Enable |
| GICD_ISPENDR | 0x0200 | 中断 Set-Pending |
| GICD_ICFGR | 0x0C00 | 中断配置 (level/edge) |
| GICR_IGROUPR0 | 0x0080 | SGI/PPI 分组 |
| GICR_ICFGR1 | 0x0C04 | PPI 配置 |
| PPI_BASE | 16 | PPI 起始号 |
| SPI_BASE | 32 | SPI 起始号 |
| gicd_read | 函数 | GICD 寄存器读取 |

**处理**: 删除常量定义 + 删除 `gicd_read` 函数

#### A2: APIC 寄存器 (apic.rs, 8 项)

| 常量 | 值 | 说明 |
|------|-----|------|
| APIC_ISR_BASE | 0x100 | ISR 寄存器基址 |
| APIC_TMR_BASE | 0x180 | TMR 寄存器基址 |
| APIC_IRR_BASE | 0x200 | IRR 寄存器基址 |
| LVT_DELIVERY_FIXED | 0x000 | Fixed 投递模式 |
| LVT_DELIVERY_SMI | 0x200 | SMI 投递模式 |
| LVT_DELIVERY_NMI | 0x400 | NMI 投递模式 |
| ICR_LEVEL | 1 << 15 | ICI 电平触发 |
| ICR_BROADCAST | 1 << 19 | ICI 广播 |

**处理**: 删除常量定义

#### A3: IOAPIC 寄存器 (ioapic.rs, 7 项)

| 常量 | 值 | 说明 |
|------|-----|------|
| IOAPIC_ID | 0x00 | IOAPIC ID 寄存器 |
| IOAPIC_ARB | 0x02 | IOAPIC 仲裁寄存器 |
| REDTBL_LOW_PRIORITY | 1 << 13 | 低优先级路由 |
| REDTBL_LOGICAL | 1 << 11 | 逻辑目标模式 |
| DELIVERY_SMI | 0x200 | SMI 投递模式 |
| DELIVERY_NMI | 0x400 | NMI 投递模式 |
| DELIVERY_EXTINT | 0x700 | ExtINT 投递模式 |

**处理**: 删除常量定义

#### A4: ATA 寄存器 (ata.rs, 5 项)

| 常量 | 值 | 说明 |
|------|-----|------|
| ATA_CTRL_ALT_STATUS | 0 | 替代状态寄存器 |
| ATA_STATUS_DSC | 0x10 | Seek Complete |
| ATA_STATUS_CORR | 0x04 | Corrected Data |
| ATA_STATUS_IDX | 0x02 | Index |
| ATA_TIMEOUT_ERR | -2 | 超时错误码 |

**处理**: 删除常量定义

#### A5: e1000 寄存器 (e1000.rs, 5 项)

| 常量 | 值 | 说明 |
|------|-----|------|
| E1000_EERD | 0x0014 | EEPROM 读寄存器 |
| E1000_EERD_START | 1 << 0 | EEPROM 读启动位 |
| E1000_EERD_DONE | 1 << 4 | EEPROM 读完成位 |
| E1000_ICR_RXO | 1 << 6 | 接收溢出中断 |
| eeprom_read | 函数 | QEMU 路径下未调用 |

**处理**: 删除常量定义 + 删除 `eeprom_read` (QEMU 路径)

#### A6: PCI 寄存器 (pci/mod.rs, 4 项)

| 常量/函数 | 值 | 说明 |
|----------|-----|------|
| outw | 函数 | 16 位 I/O 写 |
| inb | 函数 | 8 位 I/O 读 |
| inw | 函数 | 16 位 I/O 读 |
| REG_CLASS_CODE | 0x0B | PCI 子类编程接口 |

**处理**: 删除函数定义 + 删除常量定义

#### A7: Keyboard 寄存器 (keyboard.rs, 4 项)

| 常量 | 值 | 说明 |
|------|-----|------|
| PS2_STATUS_SYSTEM | 0x04 | 系统标志 |
| KB_CMD_ECHO | 0xEE | Echo 命令 |
| KB_CMD_SCANCODE | 0xF0 | 扫描码集命令 |
| KB_CMD_IDENTIFY | 0xF2 | Identify 命令 |

**处理**: 删除常量定义

#### A8: 其他驱动 (14 项)

| 文件 | 常量/函数 | 说明 |
|------|----------|------|
| serial.rs:60 | LSR_TRANSMIT_IDLE | 发送器空闲 |
| vga.rs:50 | VGA_DATA_REGISTER | VGA 数据端口 |
| font.rs:8 | GLYPH_BYTES | 字形字节数 |
| display/mod.rs:117 | VBE_DISPI_MMIO_BASE | Bochs VBE MMIO |
| dma/engine.rs:442 | cache_invalidate | DMA 缓存失效 |
| mm/kpti.rs:52 | INVPCID_TYPE_SINGLE | INVPCID 单条目 |
| virtio/blk.rs:118 | BLK_CONFIG_CAPACITY_HI | >2TB 容量高 32 位 |
| timer/pit.rs:32-34 | PIT_CHANNEL_1/2_DATA | PIT 通道 1/2 |
| usb/xhci.rs:169,172 | PORT_ENABLED/POWER | xHCI 端口状态 |
| sync/lockdep.rs:417 | any_in_irq | 中断上下文检测 |
| services/syscall/mod.rs:208 | USER_ADDR_MAX | 用户地址上限 |

**处理**: 删除常量定义 + 删除函数定义

---

## 三、实施顺序

| 阶段 | Group | 工作量 | 说明 |
|------|-------|--------|------|
| Phase 1 | A1-A3 (GIC/APIC/IOAPIC) | 15 分钟 | 中断控制器常量 |
| Phase 2 | A4-A7 (ATA/e1000/PCI/Keyboard) | 15 分钟 | 驱动常量 |
| Phase 3 | A8 (其他驱动) + 函数删除 | 20 分钟 | 混合项 |

**总预估**: 50 分钟

---

## 四、验证标准

每个 Phase 完成后:

1. 双架构编译 0 warning 0 error
2. `audit_dead_code.py` 违规数保持 0
3. host-tests 通过
4. 相关功能不受影响

---

## 五、最终目标

消除全部 58 项降级死代码, 内核无 `#[allow(dead_code)]` 注解 (除审计脚本豁免的硬件规范定义).

**当前进度**:
- Phase 1 (A1-A3): [X] 已完成 — 中断控制器常量
- Phase 2 (A4-A7): [X] 已完成 — 驱动常量
- Phase 3 (A8 + 函数): [X] 已完成 — 混合项

**最终状态**: 58 项降级死代码全部消除, audit_dead_code.py 违规数 = 0 (2026-07-17 验证)
