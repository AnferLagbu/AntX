#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 MMIO 操作通过 `E1000Io` 安全代理,
//! 替代原始 e1000 驱动中的裸 `mmio_read32`/`mmio_write32` (28 unsafe 块)。
//!
//! ## 迁移路径
//!
//! 原始驱动: `kernel::driver::net::e1000::E1000Device` — 28 个 unsafe 块
//! Services 适配: 通过 `IoMem` 封装 MMIO + 安全初始化序列, 消除 unsafe。
//! DMA 描述符环管理保留在 framework 层 (需要 unsafe 指针操作),
//! Chitin FFI 回调保留在 framework 层 (需要 unsafe 指针转换)。
//!
//! ## 职责
//!
//! - `E1000Io`: E1000 MMIO 寄存器安全读写
//! - `E1000Driver`: 安全驱动逻辑 (初始化序列/寄存器配置/中断应答)
//! - DMA 环管理/发送/接收/中断处理: 在 framework 层

use crate::kernel::framework::iomem::IoMem;
use crate::kernel::framework::mm::PhysAddr;
use crate::kernel::framework::driver::net::e1000::{TxRing, RxRing};
use crate::kernel::services::error::KernelError;

// Services 层安全日志宏 (无 unsafe 展开)
use crate::slog_info;
use crate::slog_warn;

// ============================================================================
// E1000 寄存器常量
// ============================================================================

// 控制寄存器
const E1000_CTRL: u32 = 0x0000;
const E1000_CTRL_RST: u32 = 1 << 31;
const E1000_CTRL_SLU: u32 = 1 << 6;
const E1000_CTRL_ASDE: u32 = 1 << 5;
const E1000_CTRL_SPEED_1000: u32 = 2 << 8;
const E1000_CTRL_FRCDPX: u32 = 1 << 14;
const E1000_CTRL_FD: u32 = 1 << 0;
const E1000_CTRL_FRCSPD: u32 = 1 << 11;

// 状态寄存器
const E1000_STATUS: u32 = 0x0008;
const E1000_STATUS_LU: u32 = 1 << 1;
const E1000_STATUS_FD: u32 = 1 << 0;
const E1000_STATUS_SPEED_1000: u32 = 2 << 6;
const E1000_STATUS_SPEED_100: u32 = 1 << 6;

// EEPROM 寄存器
const E1000_EERD: u32 = 0x0014;
const E1000_EERD_START: u32 = 1 << 0;
const E1000_EERD_DONE: u32 = 1 << 4;

// 接收控制
const E1000_RCTL: u32 = 0x0100;
const E1000_RCTL_EN: u32 = 1 << 1;
const E1000_RCTL_SBP: u32 = 1 << 2;
const E1000_RCTL_UPE: u32 = 1 << 3;
const E1000_RCTL_MPE: u32 = 1 << 4;
const E1000_RCTL_BAM: u32 = 1 << 15;
const E1000_RCTL_SECRC: u32 = 1 << 26;
const E1000_RCTL_BSIZE_2048: u32 = 1 << 25;

// 发送控制
const E1000_TCTL: u32 = 0x0400;
const E1000_TCTL_EN: u32 = 1 << 1;
const E1000_TCTL_PSP: u32 = 1 << 3;
const E1000_TCTL_COLD_FD: u32 = 0x40 << 12;
const E1000_TCTL_CT_FD: u32 = 0x10 << 4;

// TX 描述符环寄存器
const E1000_TDBAL: u32 = 0x3800;
const E1000_TDBAH: u32 = 0x3804;
const E1000_TDLEN: u32 = 0x3808;
const E1000_TDH: u32 = 0x3810;
const E1000_TDT: u32 = 0x3818;

// RX 描述符环寄存器
const E1000_RDBAL: u32 = 0x2800;
const E1000_RDBAH: u32 = 0x2804;
const E1000_RDLEN: u32 = 0x2808;
const E1000_RDH: u32 = 0x2810;
/// RX tail 寄存器偏移 (framework 层 dump_regs 使用)
pub const E1000_RDT: u32 = 0x2818;

// 中断控制
const E1000_IMC: u32 = 0x00D8;
const E1000_ICR: u32 = 0x00C0;
const E1000_IMS: u32 = 0x00D0;
/// 中断原因: 链路状态变化
pub const E1000_ICR_LSC: u32 = 1 << 2;
/// 中断原因: RX 描述符最小阈值
pub const E1000_ICR_RXDMT0: u32 = 1 << 4;
/// 中断原因: 接收缓冲区溢出
pub const E1000_ICR_RXO: u32 = 1 << 6;
/// 中断原因: RX 定时器
pub const E1000_ICR_RXT0: u32 = 1 << 7;

