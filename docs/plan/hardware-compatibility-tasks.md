# 硬件兼容性开发任务

> IOAPIC/Local APIC/PS/2 子系统的硬件兼容性缺口，需独立开发补齐。

## 背景

QueenX 中断与输入子系统在基础功能验证通过后，缺少若干高级硬件特性支持。本文件记录已识别的缺口及对应开发任务。

---

## P0: IOAPIC 高级投递模式

**风险等级:** 高
**涉及已删除常量:** `DELIVERY_SMI` (0x200), `DELIVERY_NMI` (0x400), `DELIVERY_EXTINT` (0x700)

### 问题描述

当前 IOAPIC 驱动 (`framework/arch/x86_64/ioapic.rs`) 仅支持 `DELIVERY_FIXED` 投递模式。删除的 3 个常量对应 IOAPIC 规范中 3 种高级投递模式:

| 模式 | 值 | 用途 | 影响 |
|------|-----|------|------|
| SMI | 0x200 | 系统管理中断 — 固件 (BIOS/UEFI) 与 OS 通信机制 | 服务器固件事件无法正确路由 |
| NMI | 0x400 | 不可屏蔽中断 — 硬件错误报告 (ECC 内存错误、看门狗超时) | 多核系统上硬件错误无法到达目标 CPU |
| ExtINT | 0x700 | 8259A 兼容中断 — 遗留 ISA 设备 | ISA 设备中断无法正确投递 |

**注意:** Local APIC 的 `LVT_DELIVERY_SMI/NMI/EXTINT` (不同常量) 已保留，不受影响。

### 涉及文件

- `src/kernel/framework/arch/x86_64/ioapic.rs` — 添加投递模式常量 + 路由配置
- `src/kernel/framework/arch/x86_64/apic.rs` — Local APIC LVT 投递模式支持

### 验证标准

- [ ] IOAPIC 路由配置支持 SMI/NMI/ExtINT 投递模式
- [ ] 双架构编译 0 warning 0 error
- [ ] 审计全部通过
- [ ] QEMU 启动测试通过

---

## P1: Local APIC 寄存器内省

**风险等级:** 中
**涉及已删除常量:** `APIC_ISR_BASE` (0x100), `APIC_TMR_BASE` (0x180), `APIC_IRR_BASE` (0x200)

### 问题描述

Local APIC 有 3 组只读寄存器用于中断状态内省:

| 寄存器组 | 偏移 | 用途 |
|---------|------|------|
| ISR (In-Service Register) | 0x100-0x17F | 记录正在处理的中断 |
| TMR (Trigger Mode Register) | 0x180-0x1FF | 记录中断触发模式 (边沿/电平) |
| IRR (Interrupt Request Register) | 0x200-0x27F | 记录待处理的中断请求 |

**诊断价值:** 当出现"中断挂起但 CPU 未响应"时，通过读取 IRR 确认中断已到达 APIC，通过 ISR 确认是否在处理中，通过 TMR 确认触发模式是否正确。没有此能力，中断调试只能靠猜测。

### 涉及文件

- `src/kernel/framework/arch/x86_64/apic.rs` — 添加寄存器读取函数
- `src/kernel/framework/idt/` — 中断诊断路径接入

### 验证标准

- [ ] 提供 `apic_read_isr()`, `apic_read_tmr()`, `apic_read_irr()` 函数
- [ ] 每个 32-bit 寄存器组的 8 个寄存器均可读取
- [ ] 双架构编译 0 warning 0 error
- [ ] 审计全部通过

---

## P1: PS/2 键盘扫描码集协商

**风险等级:** 中
**涉及已删除常量:** `KB_CMD_SCANCODE` (0xF0), `KB_CMD_IDENTIFY` (0xF2)

### 问题描述

键盘驱动 (`framework/driver/input/keyboard.rs`) 硬编码了 scancode set 1 映射表。现代 PS/2 键盘默认使用 scancode set 2:

- **QEMU/Bochs:** PS/2 控制器自动做 set 2 → set 1 转换，驱动正常工作
- **真实硬件:** 某些 PS/2 控制器**不做此转换**，键盘直接发送 set 2 扫描码，驱动解析出错误字符

**需要的能力:**

1. `KB_CMD_SCANCODE` (0xF0): 查询当前扫描码集 + 切换到 set 1
2. `KB_CMD_IDENTIFY` (0xF2): 识别键盘类型 (AT/XT/Enhanced)，处理不同键盘的初始化差异

### 涉及文件

- `src/kernel/framework/driver/input/keyboard.rs` — 键盘初始化路径添加扫描码集协商

### 验证标准

- [ ] 键盘初始化时发送 `KB_CMD_SCANCODE` 查询当前扫描码集
- [ ] 若非 set 1，发送 `KB_CMD_SCANCODE` 切换到 set 1
- [ ] 若 set 2, 切换失败，使用 set 2 映射表 (后备方案)
- [ ] 双架构编译 0 warning 0 error
- [ ] QEMU 键盘输入测试通过

---

## P2: 多 IOAPIC 支持

**风险等级:** 低
**涉及已删除常量:** `IOAPIC_ID` (0x00), `IOAPIC_ARB` (0x02)

### 问题描述

当前 IOAPIC 驱动假设系统只有 1 个 IOAPIC 控制器。多路 CPU 服务器可能有多个 IOAPIC，需要通过 `IOAPIC_ID` 寄存器区分:

- `IOAPIC_ID` (offset 0x00): 读取/设置 IOAPIC 标识符
- `IOAPIC_ARB` (offset 0x02): 仲裁 ID (多 IOAPIC 中断分配)

**当前影响:** 单 IOAPIC 场景 (QEMU/桌面) 不受影响。多路服务器才需要。

### 涉及文件

- `src/kernel/framework/arch/x86_64/ioapic.rs` — 多 IOAPIC 枚举与管理

### 验证标准

- [ ] 支持通过 MADT 表枚举多个 IOAPIC
- [ ] 每个 IOAPIC 可独立配置中断路由
- [ ] 双架构编译 0 warning 0 error

---

## 关联文档

- `docs/explain/explain-framekernel.md` — 框内核架构说明
- `AGENTS.md` §4 — 架构责任分离
