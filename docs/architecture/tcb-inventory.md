# QueenX TCB Inventory — unsafe 分类清单

> **生成日期**: 2026-06-02
> **Phase 0.3 产物**: 全量 unsafe 行 `→ framework` vs `→ services (可下沉)` 分类
> **总计**: 1,688 unsafe 行, 30+ 文件

## 分类标准

| 类别 | 判定 | 归属 |
|------|------|------|
| **Hardware** | MMIO/PIO/寄存器/IDT/GDT/页表/APIC/GIC/DMA | `framework` |
| **Context** | 上下文切换/asm stub/用户态进入 | `framework` |
| **Primitive** | 同步原语(RawMutex/atomic)/内存分配器底层 | `framework` |
| **Platform** | 架构特定操作 (arch!) | `framework` |
| **Driver** | 驱动 MMIO (可走 IoMem 下沉) | `services` (Phase 2) |
| **Logic** | 进程表/syscall 分发/FS 操作 raw ptr | `services` (Phase 2) |

## 按子系统分类

| 子系统 | unsafe 行 | 必须保留 (framework) | 可下沉 (services) | 归属 |
|--------|-----------|---------------------|-------------------|------|
| **sync** | 122 | 122 (RawMutex 底层) | 0 | framework |
| **mm** | 188 | 140 (页表/PM/Slab 底层) | 48 (COW/VMA raw ptr 逻辑) | 混合 |
| **proc** | 181 | 80 (ctx_switch/asm stub) | 101 (进程表 raw ptr/调度器逻辑) | 混合 |
| **syscall** | 130 | 0 | 130 (分发/user ptr) | services |
| **fs** | 116 | 0 | 116 (ramfs/HvFS raw ptr) | services |
| **driver** | 249 | 0 | 249 (走 IoMem/IrqLine) | services |
| **arch/x86_64** | 75 | 75 (GDT/IDT/APIC/ACPI) | 0 | framework |
| **arch/aarch64** | 87 | 87 (MMU/GIC/PSCI) | 0 | framework |
| **credo** | 77 | 0 | 77 (session/全局锁 → framework::sync) | services |
| **chitin** | 55 | 0 | 55 (设备注册表 raw ptr) | services |
| **idt** | 54 | 54 (IDT 硬件) | 0 | framework |
| **ipc** | 50 | 0 | 50 (raw ptr → VmSpace/Frame) | services |
| **net** | 45 | 0 | 45 (smoltcp FFI/init) | services |
| **barrier** | 33 | 0 | 33 (恢复域逻辑) | services |
| **cpu** | 30 | 30 (CPUID/MSR/寄存器) | 0 | framework |
| **lib** | 28 | 28 (优化 memset/字符串) | 0 | framework (工具) |
| **boot** | 26 | 26 (启动逻辑) | 0 | framework (启动) |
| **klog** | 21 | 21 (串口/日志硬件) | 0 | framework (IO) |
| **pci** | 20 | 20 (Port I/O/ECAM MMIO) | 0 | framework |
| **dma** | 10 | 10 (DMA 引擎) | 0 | framework |
| **合计** | **1,688** | **~693 (41%)** | **~995 (59%)** | — |

## 迁移结论

| 类别 | 行数 | 说明 |
|------|------|------|
| framework (Phase 1) | ~693 | 已在 `arch/mm/sync/cpu/idt/boot` 中, 迁移不需要移动代码, 只需加 SAFETY 注释 + 安全 API 封装 |
| services (Phase 2) | ~995 | 通过 IoMem/IrqLine/VmSpace/Frame 抽象后消除 unsafe |

**TCB 目标**: Phase 1 完成后 framework ≈ 3,000-5,000 LoC (含新 API 封装层)
**TCB 占比**: 5,000 / 82,000 ≈ 6% (优于 Asterinas 的 14%)