// IPG (Inter-Packet Gap)
const E1000_IPG: u32 = 0x00B0;

// MAC 地址寄存器
const E1000_RAL0: u32 = 0x5400;
const E1000_RAH0: u32 = 0x5404;
const E1000_RAH_AV: u32 = 1 << 31;

// 超时常量
const E1000_TIMEOUT: u32 = 100000;

// ============================================================================
// DMA 环大小常量 (framework 层也使用)
// ============================================================================

/// TX 描述符环大小
pub const E1000_TX_RING_SIZE: usize = 64;
/// RX 描述符环大小
pub const E1000_RX_RING_SIZE: usize = 128;
/// RX 缓冲区大小
pub const E1000_RX_BUFFER_SIZE: usize = 2048;

// ============================================================================
// 描述符结构体 (硬件格式, repr(C))
// ============================================================================

/// E1000 TX 描述符 (16 字节, 与硬件 DMA 格式一致)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct E1000TxDesc {
    pub addr: u64,
    pub length: u16,
    pub cso: u8,
    pub cmd: u8,
    pub status: u8,
    pub css: u8,
    pub special: u16,
}

/// E1000 RX 描述符 (16 字节, 与硬件 DMA 格式一致)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct E1000RxDesc {
    pub addr: u64,
    pub length: u16,
    pub checksum: u16,
    pub status: u8,
    pub errors: u8,
    pub special: u16,
}

// ============================================================================
// 描述符状态/命令常量
// ============================================================================

/// TX 描述符命令: End Of Packet
pub const E1000_TXD_CMD_EOP: u8 = 1 << 0;
/// TX 插述符命令: Insert FCS
pub const E1000_TXD_CMD_IFCS: u8 = 1 << 1;
/// TX 描述符命令: Report Status
pub const E1000_TXD_CMD_RS: u8 = 1 << 3;
/// TX 描述符状态: Descriptor Done
pub const E1000_TXD_STAT_DD: u8 = 1 << 0;

/// RX 描述符状态: Descriptor Done
pub const E1000_RXD_STAT_DD: u8 = 1 << 0;
/// RX 描述符错误: CRC Error
pub const E1000_RXD_ERR_CE: u8 = 1 << 0;
/// RX 描述符错误: Symbol Error
pub const E1000_RXD_ERR_SE: u8 = 1 << 1;
/// RX 描述符错误: Sequence Error
pub const E1000_RXD_ERR_SEQ: u8 = 1 << 2;
/// RX 描述符错误: Receive Error
pub const E1000_RXD_ERR_RXE: u8 = 1 << 3;

// ============================================================================
// 安全 MMIO 访问器
// ============================================================================

/// 安全的 E1000 MMIO 访问器。
///
/// 包装 `IoMem`, 提供所有 E1000 寄存器的类型安全读写。
/// services 层通过此结构安全访问 E1000 网卡, 替代裸指针 `mmio_base: *mut u8`。
pub struct E1000Io {
    mmio: IoMem,
}

impl E1000Io {
    /// 从物理地址创建 E1000 MMIO 访问器。
    ///
    /// # 参数
    /// - `phys`: E1000 BAR0 物理地址 (来自 PCI 枚举)
    /// - `len`: MMIO 区域大小 (通常 128KB)
    pub fn new(phys: PhysAddr, len: usize) -> Result<Self, KernelError> {
        let mmio = IoMem::from_pci_bar(phys, len, "e1000-bar0")
            .map_err(|_| KernelError::Io)?;
        Ok(Self { mmio })
    }

    // ── 寄存器读写 ──

    /// 读取 32 位寄存器
    #[inline(always)]
    pub fn read32(&self, reg: u32) -> u32 {
        self.mmio.read_u32(reg as usize)
    }

    /// 写入 32 位寄存器
    #[inline(always)]
    pub fn write32(&self, reg: u32, val: u32) {
        self.mmio.write_u32(reg as usize, val)
    }

    // ── EEPROM 读取 ──

    /// 通过 EERD 寄存器读取 EEPROM 字。
    ///
    /// 轮询 EERD.DONE 位, 带超时保护。
    pub fn eeprom_read(&self, addr: u8) -> u16 {
        self.write32(E1000_EERD, ((addr as u32) << 2) | E1000_EERD_START);
        let mut timeout: u32 = 0;
        while timeout < E1000_TIMEOUT {
            let val = self.read32(E1000_EERD);
            if val & E1000_EERD_DONE != 0 {
                return ((val >> 16) & 0xFFFF) as u16;
            }
            timeout += 1;
            core::hint::spin_loop();
        }
        0xFFFF
    }

