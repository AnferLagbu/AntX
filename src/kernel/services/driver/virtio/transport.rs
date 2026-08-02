#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! `VirtIO` MMIO Transport — services 层安全代理
//!
//! 封装 [`kernel::driver::virtio::VirtioMmioDevice`] 的核心 MMIO 操作,
//! 通过 `framework::IoMem` 提供类型安全的 `VirtIO` 设备访问。
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 所有 MMIO 读/写通过 `IoMem` 安全代理, 边界检查在 `IoMem` 内部
//! - **薄包装**: 不复制 transport 内部状态机, 仅提供类型化 API
//! - **零开销**: `#[inline]` 让编译器将 `read32`/`write32` 内联为直接 volatile 访问
//! - **可替代**: 原 `kernel/driver/virtio/mod.rs` 仍存在, 本文件是迁移目标
//!
//! ## 与原驱动的差异
//!
//! 原 `VirtioMmioDevice` 在 `kernel/driver/` 下, 是 `pub` 但仍含少量 `unsafe` (例如 `IoMem::new` 包装).
//! 本文件提供 100% safe 替代: 内部 `IoMem` 来自 PCI 枚举路径 (使用 `from_pci_bar`),
//! 由 services 层调用, 避免 `unsafe` 边界跨越 services/framework.
//!
//! ## 适用范围
//!
//! - `services/driver/virtio/blk.rs` (virtio-blk 块设备)
//! - `services/driver/virtio/net.rs` (virtio-net 网卡)
//! - `services/driver/virtio/gpu.rs` (virtio-gpu 显示设备, 未来)
//!
//! 评估日期: 2026-06-04
//! Phase 2.1.2 任务: `VirtIO` 传输层迁移

use crate::kernel::framework::iomem::IoMem;
use crate::kernel::framework::mm::PhysAddr;
use crate::kernel::framework::driver::virtio::{
    VIRTIO_MMIO_BASE, VIRTIO_MMIO_STRIDE, VIRTIO_MMIO_MAX_DEVICES,
};

// ── MMIO 寄存器偏移 (与 VirtIO 1.0 规范一致) ──

/// `MagicValue` 寄存器, 读取应为 0x74726976 ("virt")
pub const MAGIC_VALUE: usize = 0x000;
/// Version 寄存器: 1=transitional/legacy, 2=modern
pub const VERSION: usize = 0x004;
/// `DeviceID` 寄存器: 1=net, 2=blk, 16=gpu
pub const DEVICE_ID: usize = 0x008;
/// `VendorID` 寄存器: 0x554d4551 ("QEMU")
pub const VENDOR_ID: usize = 0x00c;
/// `DeviceFeatures`[sel:0]: 设备特性位 [0..31]
pub const DEVICE_FEATURES: usize = 0x010;
/// `DeviceFeaturesSel`: 写以选择 32 位特性字
pub const DEVICE_FEATURES_SEL: usize = 0x014;
/// `DriverFeatures`[sel:0]: 驱动写特性位
pub const DRIVER_FEATURES: usize = 0x020;
/// `DriverFeaturesSel`
pub const DRIVER_FEATURES_SEL: usize = 0x024;
/// `QueueSel`: 写以选择 virtqueue
pub const QUEUE_SEL: usize = 0x030;
/// `QueueNumMax`: 选中队列的最大尺寸
pub const QUEUE_NUM_MAX: usize = 0x034;
/// `QueueNum`: 设置队列尺寸
pub const QUEUE_NUM: usize = 0x038;
/// `QueueReady`: 标记队列就绪
pub const QUEUE_READY: usize = 0x044;
/// `QueuePFN` (legacy): 队列页号
pub const QUEUE_PFN: usize = 0x040;
/// `QueueNotify`: 通知设备有新描述符
pub const QUEUE_NOTIFY: usize = 0x050;
/// `InterruptStatus`: 读中断原因
pub const INTERRUPT_STATUS: usize = 0x060;
/// `InterruptACK`: 写以应答中断
pub const INTERRUPT_ACK: usize = 0x064;
/// Status 寄存器
pub const STATUS: usize = 0x070;
/// `QueueDescLow`: 描述符表 phys [31:0]
pub const QUEUE_DESC_LOW: usize = 0x080;
/// `QueueDescHigh`: 描述符表 phys [63:32]
pub const QUEUE_DESC_HIGH: usize = 0x084;
/// `QueueDriverLow`: available ring 物理地址 [31:0]
pub const QUEUE_DRIVER_LOW: usize = 0x090;
/// `QueueDriverHigh`: available ring 物理地址 [63:32]
pub const QUEUE_DRIVER_HIGH: usize = 0x094;
/// `QueueDeviceLow`: used ring 物理地址 [31:0]
pub const QUEUE_DEVICE_LOW: usize = 0x0a0;
/// `QueueDeviceHigh`: used ring 物理地址 [63:32]
pub const QUEUE_DEVICE_HIGH: usize = 0x0a4;
/// `ConfigGeneration`: 配置变更计数器
pub const CONFIG_GENERATION: usize = 0x0fc;

