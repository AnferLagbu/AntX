#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! VirtIO 块设备驱动 — services 层 (Phase 2.1.3)
//!
//! 通过 `VirtioDevice` (transport.rs) 提供 100% safe 的块设备初始化与配置路径。
//! VirtQueue 操作通过 framework 层安全 API 完成。
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 所有 MMIO 读/写通过 `VirtioDevice` 安全代理
//! - **请求格式**: 定义 BlkRequest / 状态码 / 配置偏移, 供 framework I/O 路径使用
//! - **初始化序列**: Reset → Ack → Driver → Feature Negotiate → Features_OK → Queue Setup → Driver_OK
//!
//! ## 与 framework 的分工
//!
//! - **services (本文件)**: 初始化序列, 特性协商, 配置空间读取, 请求格式定义
//! - **framework**: VirtQueue 分配与 DMA 缓冲区管理 (需要 unsafe 指针操作)
//!
//! 评估日期: 2026-06-04
//! Phase 2.1.3 任务: VirtIO-blk 块设备迁移

use super::transport::{VirtioDevice, DEVICE_ID_BLOCK, VIRTIO_F_VERSION_1};
use crate::slog_info;
use crate::slog_warn;

// ============================================================================
// VirtIO-blk 常量
// ============================================================================

/// 块设备扇区大小 (字节)
pub const BLK_SECTOR_SIZE: usize = 512;

// ── Feature bits ──

/// VIRTIO_BLK_F_SIZE_MAX: 驱动可指定最大缓冲区大小
pub const VIRTIO_BLK_F_SIZE_MAX: u64 = 1 << 1;
/// VIRTIO_BLK_F_SEG_MAX: 最大 scatter-gather 段数
pub const VIRTIO_BLK_F_SEG_MAX: u64 = 1 << 2;
/// VIRTIO_BLK_F_GEOMETRY: 几何信息 (cylinders/heads/sectors)
pub const VIRTIO_BLK_F_GEOMETRY: u64 = 1 << 4;
/// VIRTIO_BLK_F_BLK_SIZE: 块大小可配置
pub const VIRTIO_BLK_F_BLK_SIZE: u64 = 1 << 6;
/// VIRTIO_BLK_F_TOPOLOGY: 拓扑信息 (topology, aligned I/O)
pub const VIRTIO_BLK_F_TOPOLOGY: u64 = 1 << 10;
/// VIRTIO_BLK_F_CONFIG_WCE: 可配置 write-back 缓存
pub const VIRTIO_BLK_F_CONFIG_WCE: u64 = 1 << 11;

// ── 请求类型 ──

/// 读请求 (设备 → 驱动)
pub const VIRTIO_BLK_T_IN: u32 = 0;
/// 写请求 (驱动 → 设备)
pub const VIRTIO_BLK_T_OUT: u32 = 1;
/// 刷新请求
pub const VIRTIO_BLK_T_FLUSH: u32 = 4;

// ── 状态码 ──

/// 请求成功
pub const VIRTIO_BLK_S_OK: u8 = 0;
/// I/O 错误
pub const VIRTIO_BLK_S_IOERR: u8 = 1;
/// 不支持的请求
pub const VIRTIO_BLK_S_UNSUPP: u8 = 2;

// ── 配置空间偏移 (相对于 0x100) ──

/// capacity_lo: 容量低 32 位 (512 字节扇区)
pub const BLK_CONFIG_CAPACITY_LO: usize = 0x00;
/// capacity_hi: 容量高 32 位
pub const BLK_CONFIG_CAPACITY_HI: usize = 0x04;
/// size_max: 最大缓冲区大小
pub const BLK_CONFIG_SIZE_MAX: usize = 0x08;
/// seg_max: 最大段数
pub const BLK_CONFIG_SEG_MAX: usize = 0x0C;
/// geometry_cylinders: 几何 — 偏移 0x10 包含 cylinders(u16) + heads(u8) + sectors(u8)
pub const BLK_CONFIG_GEOM_CYL: usize = 0x10;
/// blk_size: 块大小
pub const BLK_CONFIG_BLK_SIZE: usize = 0x14;

// ============================================================================
// 请求结构体
// ============================================================================

/// VirtIO 块请求头 (小端, 与设备 DMA 格式一致).
///
/// 描述符链布局:
///   desc[0]: BlkRequest (8 字节, 设备读)
///   desc[1]: 数据缓冲区 (IN 时设备写, OUT 时设备读)
///   desc[2]: 状态字节 (1 字节, 设备写)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BlkRequest {
    /// 请求类型: 0=读, 1=写
    pub req_type: u32,
    /// 保留字段
    pub reserved: u32,
    /// LBA 扇区号 (小端)
    pub sector: u64,
}