    // ── 中断 ──

    /// 读取中断原因并应答 (write-1-to-clear)
    pub fn irq_ack(&self) -> u32 {
        let icr = self.read32(E1000_ICR);
        self.write32(E1000_ICR, icr);
        icr
    }

    /// 清除所有待处理中断
    pub fn irq_disable_all(&self) {
        self.write32(E1000_IMC, 0xFFFFFFFF);
    }

    /// 启用指定中断
    pub fn irq_enable(&self, mask: u32) {
        self.write32(E1000_IMS, mask);
    }

    /// 读取中断状态
    pub fn icr(&self) -> u32 {
        self.read32(E1000_ICR)
    }

    /// 读取中断掩码
    pub fn ims(&self) -> u32 {
        self.read32(E1000_IMS)
    }

    // ── 链路状态 ──

    /// 链路是否 UP
    pub fn link_is_up(&self) -> bool {
        self.read32(E1000_STATUS) & E1000_STATUS_LU != 0
    }

    /// 读取链路速度与双工状态
    pub fn link_status(&self) -> (&'static str, &'static str) {
        let status = self.read32(E1000_STATUS);
        let speed = if status & E1000_STATUS_SPEED_1000 != 0 {
            "1000"
        } else if status & E1000_STATUS_SPEED_100 != 0 {
            "100"
        } else {
            "10"
        };
        let duplex = if status & E1000_STATUS_FD != 0 { "FD" } else { "HD" };
        (speed, duplex)
    }

    // ── 收发描述符环配置 ──

    /// 设置 RX 描述符环物理基地址
    pub fn set_rx_base(&self, phys: u64) {
        self.write32(E1000_RDBAL, phys as u32);
        self.write32(E1000_RDBAH, (phys >> 32) as u32);
    }

    /// 设置 TX 描述符环物理基地址
    pub fn set_tx_base(&self, phys: u64) {
        self.write32(E1000_TDBAL, phys as u32);
        self.write32(E1000_TDBAH, (phys >> 32) as u32);
    }

    /// 设置 RX 描述符环长度 (字节)
    pub fn set_rx_len(&self, len: u32) {
        self.write32(E1000_RDLEN, len);
    }

    /// 设置 TX 描述符环长度 (字节)
    pub fn set_tx_len(&self, len: u32) {
        self.write32(E1000_TDLEN, len);
    }

    /// 读取 RX head 指针 (硬件更新)
    pub fn rx_head(&self) -> u32 {
        self.read32(E1000_RDH)
    }

    /// 设置 RX tail 指针 (通知硬件接收范围)
    pub fn set_rx_tail(&self, val: u32) {
        self.write32(E1000_RDT, val);
    }

    /// 读取 TX head 指针 (硬件更新)
    pub fn tx_head(&self) -> u32 {
        self.read32(E1000_TDH)
    }

    /// 设置 TX tail 指针 (通知硬件发送范围)
    pub fn set_tx_tail(&self, val: u32) {
        self.write32(E1000_TDT, val);
    }

    /// 设置 RX head 指针 (初始化用, 硬件通常只读)
    pub fn set_rx_head_raw(&self, val: u32) {
        self.write32(E1000_RDH, val);
    }

    /// 设置 TX head 指针 (初始化用, 硬件通常只读)
    pub fn set_tx_head_raw(&self, val: u32) {
        self.write32(E1000_TDH, val);
    }

    // ── 控制寄存器 ──

    /// 读取控制寄存器
    pub fn ctrl(&self) -> u32 {
        self.read32(E1000_CTRL)
    }

    /// 写入控制寄存器
    pub fn set_ctrl(&self, val: u32) {
        self.write32(E1000_CTRL, val);
    }

    /// 读取接收控制寄存器
    pub fn rx_ctl(&self) -> u32 {
        self.read32(E1000_RCTL)
    }

    /// 写入接收控制寄存器
    pub fn set_rx_ctl(&self, val: u32) {
        self.write32(E1000_RCTL, val);
    }

    /// 读取发送控制寄存器
    pub fn tx_ctl(&self) -> u32 {
        self.read32(E1000_TCTL)
    }

    /// 写入发送控制寄存器
    pub fn set_tx_ctl(&self, val: u32) {
        self.write32(E1000_TCTL, val);
    }