// ── 设备状态位 (Status 寄存器) ──

/// 驱动识别设备存在
pub const STATUS_ACKNOWLEDGE: u32 = 1;
/// 驱动已加载
pub const STATUS_DRIVER: u32 = 2;
/// 驱动 OK
pub const STATUS_DRIVER_OK: u32 = 4;
/// 特性协商完成
pub const STATUS_FEATURES_OK: u32 = 8;
/// 设备需要重置
pub const STATUS_NEEDS_RESET: u32 = 0x40;
/// 设备失败
pub const STATUS_FAILED: u32 = 0x80;

// ── 设备 ID ──

/// `VirtIO` Net 设备 ID
pub const DEVICE_ID_NET: u32 = 1;
/// `VirtIO` Block 设备 ID
pub const DEVICE_ID_BLOCK: u32 = 2;
/// `VirtIO` GPU 设备 ID
pub const DEVICE_ID_GPU: u32 = 16;

// ── MMIO 区域常量 (来自 framework) ──

/// 单设备 MMIO 区域大小
pub const VIRTIO_MMIO_DEVICE_SIZE: usize = 0x200;

// ── Magic 常量 ──

/// `VirtIO` MMIO Magic Value (`0x7472_6976` = "virt" 小端序)
pub const VIRTIO_MAGIC: u32 = 0x7472_6976;

// ── 特性位 ──

/// `VIRTIO_F_VERSION_1` (位 32)
pub const VIRTIO_F_VERSION_1: u64 = 1u64 << 32;

// ============================================================================
// 设备类型枚举
// ============================================================================

/// `VirtIO` 设备类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtioDeviceKind {
    /// Net 设备 (`device_id=1`)
    Net,
    /// Block 设备 (`device_id=2`)
    Block,
    /// GPU 设备 (`device_id=16`)
    Gpu,
    /// 未知/未实现的设备类型
    Unknown(u32),
}

impl VirtioDeviceKind {
    /// 从 `device_id` 转换为设备类型
    pub fn from_id(id: u32) -> Self {
        match id {
            DEVICE_ID_NET => Self::Net,
            DEVICE_ID_BLOCK => Self::Block,
            DEVICE_ID_GPU => Self::Gpu,
            other => Self::Unknown(other),
        }
    }

    /// 该设备类型期望的 virtqueue 数量
    pub fn expected_queue_count(self) -> u32 {
        match self {
            Self::Net => 2,    // RX + TX
            Self::Block => 1,  // 单请求队列
            Self::Gpu => 0,    // 不在本迁移范围
            Self::Unknown(_) => 0,
        }
    }
}

// ============================================================================
// 安全设备句柄
// ============================================================================

/// `VirtIO` MMIO 设备的安全句柄 (services 层)。
///
/// 包装 `IoMem` 提供类型安全的 MMIO 访问, 不暴露 `unsafe`。
/// 生命周期: 与底层 `IoMem` 一致, drop 时 `IoMem` 注销别名。
pub struct VirtioDevice {
    iomem: IoMem,
    device_id: u32,
    vendor_id: u32,
    version: u32,
    kind: VirtioDeviceKind,
    queue_count: u32,
}