impl BlkRequest {
    /// 创建读请求
    pub fn read(lba: u64) -> Self {
        Self {
            req_type: VIRTIO_BLK_T_IN.to_le(),
            reserved: 0,
            sector: lba.to_le(),
        }
    }

    /// 创建写请求
    pub fn write(lba: u64) -> Self {
        Self {
            req_type: VIRTIO_BLK_T_OUT.to_le(),
            reserved: 0,
            sector: lba.to_le(),
        }
    }

    /// 创建刷新请求
    pub fn flush() -> Self {
        Self {
            req_type: VIRTIO_BLK_T_FLUSH.to_le(),
            reserved: 0,
            sector: 0,
        }
    }

    /// 请求头大小 (字节)
    pub const fn header_size() -> usize {
        core::mem::size_of::<Self>()
    }
}

// ============================================================================
// 安全驱动逻辑
// ============================================================================

/// VirtIO 块设备安全驱动 (services 层, 0 unsafe)。
///
/// 封装 VirtIO 块设备的初始化序列与配置读取, 通过 `VirtioDevice` 安全代理访问 MMIO。
/// DMA 缓冲区管理与 VirtQueue 操作保留在 framework 层。
///
/// ## 初始化流程
///
/// 1. `VirtioBlkDriver::new(device)` — 验证设备 ID, 执行初始化序列
/// 2. 读取配置空间获取容量信息
/// 3. framework: 分配 VirtQueue 并配置
/// 4. `set_driver_ok()` — 设备进入 live 状态
pub struct VirtioBlkDriver {
    /// MMIO 设备传输代理
    device: VirtioDevice,
    /// 以 512 字节扇区为单位的总容量
    capacity_sectors: u64,
    /// 设备支持的特性位
    negotiated_features: u64,
}

impl VirtioBlkDriver {
    /// 创建并初始化 VirtIO 块设备驱动。
    ///
    /// 验证设备 ID 为块设备, 执行完整初始化序列 (Reset → Ack → Driver → Feature → Features_OK)。
    /// 返回 `Some(VirtioBlkDriver)` 表示设备就绪, 可继续配置 VirtQueue。
    ///
    /// # 参数
    /// - `device`: 已探测到的 VirtIO 设备 (device_id 必须为 DEVICE_ID_BLOCK)
    pub fn new(device: VirtioDevice) -> Option<Self> {
        if device.device_id() != DEVICE_ID_BLOCK {
            slog_warn!(
                Driver,
                "virtio-blk: 期望 device_id={}, 实际={}",
                DEVICE_ID_BLOCK,
                device.device_id()
            );
            return None;
        }

        slog_info!(
            Driver,
            "virtio-blk: 初始化 at {:#x} (version={})",
            device.mmio_base(),
            device.version()
        );

        // Step 1: Reset
        device.reset();

        // Step 2: ACKNOWLEDGE
        device.ack();

        // Step 3: DRIVER
        device.set_driver();

        // Step 4: 特性协商
        let dev_features = device.device_features();
        slog_info!(Driver, "virtio-blk: 设备特性={:#018x}", dev_features);

        // 驱动请求的特性: VERSION_1 + 基础块设备特性
        let mut driver_features = VIRTIO_F_VERSION_1;
        // 可选: 协商 blk_size 如果设备支持
        if dev_features & VIRTIO_BLK_F_BLK_SIZE != 0 {
            driver_features |= VIRTIO_BLK_F_BLK_SIZE;
        }
        // 可选: 协商 TOPOLOGY 如果设备支持
        if dev_features & VIRTIO_BLK_F_TOPOLOGY != 0 {
            driver_features |= VIRTIO_BLK_F_TOPOLOGY;
        }

        device.set_driver_features(driver_features);
        slog_info!(
            Driver,
            "virtio-blk: 驱动特性={:#018x}",
            driver_features
        );

        // Step 5: FEATURES_OK
        if !device.features_ok() {
            slog_warn!(Driver, "virtio-blk: FEATURES_OK 被拒绝");
            return None;
        }

        // 读取配置空间: 容量
        let cap_lo = device.read_config32(BLK_CONFIG_CAPACITY_LO) as u64;
        let cap_hi = device.read_config32(BLK_CONFIG_CAPACITY_HI) as u64;
        let capacity = cap_lo | (cap_hi << 32);

        slog_info!(
            Driver,
            "virtio-blk: 容量={} 扇区 ({:.1} MB)",
            capacity,
            (capacity * 512) as f64 / (1024.0 * 1024.0)
        );

        Some(Self {
            device,
            capacity_sectors: capacity,
            negotiated_features: driver_features,
        })
    }