    // ── MAC 地址 ──

    /// 写入 MAC 地址到 RAL0/RAH0 寄存器
    pub fn set_mac(&self, mac: &[u8; 6]) {
        let ral = (mac[0] as u32)
            | ((mac[1] as u32) << 8)
            | ((mac[2] as u32) << 16)
            | ((mac[3] as u32) << 24);
        let rah = (mac[4] as u32) | ((mac[5] as u32) << 8) | E1000_RAH_AV;
        self.write32(E1000_RAL0, ral);
        self.write32(E1000_RAH0, rah);
    }

    // ── IPG ──

    /// 设置 Inter-Packet Gap
    pub fn set_ipg(&self, val: u32) {
        self.write32(E1000_IPG, val);
    }
}

// ============================================================================
// 安全驱动逻辑
// ============================================================================

/// E1000 安全驱动 (services 层, 0 unsafe)。
///
/// 封装所有 E1000 硬件寄存器操作, 通过 `E1000Io` 安全代理访问 MMIO。
/// DMA 描述符环管理不在本结构内 — 保留在 framework 层。
///
/// ## 初始化流程
///
/// 1. `E1000Driver::new(io, mac, irq)` — 创建驱动实例
/// 2. `reset_and_detect_link()` — 复位硬件并等待链路 UP
/// 3. framework: `setup_descriptor_rings` — 分配 DMA 环并配置基地址
/// 4. `complete_init()` — 启用收发/中断, 配置 MAC
pub struct E1000Driver {
    io: E1000Io,
    pub mac: [u8; 6],
    pub irq: u8,
    initialized: bool,
    /// TX 描述符环 (安全包装, 由 framework 层分配)
    tx_ring: Option<TxRing>,
    /// RX 描述符环 (安全包装, 由 framework 层分配)
    rx_ring: Option<RxRing>,
}

impl E1000Driver {
    /// 从 MMIO 访问器、MAC 地址和 IRQ 创建驱动实例。
    pub fn new(io: E1000Io, mac: [u8; 6], irq: u8) -> Self {
        Self {
            io,
            mac,
            irq,
            initialized: false,
            tx_ring: None,
            rx_ring: None,
        }
    }

    /// 获取底层 MMIO 访问器引用 (供 framework 层直接寄存器访问, 如 dump_regs)。
    pub fn io(&self) -> &E1000Io {
        &self.io
    }

    /// 硬件复位与链路检测。
    ///
    /// 执行 E1000 软复位, 清除中断, 配置链路/速率/双工, 并等待链路 UP。
    /// 应在 DMA 环分配前调用。
    pub fn reset_and_detect_link(&mut self) -> Result<(), KernelError> {
        // 1. 软复位: 写 CTRL.RST, 轮询直到清零
        self.io.write32(E1000_CTRL, E1000_CTRL_RST);
        let mut timeout = 0u32;
        while timeout < E1000_TIMEOUT {
            if self.io.read32(E1000_CTRL) & E1000_CTRL_RST == 0 {
                break;
            }
            timeout += 1;
            core::hint::spin_loop();
        }
        if timeout >= E1000_TIMEOUT {
            return Err(KernelError::Io);
        }

        // 2. 清除所有待处理中断
        self.io.irq_disable_all();

        // 3. 配置控制寄存器: SLU + ASDE + 强制 1000Mbps + 强制全双工
        let ctrl = self.io.read32(E1000_CTRL);
        let new_ctrl = (ctrl & !E1000_CTRL_RST)
            | E1000_CTRL_SLU
            | E1000_CTRL_ASDE
            | E1000_CTRL_FRCSPD
            | E1000_CTRL_SPEED_1000
            | E1000_CTRL_FRCDPX
            | E1000_CTRL_FD;
        self.io.write32(E1000_CTRL, new_ctrl);

        // 4. 等待链路 UP (500k 次 spin_loop)
        let mut link_ready = false;
        for _ in 0..500000 {
            if self.io.link_is_up() {
                link_ready = true;
                break;
            }
            core::hint::spin_loop();
        }

        if !link_ready {
            slog_warn!(Net, "e1000: link not ready, continuing anyway");
        } else {
            let (speed, _duplex) = self.io.link_status();
            slog_info!(Net, "e1000: NIC Link is Up {} Mbps Full Duplex", speed);
        }

        Ok(())
    }