impl VirtioDevice {
    /// 探测指定 MMIO 基地址是否有有效 `VirtIO` 设备。
    ///
    /// # 参数
    /// - `mmio_base`: virtio-mmio 设备的基地址 (0x200 步长对齐)
    ///
    /// # 返回
    /// - `Some(VirtioDevice)`: 探测成功, 设备就绪
    /// - `None`: 该位置无设备 / Magic 不匹配 / Version 不支持
    pub fn probe(mmio_base: u64) -> Option<Self> {
        let iomem = IoMem::from_pci_bar(
            PhysAddr::new(mmio_base),
            VIRTIO_MMIO_DEVICE_SIZE,
            "virtio-mmio",
        )
        .ok()?;

        let magic = iomem.read_u32(MAGIC_VALUE);
        if magic != VIRTIO_MAGIC {
            return None;
        }

        let version = iomem.read_u32(VERSION);
        if version != 1 && version != 2 {
            return None;
        }

        let device_id = iomem.read_u32(DEVICE_ID);
        if device_id == 0 {
            return None;
        }

        let vendor_id = iomem.read_u32(VENDOR_ID);
        let kind = VirtioDeviceKind::from_id(device_id);
        let queue_count = kind.expected_queue_count();

        Some(Self {
            iomem,
            device_id,
            vendor_id,
            version,
            kind,
            queue_count,
        })
    }

    /// 设备 ID (1=net, 2=blk, ...)
    #[inline(always)]
    pub fn device_id(&self) -> u32 {
        self.device_id
    }

    /// 厂商 ID
    #[inline(always)]
    pub fn vendor_id(&self) -> u32 {
        self.vendor_id
    }

    /// `VirtIO` 版本 (1=legacy, 2=modern)
    #[inline(always)]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// 是否为 legacy 模式 (version == 1)
    #[inline(always)]
    pub fn is_legacy(&self) -> bool {
        self.version == 1
    }

    /// 设备类型
    #[inline(always)]
    pub fn kind(&self) -> VirtioDeviceKind {
        self.kind
    }

    /// MMIO 基地址
    #[inline(always)]
    pub fn mmio_base(&self) -> u64 {
        self.iomem.phys().as_u64()
    }

    /// 期望的 virtqueue 数量 (按设备类型)
    #[inline(always)]
    pub fn expected_queue_count(&self) -> u32 {
        self.queue_count
    }

    // ── 基础 MMIO 读/写 ──

    /// 读 32 位 MMIO 寄存器
    #[inline(always)]
    pub fn read32(&self, offset: usize) -> u32 {
        self.iomem.read_u32(offset)
    }

    /// 写 32 位 MMIO 寄存器
    #[inline(always)]
    pub fn write32(&self, offset: usize, val: u32) {
        self.iomem.write_u32(offset, val);
    }

    /// 读 64 位跨 Low/High 寄存器
    pub fn read64(&self, low_off: usize, high_off: usize) -> u64 {
        let lo = u64::from(self.read32(low_off));
        let hi = u64::from(self.read32(high_off));
        lo | (hi << 32)
    }

    /// 写 64 位跨 Low/High 寄存器
    pub fn write64(&self, low_off: usize, high_off: usize, val: u64) {
        self.write32(low_off, (val & 0xFFFF_FFFF) as u32);
        self.write32(high_off, (val >> 32) as u32);
    }

    // ── 设备状态机 (Status 寄存器) ──

    /// 读 Status 寄存器
    #[inline(always)]
    pub fn status(&self) -> u32 {
        self.read32(STATUS)
    }

    /// 写 Status 寄存器
    #[inline(always)]
    pub fn set_status(&self, val: u32) {
        self.write32(STATUS, val);
    }

    /// 重置设备 (status=0)
    pub fn reset(&self) {
        self.write32(STATUS, 0);
        // 内存屏障: 确保设备观察到 reset
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }

    /// 进入 ACKNOWLEDGE 状态
    pub fn ack(&self) {
        self.write32(STATUS, STATUS_ACKNOWLEDGE);
    }

    /// 进入 DRIVER 状态
    pub fn set_driver(&self) {
        self.write32(STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);
    }

    /// 进入 `FEATURES_OK` 状态
    ///
    /// 返回 true 表示设备接受特性协商
    pub fn features_ok(&self) -> bool {
        self.write32(
            STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK,
        );
        self.read32(STATUS) & STATUS_FEATURES_OK != 0
    }

    /// 进入 `DRIVER_OK` 状态 (设备上线)
    pub fn set_driver_ok(&self) {
        self.write32(
            STATUS,
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
        );
    }

    // ── 特性协商 ──

