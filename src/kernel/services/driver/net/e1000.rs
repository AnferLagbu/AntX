//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 MMIO 操作通过 `framework::IoMem` 安全代理,
//! 替代原始 e1000 驱动中的裸 `mmio_read32`/`mmio_write32` (56 unsafe 行)。
//!
//! ## 迁移路径
//!
//! 原始驱动: `kernel::driver::net::e1000::E1000Device` — 56 unsafe 行
//! Services 适配: 通过 `IoMem` 封装 MMIO, 消除 unsafe。
//! 完整迁移 (Phase 2 后续) 将 DMA 描述符改为 `DmaStream`。

use crate::kernel::framework::iomem::IoMem;
use crate::kernel::framework::mm::PhysAddr;

// ── E1000 寄存器常量 (从原始驱动复制, 保持 ABI 兼容) ──

const E1000_CTRL: u32 = 0x0000;
const E1000_STATUS: u32 = 0x0008;
const E1000_EERD: u32 = 0x0014;
const E1000_EERD_START: u32 = 1 << 0;
const E1000_EERD_DONE: u32 = 1 << 4;
const E1000_IMS: u32 = 0x00D0;
const E1000_ICR: u32 = 0x00C0;
const E1000_RCTL: u32 = 0x0100;
const E1000_TCTL: u32 = 0x0400;
const E1000_RDBAL: u32 = 0x2800;
const E1000_RDBAH: u32 = 0x2804;
const E1000_RDLEN: u32 = 0x2808;
const E1000_RDH: u32 = 0x2810;
const E1000_RDT: u32 = 0x2818;
const E1000_TDBAL: u32 = 0x3800;
const E1000_TDBAH: u32 = 0x3804;
const E1000_TDLEN: u32 = 0x3808;
const E1000_TDH: u32 = 0x3810;
const E1000_TDT: u32 = 0x3818;
const E1000_RAL: u32 = 0x5400;
const E1000_RAH: u32 = 0x5404;
const E1000_TIMEOUT: u32 = 100000;

/// 安全的 E1000 MMIO 访问器。
///
/// 包装 `IoMem`，提供所有 E1000 寄存器的类型安全读写。
/// services 层通过此结构安全访问 E1000 网卡，替代裸指针 `mmio_base: *mut u8`。
pub struct E1000Io {
    mmio: IoMem,
}

impl E1000Io {
    /// 从物理地址创建 E1000 MMIO 访问器。
    ///
    /// # 参数
    /// - `phys`: E1000 BAR0 物理地址 (来自 PCI 枚举)
    /// - `len`: MMIO 区域大小 (通常 128KB)
    pub fn new(phys: PhysAddr, len: usize) -> Result<Self, &'static str> {
        // SAFETY: phys 来自 PCI BAR0 枚举, 保证是有效的 MMIO 区域。
        // IoMem::from_pci_bar 内部做零值检测和别名检测。
        let mmio = IoMem::from_pci_bar(phys, len, "e1000-bar0")?;
        Ok(Self { mmio })
    }

    // ── 寄存器读写 ──

    #[inline(always)]
    pub fn read32(&self, reg: u32) -> u32 {
        self.mmio.read_u32(reg as usize)
    }

    #[inline(always)]
    pub fn write32(&self, reg: u32, val: u32) {
        self.mmio.write_u32(reg as usize, val)
    }

    // ── EEPROM 读取 ──

    /// 通过 EERD 寄存器读 EEPROM 字。
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

    /// 读取中断原因并应答 (write-1-to-clear)。
    pub fn irq_ack(&self) -> u32 {
        let icr = self.read32(E1000_ICR);
        self.write32(E1000_ICR, icr);
        icr
    }

    /// 启用指定中断。
    pub fn irq_enable(&self, mask: u32) {
        self.write32(E1000_IMS, mask);
    }

    // ── 链路状态 ──

    /// 链路是否 UP。
    pub fn link_is_up(&self) -> bool {
        self.read32(E1000_STATUS) & 0x02 != 0
    }

    // ── 收发描述符基址 ──

    pub fn set_rx_base(&self, phys: u64) {
        self.write32(E1000_RDBAL, phys as u32);
        self.write32(E1000_RDBAH, (phys >> 32) as u32);
    }

    pub fn set_tx_base(&self, phys: u64) {
        self.write32(E1000_TDBAL, phys as u32);
        self.write32(E1000_TDBAH, (phys >> 32) as u32);
    }

    pub fn set_rx_len(&self, len: u32) { self.write32(E1000_RDLEN, len); }
    pub fn set_tx_len(&self, len: u32) { self.write32(E1000_TDLEN, len); }

    pub fn rx_head(&self) -> u32 { self.read32(E1000_RDH) }
    pub fn set_rx_tail(&self, val: u32) { self.write32(E1000_RDT, val); }
    pub fn tx_head(&self) -> u32 { self.read32(E1000_TDH) }
    pub fn set_tx_tail(&self, val: u32) { self.write32(E1000_TDT, val); }

    // ── 控制 ──

    pub fn ctrl(&self) -> u32 { self.read32(E1000_CTRL) }
    pub fn set_ctrl(&self, val: u32) { self.write32(E1000_CTRL, val); }
    pub fn rx_ctl(&self) -> u32 { self.read32(E1000_RCTL) }
    pub fn set_rx_ctl(&self, val: u32) { self.write32(E1000_RCTL, val); }
    pub fn tx_ctl(&self) -> u32 { self.read32(E1000_TCTL) }
    pub fn set_tx_ctl(&self, val: u32) { self.write32(E1000_TCTL, val); }
}