    /// 完成初始化: TCTL/RCTL/MAC/IPG/IMS。
    ///
    /// 应在 DMA 环分配并配置基地址后调用。
    pub fn complete_init(&mut self) {
        // 发送控制: 启用 + PSP + 全双工碰撞距离 + 碰撞阈值
        let tctl = E1000_TCTL_EN | E1000_TCTL_PSP | E1000_TCTL_COLD_FD | E1000_TCTL_CT_FD;
        self.io.write32(E1000_TCTL, tctl);

        // 接收控制: 启用 + 存储错误帧 + 单播混杂 + 多播混杂
        //           + 广播接受模式 + 剥离以太网 CRC + 缓冲区 2048 字节
        let rctl = E1000_RCTL_EN
            | E1000_RCTL_SBP
            | E1000_RCTL_UPE
            | E1000_RCTL_MPE
            | E1000_RCTL_BAM
            | E1000_RCTL_SECRC
            | E1000_RCTL_BSIZE_2048;
        self.io.write32(E1000_RCTL, rctl);

        // MAC 地址写入 RAL0/RAH0
        self.io.set_mac(&self.mac);
        slog_info!(
            Net,
            "e1000: MAC={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.mac[0], self.mac[1], self.mac[2],
            self.mac[3], self.mac[4], self.mac[5]
        );

        // RX tail: 通知硬件可接收范围
        self.io.set_rx_tail((E1000_RX_RING_SIZE - 1) as u32);

        // IPG (Inter-Packet Gap)
        self.io.write32(E1000_IPG, 0x0060200A);

        // 启用中断: RX Timer + RX Descriptor Minimum Threshold + Link Status Change
        self.io.irq_enable(E1000_ICR_RXT0 | E1000_ICR_RXDMT0 | E1000_ICR_LSC);

        slog_info!(
            Net,
            "e1000: initialized (CTRL=0x{:x} RDLEN=0x{:x})",
            self.io.ctrl(),
            self.io.read32(E1000_RDLEN)
        );

        self.initialized = true;
    }

    /// 驱动是否已初始化。
    pub fn is_ready(&self) -> bool {
        self.initialized
    }

    /// 标记驱动为已初始化 (kernel_test 模式使用)。
    pub fn mark_initialized(&mut self) {
        self.initialized = true;
    }

    /// 关闭设备: 禁用链路、清空收发控制寄存器。
    pub fn shutdown(&mut self) {
        let ctrl = self.io.read32(E1000_CTRL);
        self.io
            .write32(E1000_CTRL, ctrl & !(E1000_CTRL_SLU | E1000_CTRL_FD));
        self.io.write32(E1000_RCTL, 0);
        self.io.write32(E1000_TCTL, 0);
        self.initialized = false;
    }

    /// 读取并清除中断原因寄存器 (ICR)。
    pub fn ack_interrupt(&self) -> u32 {
        self.io.irq_ack()
    }

    /// 检查链路状态。
    pub fn link_is_up(&self) -> bool {
        self.io.link_is_up()
    }

    // ── DMA 环基地址配置 (委托给 E1000Io) ──

    /// 设置 TX 描述符环物理基地址
    pub fn set_tx_base(&self, phys: u64) {
        self.io.set_tx_base(phys);
    }

    /// 设置 TX 描述符环长度
    pub fn set_tx_len(&self, len: u32) {
        self.io.set_tx_len(len);
    }

    /// 设置 TX head 指针 (初始化用)
    pub fn set_tx_head(&self, val: u32) {
        self.io.set_tx_head_raw(val);
    }

    /// 设置 TX tail 指针
    pub fn set_tx_tail(&self, val: u32) {
        self.io.set_tx_tail(val);
    }

    /// 读取 TX head 指针 (硬件更新)
    pub fn tx_head(&self) -> u32 {
        self.io.tx_head()
    }

    /// 设置 RX 描述符环物理基地址
    pub fn set_rx_base(&self, phys: u64) {
        self.io.set_rx_base(phys);
    }

    /// 设置 RX 描述符环长度
    pub fn set_rx_len(&self, len: u32) {
        self.io.set_rx_len(len);
    }

    /// 设置 RX head 指针 (初始化用)
    pub fn set_rx_head(&self, val: u32) {
        self.io.set_rx_head_raw(val);
    }

    /// 设置 RX tail 指针
    pub fn set_rx_tail(&self, val: u32) {
        self.io.set_rx_tail(val);
    }

    /// 读取 RX head 指针 (硬件更新)
    pub fn rx_head(&self) -> u32 {
        self.io.rx_head()
    }

    // ── 直接寄存器访问 (供 dump_regs 等诊断用途) ──

