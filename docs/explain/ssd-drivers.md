# AntX 固态硬盘驱动开发指南

## 📋 概述

本文档介绍 AntX 内核的固态硬盘（SSD）驱动，包括 NVMe 和 AHCI/SATA 接口。

## 🔧 已实现的驱动

### 1. NVMe 驱动 (Non-Volatile Memory Express)

#### NVMe 控制器 (`storage/nvme.rs`)

**功能**：
- **PCIe接口**：高速PCIe总线连接
- **多队列**：支持多个I/O队列并行处理
- **命名空间**：多个逻辑设备支持
- **高性能**：直接内存访问，低延迟

**架构**：
```text
NVMe Controller
├── Admin队列对 (管理命令)
├── I/O队列对 (数据传输)
├── 命名空间管理
└── 命令处理
```

**使用示例**：
```rust
use crate::kernel::driver::storage::nvme::NvmeController;

let mut nvme = NvmeController::new(0xFE000000);  // PCIe MMIO基地址
nvme.init().unwrap();

// 获取命名空间数量
let ns_count = nvme.namespace_count();
println!("Found {} namespaces", ns_count);

// 创建I/O队列
let qid = nvme.create_io_queue(64).unwrap();

// 读取数据
let buffer = vec![0u8; 4096];
nvme.read(1, 0, 1, buffer.as_mut_ptr()).unwrap();

// 写入数据
nvme.write(1, 0, 1, buffer.as_ptr()).unwrap();

// TRIM命令
nvme.trim(1, 0, 8).unwrap();
```

**NVMe命令**：
- **Admin命令**：
  - Identify: 识别控制器/命名空间
  - Create SQ/CQ: 创建队列
  - Set/Get Features: 设置/获取特性
  
- **NVM命令**：
  - Read: 读取数据
  - Write: 写入数据
  - Flush: 刷新缓存
  - Dataset Management: TRIM

**NVMe寄存器**：
```text
Controller Registers:
├── CAP: 控制器能力
├── VS: 版本
├── CC: 控制器配置
├── CSTS: 控制器状态
├── AQA: Admin队列属性
├── ASQ: Admin提交队列基址
└── ACQ: Admin完成队列基址
```

**队列管理**：
- Admin队列：管理命令（创建队列、识别设备等）
- I/O队列：数据传输命令（读、写、TRIM等）
- 队列深度：最多65536个条目
- 多队列并行：提高I/O性能

---

### 2. AHCI/SATA 驱动

#### AHCI 控制器 (`storage/ahci.rs`)

**功能**：
- **SATA接口**：传统SATA SSD和HDD
- **NCQ支持**：原生命令队列（最多32个命令）
- **热插拔**：设备动态连接
- **多端口**：支持最多32个SATA端口

**架构**：
```text
AHCI Controller
├── HBA内存 (ABAR)
├── 端口管理 (最多32个)
│   ├── 命令列表
│   ├── FIS缓冲区
│   └── PRDT (物理区域描述符)
└── 中断处理
```

**使用示例**：
```rust
use crate::kernel::driver::storage::ahci::AhciController;

let mut ahci = AhciController::new(0xFE000000);  // MMIO基地址
ahci.init().unwrap();

// 获取端口数量
let port_count = ahci.port_count();
println!("Found {} active ports", port_count);

// 通过端口访问设备
if let Some(port) = ahci.ports.get_mut(0) {
    // 读取数据
    let buffer = vec![0u8; 512];
    port.read(0, 1, buffer.as_mut_ptr()).unwrap();
    
    // 写入数据
    port.write(0, 1, buffer.as_ptr()).unwrap();
}
```

**AHCI寄存器**：
```text
HBA Generic Host Control:
├── CAP: HBA能力
├── GHC: 全局HBA控制
├── IS: 中断状态
├── PI: 端口实现
└── VS: 版本

Port Registers (0x10 + 0x80*n):
├── PxCLB: 命令列表基址
├── PxFB: FIS基址
├── PxIS: 中断状态
├── PxIE: 中断使能
├── PxCMD: 命令和状态
├── PxTFD: 任务文件数据
├── PxSIG: 设备签名
├── PxSSTS: SATA状态
└── PxCI: 命令发布
```

**NCQ (Native Command Queuing)**：
- 最多32个并发命令
- 标签管理（0-31）
- 乱序完成
- 提高性能

**SATA速度**：
- SATA I: 1.5 Gbps (150 MB/s)
- SATA II: 3.0 Gbps (300 MB/s)
- SATA III: 6.0 Gbps (600 MB/s)