    /// 设置 DRIVER_OK (设备进入 live 状态).
    ///
    /// 必须在所有 virtqueue 配置完成后调用.
    pub fn set_driver_ok(&self) {
        self.device.set_driver_ok();
        slog_info!(Driver, "virtio-blk: DRIVER_OK 已设置");
    }

    /// 获取 MMIO 设备引用 (用于 VirtQueue 配置).
    pub fn device(&self) -> &VirtioDevice {
        &self.device
    }

    /// 获取以扇区为单位的总容量.
    pub fn capacity_sectors(&self) -> u64 {
        self.capacity_sectors
    }

    /// 获取协商后的特性位.
    pub fn negotiated_features(&self) -> u64 {
        self.negotiated_features
    }

    /// 检查是否协商了指定特性.
    pub fn has_feature(&self, feature: u64) -> bool {
        self.negotiated_features & feature != 0
    }

    // ── 配置空间读取 (安全, 通过 VirtioDevice) ──

    /// 读取块大小 (仅当 VIRTIO_BLK_F_BLK_SIZE 被协商时有效).
    pub fn block_size(&self) -> u32 {
        self.device.read_config32(BLK_CONFIG_BLK_SIZE)
    }

    /// 读取最大缓冲区大小 (仅当 VIRTIO_BLK_F_SIZE_MAX 被协商时有效).
    pub fn size_max(&self) -> u32 {
        self.device.read_config32(BLK_CONFIG_SIZE_MAX)
    }

    /// 读取最大 scatter-gather 段数 (仅当 VIRTIO_BLK_F_SEG_MAX 被协商时有效).
    pub fn seg_max(&self) -> u32 {
        self.device.read_config32(BLK_CONFIG_SEG_MAX)
    }

    /// 读取几何信息 (cylinders, heads, sectors).
    ///
    /// VirtIO-blk 配置空间偏移 0x10: cylinders(u16) + heads(u8) + sectors(u8)
    /// 通过 read_config32 读取对齐的 32 位, 再拆分.
    pub fn geometry(&self) -> BlkGeometry {
        // 偏移 0x10 包含 cylinders(u16) + heads(u8) + sectors(u8), 共 4 字节
        let raw = self.device.read_config32(BLK_CONFIG_GEOM_CYL);
        let cylinders = (raw & 0xFFFF) as u16;
        let heads = ((raw >> 16) & 0xFF) as u8;
        let sectors = ((raw >> 24) & 0xFF) as u8;
        BlkGeometry {
            cylinders,
            heads,
            sectors,
        }
    }

    // ── 中断处理 ──

    /// 读取并应答中断状态 (写 1 清除).
    pub fn ack_interrupt(&self) -> u32 {
        let status = self.device.interrupt_status();
        if status != 0 {
            self.device.ack_interrupt(status);
        }
        status
    }

    // ── 队列配置辅助 (通过 VirtioDevice MMIO) ──

    /// 配置指定 virtqueue 的 MMIO 寄存器.
    ///
    /// # 参数
    /// - `vq_index`: virtqueue 索引 (blk 通常为 0)
    /// - `desc_paddr`: 描述符表物理地址
    /// - `avail_paddr`: available ring 物理地址
    /// - `used_paddr`: used ring 物理地址
    ///
    /// # 返回
    /// - `Ok(max_size)`: 队列最大尺寸
    /// - `Err(())`: 配置失败
    pub fn setup_queue(
        &self,
        vq_index: u16,
        desc_paddr: u64,
        avail_paddr: u64,
        used_paddr: u64,
    ) -> Result<u32, ()> {
        self.device.select_queue(vq_index);
        let max_size = self.device.queue_num_max();
        slog_info!(
            Driver,
            "virtio-blk: vq{} max_size={}",
            vq_index,
            max_size
        );
        self.device.setup_queue_addrs(desc_paddr, avail_paddr, used_paddr);
        self.device.set_queue_ready();
        slog_info!(Driver, "virtio-blk: vq{} ready", vq_index);
        Ok(max_size)
    }

    /// 通知设备: 指定队列有新描述符.
    pub fn notify(&self, vq_index: u16) {
        self.device.notify_queue(vq_index);
    }
}

/// 块设备几何信息.
#[derive(Debug, Clone, Copy)]
pub struct BlkGeometry {
    /// 柱面数
    pub cylinders: u16,
    /// 磁头数
    pub heads: u8,
    /// 每磁道扇区数
    pub sectors: u8,
}