    /// 读取控制寄存器
    pub fn ctrl(&self) -> u32 {
        self.io.ctrl()
    }

    /// 读取接收控制寄存器
    pub fn rx_ctl(&self) -> u32 {
        self.io.rx_ctl()
    }

    /// 读取发送控制寄存器
    pub fn tx_ctl(&self) -> u32 {
        self.io.tx_ctl()
    }

    /// 读取指定寄存器
    pub fn read_reg(&self, reg: u32) -> u32 {
        self.io.read32(reg)
    }

    /// 写入指定寄存器
    pub fn write_reg(&self, reg: u32, val: u32) {
        self.io.write32(reg, val);
    }

    // ── DMA 环安装 (由 framework 层分配后调用) ──

    /// 安装 TX/RX 描述符环 (framework 层分配后传入)
    pub fn install_rings(&mut self, tx_ring: TxRing, rx_ring: RxRing) {
        self.tx_ring = Some(tx_ring);
        self.rx_ring = Some(rx_ring);
    }

    // ── 数据路径 (services 层, 0 unsafe) ──

    /// 发送一个数据包
    ///
    /// 通过 TxRing 安全包装设置 DMA 描述符, 写入 TDT 寄存器通知硬件。
    pub fn send_packet(&mut self, data: &[u8]) -> Result<usize, KernelError> {
        let tx_ring = self.tx_ring.as_mut().ok_or(KernelError::NotInitialized)?;

        let tail = tx_ring.tail();
        let mut timeout: u32 = E1000_TIMEOUT;
        while !tx_ring.is_done(tail) && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
        if timeout == 0 {
            return Err(KernelError::Io);
        }

        let total_len = data.len().min(2048);
        tx_ring.prepare_from_virt(data.as_ptr() as u64, total_len as u16);

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        tx_ring.advance_tail();
        self.io.set_tx_tail(tx_ring.tail() as u32);

        Ok(total_len)
    }

    /// 接收一个数据包
    ///
    /// 通过 RxRing 安全包装检查描述符状态, 复制数据到调用方缓冲区。
    /// 跳过错误描述符, 继续查找有效数据包。
    pub fn receive_packet(&mut self, buf: &mut [u8]) -> Option<usize> {
        let rx_ring = self.rx_ring.as_mut()?;

        loop {
            let tail = rx_ring.tail();
            let rdh = self.io.rx_head() as usize;
            if tail == rdh {
                return None;
            }

            if !rx_ring.is_done(tail) {
                break;
            }

            if rx_ring.has_errors(tail) {
                slog_warn!(
                    Net,
                    "e1000: try_receive skip error desc[{}] errors=0x{:x}",
                    tail,
                    rx_ring.errors(tail)
                );
                rx_ring.clear_status(tail);
                let prev = tail;
                rx_ring.advance_tail();
                self.io.set_rx_tail(prev as u32);
                continue;
            }

            let copy_len = rx_ring.copy_packet(tail, buf);
            rx_ring.clear_status(tail);
            let prev = tail;
            rx_ring.advance_tail();
            self.io.set_rx_tail(prev as u32);

            return Some(copy_len);
        }

        None
    }

    /// 批量处理接收描述符 (中断路径调用)
    ///
    /// 遍历所有就绪的 RX 描述符, 清除状态并推进 tail。
    /// 返回本次处理的数据包数量 (不含错误包)。
    pub fn process_rx(&mut self) -> u32 {
        let rx_ring = match self.rx_ring.as_mut() {
            Some(r) => r,
            None => return 0,
        };

        let mut processed = 0u32;
        loop {
            let tail = rx_ring.tail();
            let rdh = self.io.rx_head() as usize;
            if tail == rdh {
                break;
            }

            if !rx_ring.is_done(tail) {
                slog_warn!(
                    Net,
                    "e1000: rx_tail={} != rdh={} but DD=0, errors=0x{:x}",
                    tail,
                    rdh,
                    rx_ring.errors(tail)
                );
                break;
            }

            let len = rx_ring.packet_length(tail);

            if !rx_ring.has_errors(tail) {
                processed += 1;
            } else {
                slog_warn!(
                    Net,
                    "e1000: RX desc[{}] errors=0x{:x} len={}",
                    tail,
                    rx_ring.errors(tail),
                    len
                );
            }

            rx_ring.clear_status(tail);
            let prev = tail;
            rx_ring.advance_tail();
            self.io.set_rx_tail(prev as u32);
        }

        processed
    }
}