---

## 📊 性能对比

### NVMe vs AHCI/SATA

| 特性 | NVMe | AHCI/SATA |
|------|------|-----------|
| 接口 | PCIe | SATA |
| 最大队列数 | 65536 | 1 (NCQ: 32) |
| 队列深度 | 65536 | 32 (NCQ) |
| 延迟 | ~10μs | ~100μs |
| 最大吞吐 | 7+ GB/s | 600 MB/s |
| CPU开销 | 低 | 高 |
| 命令开销 | 低 | 高 |

### 性能优化建议

**NVMe优化**：
1. 使用多个I/O队列（每CPU核心一个）
2. 增加队列深度（提高并行度）
3. 使用大块I/O（减少命令开销）
4. 启用写入缓存（提高写入性能）

**AHCI优化**：
1. 启用NCQ（提高并发）
2. 使用大块I/O（减少命令开销）
3. 启用写入缓存
4. 避免碎片化I/O

---

## 🚀 QEMU 测试

### NVMe 测试

QEMU支持NVMe设备模拟：

```bash
# 创建NVMe设备
qemu-img create -f qcow2 nvme.qcow2 10G

# 启动QEMU with NVMe
qemu-system-x86_64 \
    -device nvme,drive=nvme0,serial=NVME001 \
    -drive if=none,id=nvme0,file=nvme.qcow2
```

### AHCI/SATA 测试

QEMU支持AHCI控制器：

```bash
# 创建SATA磁盘
qemu-img create -f qcow2 sata.qcow2 10G

# 启动QEMU with AHCI
qemu-system-x86_64 \
    -device ahci,id=ahci \
    -drive id=sata0,file=sata.qcow2,if=none \
    -device ide-hd,drive=sata0,bus=ahci.0
```

---

## 📁 文件结构

```
src/kernel/driver/storage/
├── mod.rs           # 存储模块注册
├── nvme.rs          # NVMe驱动 (PCIe SSD)
└── ahci.rs          # AHCI/SATA驱动
```

---

## 🎯 SSD 特性支持

### TRIM/Discard

**NVMe TRIM**：
```rust
// 数据集管理命令
nvme.trim(nsid, lba, count)?;
```

**SATA TRIM**：
```rust
// DataSetManagement命令
port.trim(lba, count)?;
```

TRIM的好处：
- 提高写入性能
- 延长SSD寿命
- 减少写放大

### 磨损均衡

SSD内部自动管理：
- 动态磨损均衡
- 静态磨损均衡
- 坏块管理

### 写入缓存

**启用写入缓存**：
```rust
// NVMe
nvme.set_feature(FEATURE_VOLATILE_WRITE_CACHE, 1)?;

// SATA
port.set_feature(0x82, 1)?;  // 启用写入缓存
```

**注意**：启用写入缓存可能丢失数据，需要定期刷新。

---

## 🔍 故障排查

### NVMe 问题

**控制器不就绪**：
```
检查：CSTS.RDY位是否为1
解决：等待控制器初始化完成
```

**命令超时**：
```
检查：队列是否正常工作
解决：检查门铃寄存器、完成队列
```

### AHCI 问题

**端口未检测到设备**：
```
检查：PxSSTS.DET字段
解决：检查SATA连接、电源
```

**NCQ不工作**：
```
检查：CAP.SNCQ位是否为1
解决：确保控制器支持NCQ
```

---

## 📚 参考资料

### NVMe规范
- [NVMe Specification 1.4](https://nvmexpress.org/specifications/)
- [NVMe Express Base Specification 2.0](https://nvmexpress.org/specifications/)

### AHCI规范
- [AHCI Specification 1.3.1](https://www.intel.com/content/www/us/en/io-controller-hub/ahci-specification.html)
- [SATA Specification 3.5](https://www.sata-io.org/)

### QEMU文档
- [QEMU NVMe Emulation](https://qemu.org/docs/master/system/devices/nvme.html)
- [QEMU IDE/AHCI](https://qemu.org/docs/master/system/devices/ide.html)

---

## 📝 更新日志

**2026-05-18**：
- ✅ 实现NVMe控制器驱动
- ✅ 实现NVMe命令和队列管理
- ✅ 实现AHCI/SATA控制器驱动
- ✅ 实现NCQ支持
- ✅ 支持TRIM/Discard命令
- ✅ 创建SSD驱动文档

---

**最后更新**：2026-05-18  
**维护者**：AntX Team