    /// 读设备特性 (全 64 位)
    ///
    /// 内部两次 MMIO 读: sel=0 (low) + sel=1 (high)
    pub fn device_features(&self) -> u64 {
        self.write32(DEVICE_FEATURES_SEL, 0);
        let lo = u64::from(self.read32(DEVICE_FEATURES));
        self.write32(DEVICE_FEATURES_SEL, 1);
        let hi = u64::from(self.read32(DEVICE_FEATURES));
        lo | (hi << 32)
    }

    /// 写驱动特性 (全 64 位)
    pub fn set_driver_features(&self, features: u64) {
        self.write32(DRIVER_FEATURES_SEL, 1);
        self.write32(DRIVER_FEATURES, (features >> 32) as u32);
        self.write32(DRIVER_FEATURES_SEL, 0);
        self.write32(DRIVER_FEATURES, features as u32);
    }

    // ── Virtqueue 配置 ──

    /// 选择 virtqueue 索引 (后续 `QUEUE_NUM_MAX` 等作用于该队列)
    pub fn select_queue(&self, vq_index: u16) {
        self.write32(QUEUE_SEL, u32::from(vq_index));
    }

    /// 读选中队列的最大尺寸
    pub fn queue_num_max(&self) -> u32 {
        self.read32(QUEUE_NUM_MAX)
    }

    /// 设置选中队列的尺寸
    pub fn set_queue_num(&self, size: u32) {
        self.write32(QUEUE_NUM, size);
    }

    /// 标记选中队列为 ready
    pub fn set_queue_ready(&self) {
        self.write32(QUEUE_READY, 1);
    }

    /// 通知设备: 选中队列有新描述符
    pub fn notify_queue(&self, vq_index: u16) {
        self.write32(QUEUE_NOTIFY, u32::from(vq_index));
    }

    /// 设置选中队列的描述符/avail/used 物理地址 (modern 模式)
    pub fn setup_queue_addrs(&self, desc_paddr: u64, avail_paddr: u64, used_paddr: u64) {
        self.write64(QUEUE_DESC_LOW, QUEUE_DESC_HIGH, desc_paddr);
        self.write64(QUEUE_DRIVER_LOW, QUEUE_DRIVER_HIGH, avail_paddr);
        self.write64(QUEUE_DEVICE_LOW, QUEUE_DEVICE_HIGH, used_paddr);
    }

    /// 设置选中队列的 PFN (legacy 模式)
    pub fn setup_queue_legacy(&self, paddr: u64) {
        let pfn = (paddr >> 12) as u32;
        self.write32(QUEUE_PFN, pfn);
    }

    // ── 中断 ──

    /// 读中断状态寄存器
    pub fn interrupt_status(&self) -> u32 {
        self.read32(INTERRUPT_STATUS)
    }

    /// 应答中断
    pub fn ack_interrupt(&self, mask: u32) {
        self.write32(INTERRUPT_ACK, mask);
    }

    // ── 设备配置空间 (offset 0x100+) ──

    /// 读 32 位设备配置寄存器
    #[inline(always)]
    pub fn read_config32(&self, offset: usize) -> u32 {
        self.read32(0x100 + offset)
    }

    /// 读 64 位设备配置寄存器
    pub fn read_config64(&self, offset: usize) -> u64 {
        self.read64(0x100 + offset, 0x100 + offset + 4)
    }

    /// 读配置变更计数器 (`CONFIG_GENERATION`)
    pub fn config_generation(&self) -> u32 {
        self.read32(CONFIG_GENERATION)
    }
}

// ============================================================================
// 区域扫描
// ============================================================================

/// 扫描 virtio-mmio 区域, 返回发现的设备列表。
///
/// 从 [`VIRTIO_MMIO_BASE`] 开始, 以 [`VIRTIO_MMIO_STRIDE`] 为步长,
/// 最多探测 [`VIRTIO_MMIO_MAX_DEVICES`] 个 slot.
///
/// # 返回
/// 包含 0..N 个已发现的 `VirtIO` 设备 (Net + Block + Gpu 混合)
pub fn probe_all() -> alloc::vec::Vec<VirtioDevice> {
    let mut devices = alloc::vec::Vec::new();
    for i in 0..VIRTIO_MMIO_MAX_DEVICES {
        let base = VIRTIO_MMIO_BASE + u64::from(i) * VIRTIO_MMIO_STRIDE;
        if let Some(dev) = VirtioDevice::probe(base) {
            devices.push(dev);
        }
    }
    devices
}
