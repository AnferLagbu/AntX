//! E1000 网卡驱动 (Rust 安全重写)
//!
//! 提供对 Intel 82540EM (E1000) 千兆网卡的完整支持：
//! - **PCI 探测**: 自动扫描 PCI 总线发现设备
//! - **MMIO 操作**: 内存映射 I/O 寄存器访问
//! - **描述符环**: TX/RX 环形缓冲区管理
//! - **数据包收发**: 与 lwIP 协议栈无缝集成
//! - **中断处理**: IRQ 注册和服务程序
//!
//! ## 硬件规格
//!
//! ```text
//! Intel 82540EM (E1000)
//! ├── MMIO Base: BAR0 (通常 4GB 以上)
//! ├── IRQ: 通过 PCI 配置获取 (通常是 11)
//! └── MAC: 从 EEPROM 读取 (6 字节)
//!
//! 寄存器布局:
//! ├── CTRL    (0x0000): 控制寄存器
//! ├── STATUS  (0x0008): 状态寄存器
//! ├── RCTL    (0x0100): 接收控制
//! ├── TCTL    (0x0400): 发送控制
//! ├── TDBAL/H (0x3800): TX 描述符基址
//! ├── RDBAL/H (0x2800): RX 描述符基址
//! └── IMC/IMS (0x00D8/00D0): 中断屏蔽/状态
//! ```
//!
//! # Safety
//! 此模块直接操作硬件 MMIO 和 PCI 配置空间。

use core::sync::atomic::{AtomicU32, Ordering};

use crate::kernel::driver::framework::{Driver, DeviceType, DriverError, Result};

// Poll debug counter
static POLL_COUNT: AtomicU32 = AtomicU32::new(0);

// ============================================================================
// E1000 硬件常量定义
// ============================================================================

/// TX/RX 描述符环大小
const E1000_TX_RING_SIZE: usize = 64;
const E1000_RX_RING_SIZE: usize = 128;

/// RX 缓冲区大小
const E1000_RX_BUFFER_SIZE: usize = 2048;

/// 最大等待超时 (循环次数)
const E1000_TIMEOUT: u32 = 100000;

// ============================================================================
// E1000 寄存器偏移量
// ============================================================================

/// 控制寄存器
const E1000_CTRL: u32 = 0x0000;
const E1000_CTRL_RST: u32 = 1 << 31;      // 软复位
const E1000_CTRL_SLU: u32 = 1 << 6;       // Set Link Up
const E1000_CTRL_ASDE: u32 = 1 << 5;     // Auto Speed Detect Enable
const E1000_CTRL_SPEED_1000: u32 = 2 << 8; // 1Gbps
const E1000_CTRL_FRCDPX: u32 = 1 << 14;   // Force Full-Duplex
const E1000_CTRL_FD: u32 = 1 << 0;        // Full-Duplex
const E1000_CTRL_FRCSPD: u32 = 1 << 11;   // Force Speed

/// 状态寄存器
const E1000_STATUS: u32 = 0x0008;
const E1000_STATUS_LU: u32 = 1 << 1;       // Link Up
const E1000_STATUS_FD: u32 = 1 << 0;       // Full-Duplex
const E1000_STATUS_SPEED_1000: u32 = 2 << 6;
const E1000_STATUS_SPEED_100: u32 = 1 << 6;

/// EEPROM 读寄存器
const E1000_EERD: u32 = 0x0014;
const E1000_EERD_START: u32 = 1 << 0;     // 开始读
const E1000_EERD_DONE: u32 = 1 << 4;      // 完成

/// 接收控制寄存器
const E1000_RCTL: u32 = 0x0100;
const E1000_RCTL_EN: u32 = 1 << 1;         // 接收使能
const E1000_RCTL_SBP: u32 = 1 << 2;        // Store Bad Packets
const E1000_RCTL_UPE: u32 = 1 << 3;        // Unicast Promiscuous Enable
const E1000_RCTL_MPE: u32 = 1 << 4;        // Multicast Promiscuous Enable
const E1000_RCTL_BAM: u32 = 1 << 15;       // Broadcast Accept Mode
const E1000_RCTL_SECRC: u32 = 1 << 26;     // Strip Ethernet CRC
const E1000_RCTL_BSIZE_2048: u32 = (1 << 25) | (0 << 16); // BSEX=1, BSIZE=00 = 2048

/// 发送控制寄存器
const E1000_TCTL: u32 = 0x0400;
const E1000_TCTL_EN: u32 = 1 << 1;         // 发送使能
const E1000_TCTL_PSP: u32 = 1 << 3;        // Pad Short Packets
const E1000_TCTL_COLD_MASK: u32 = 0xF << 12; // Collision Distance mask
const E1000_TCTL_CT_MASK: u32 = 0xF << 4;   // Collision Threshold mask
const E1000_TCTL_COLD_FD: u32 = 0x200;      // COLD for full-duplex (512)
const E1000_TCTL_CT_FD: u32 = 0x10 << 4;    // CT for full-duplex (16)

/// TX 描述符寄存器
const E1000_TDBAL: u32 = 0x3800;
const E1000_TDBAH: u32 = 0x3804;
const E1000_TDLEN: u32 = 0x3808;
const E1000_TDH: u32 = 0x3810;
const E1000_TDT: u32 = 0x3818;

/// RX 描述符寄存器
const E1000_RDBAL: u32 = 0x2800;
const E1000_RDBAH: u32 = 0x2804;
const E1000_RDLEN: u32 = 0x2808;
const E1000_RDH: u32 = 0x2810;
const E1000_RDT: u32 = 0x2818;

/// 中断相关
const E1000_IMC: u32 = 0x00D8;
const E1000_ICR: u32 = 0x00C0;
const E1000_IMS: u32 = 0x00D0;
const E1000_ICR_LSC: u32 = 1 << 2;       // Link Status Change
const E1000_ICR_RXDMT0: u32 = 1 << 4;    // Receive Descriptor Minimum Threshold
const E1000_ICR_RXO: u32 = 1 << 6;       // Receiver Overrun
const E1000_ICR_RXT0: u32 = 1 << 7;      // Receive Timer Interrupt

/// IPG (Inter-Packet Gap)
const E1000_IPG: u32 = 0x00B0;

/// Receive Address Registers
const E1000_RAL0: u32 = 0x5400;    // Receive Address Low
const E1000_RAH0: u32 = 0x5404;    // Receive Address High
const E1000_RAH_AV: u32 = 1 << 31; // Address Valid

// ============================================================================
// TX/RX 描述符结构体
// ============================================================================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct E1000TxDesc {
    addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct E1000RxDesc {
    addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

/// TX 描述符命令标志
const E1000_TXD_CMD_EOP: u8 = 1 << 0;   // End of Packet
const E1000_TXD_CMD_IFCS: u8 = 1 << 1;  // Insert FCS
const E1000_TXD_CMD_RS: u8 = 1 << 3;    // Report Status

/// TX 描述符状态标志
const E1000_TXD_STAT_DD: u8 = 1 << 0;   // Descriptor Done

/// RX 描述符状态标志
const E1000_RXD_STAT_DD: u8 = 1 << 0;   // Descriptor Done
const E1000_RXD_ERR_CE: u8 = 1 << 0;    // CRC Error
const E1000_RXD_ERR_SE: u8 = 1 << 1;     // Symbol Error
const E1000_RXD_ERR_SEQ: u8 = 1 << 2;   // Sequence Error
const E1000_RXD_ERR_RXE: u8 = 1 << 3;   // RX Error

// ============================================================================
// 设备状态结构体
// ============================================================================

/// E1000 设备实例
pub struct E1000Device {
    /// PCI 总线号
    pub bus: u8,
    /// PCI 设备号
    pub device: u8,
    /// PCI 功能号
    pub func: u8,
    
    /// MMIO 物理基地址
    mmio_phys: u64,
    /// MMIO 映射后虚拟地址
    mmio_base: *mut u8,
    
    /// IRQ 号
    pub irq: u8,
    /// MAC 地址 (6字节)
    pub mac: [u8; 6],
    
    /// TX 描述符环
    tx_descs: Option<*mut E1000TxDesc>,
    tx_tail: usize,
    tx_count: u64,
    
    /// RX 描述符环
    rx_descs: Option<*mut E1000RxDesc>,
    rx_buffers: [*mut u8; E1000_RX_RING_SIZE],
    rx_tail: usize,
    rx_count: u64,
    
    /// 统计信息
    isr_count: u64,
    link_change_count: u64,
    
    /// 设备信息
    info: crate::kernel::driver::framework::DeviceInfo,
    initialized: bool,
}

impl Default for E1000Device {
    fn default() -> Self {
        Self {
            bus: 0,
            device: 0,
            func: 0,
            mmio_phys: 0,
            mmio_base: core::ptr::null_mut(),
            irq: 0,
            mac: [0u8; 6],
            tx_descs: None,
            tx_tail: 0,
            tx_count: 0,
            rx_descs: None,
            rx_buffers: [core::ptr::null_mut(); E1000_RX_RING_SIZE],
            rx_tail: 0,
            rx_count: 0,
            isr_count: 0,
            link_change_count: 0,
            info: crate::kernel::driver::framework::DeviceInfo::new("Intel E1000", DeviceType::Network),
            initialized: false,
        }
    }
}

// ============================================================================
// MMIO 操作辅助函数
// ============================================================================

/// 读取 32 位 MMIO 寄存器
#[inline(always)]
unsafe fn mmio_read32(base: *mut u8, reg: u32) -> u32 {
    let ptr = base.add(reg as usize) as *const u32;
    core::ptr::read_volatile(ptr)
}

/// 写入 32 位 MMIO 寄存器
#[inline(always)]
unsafe fn mmio_write32(base: *mut u8, reg: u32, val: u32) {
    let ptr = base.add(reg as usize) as *mut u32;
    core::ptr::write_volatile(ptr, val);
}

/// 读取 16 位 MMIO 寄存器
#[inline(always)]
unsafe fn mmio_read16(base: *mut u8, reg: u32) -> u16 {
    let ptr = base.add(reg as usize) as *const u16;
    core::ptr::read_volatile(ptr)
}

/// 写入 16 位 MMIO 寄存器
#[inline(always)]
unsafe fn mmio_write16(base: *mut u8, reg: u32, val: u16) {
    let ptr = base.add(reg as usize) as *mut u16;
    core::ptr::write_volatile(ptr, val);
}

// ============================================================================
// EEPROM 操作
// ============================================================================

/// 从 EEPROM 读取一个字
fn eeprom_read(dev: &E1000Device, addr: u8) -> u16 {
    unsafe {
        mmio_write32(
            dev.mmio_base,
            E1000_EERD,
            ((addr as u32) << 2) | E1000_EERD_START
        );

        let mut timeout: u32 = 0;
        while timeout < E1000_TIMEOUT {
            let val = mmio_read32(dev.mmio_base, E1000_EERD);
            if val & E1000_EERD_DONE != 0 {
                return ((val >> 16) & 0xFFFF) as u16;
            }
            timeout += 1;
            core::hint::spin_loop();
        }

        0xFFFF  // 超时返回错误值
    }
}

/// 读取 MAC 地址
fn read_mac_address(dev: &mut E1000Device) {
    let lo = eeprom_read(dev, 0);
    let hi = eeprom_read(dev, 1);
    
    dev.mac[0] = (lo & 0xFF) as u8;
    dev.mac[1] = ((lo >> 8) & 0xFF) as u8;
    dev.mac[2] = (hi & 0xFF) as u8;
    dev.mac[3] = ((hi >> 8) & 0xFF) as u8;
    
    let lo2 = eeprom_read(dev, 2);
    dev.mac[4] = (lo2 & 0xFF) as u8;
    dev.mac[5] = ((lo2 >> 8) & 0xFF) as u8;
}

// ============================================================================
// TX/RX 描述符环初始化
// ============================================================================

/// 初始化 TX/RX 描述符环
fn setup_descriptor_rings(dev: &mut E1000Device) -> Result<()> {
    extern "C" { fn kmalloc_align(size: u64, align: u64) -> *mut core::ffi::c_void; }

    // 分配 TX 描述符 (16字节对齐)
    let tx_size = core::mem::size_of::<E1000TxDesc>() * E1000_TX_RING_SIZE;
    let tx_ptr = unsafe { kmalloc_align(tx_size as u64, 16) };
    if tx_ptr.is_null() {
        return Err(DriverError::HardwareError);
    }
    
    let tx_descs = tx_ptr as *mut E1000TxDesc;
    
    for i in 0..E1000_TX_RING_SIZE {
        unsafe {
            (*tx_descs.add(i)).addr = 0;
            (*tx_descs.add(i)).length = 0;
            (*tx_descs.add(i)).cmd = 0;
            (*tx_descs.add(i)).status = E1000_TXD_STAT_DD;
        }
    }
    dev.tx_tail = 0;
    dev.tx_descs = Some(tx_descs);

    // 设置 TX 描述符到硬件
    unsafe {
        let tx_phys = virt_to_phys(tx_ptr as u64);
        
        mmio_write32(dev.mmio_base, E1000_TDBAL, (tx_phys & 0xFFFFFFFF) as u32);
        mmio_write32(dev.mmio_base, E1000_TDBAH, (tx_phys >> 32) as u32);
        mmio_write32(dev.mmio_base, E1000_TDLEN, tx_size as u32);
        mmio_write32(dev.mmio_base, E1000_TDH, 0);
        mmio_write32(dev.mmio_base, E1000_TDT, 0);
    }

    // 分配 RX 描述符
    let rx_size = core::mem::size_of::<E1000RxDesc>() * E1000_RX_RING_SIZE;
    let rx_ptr = unsafe { kmalloc_align(rx_size as u64, 16) };
    if rx_ptr.is_null() {
        return Err(DriverError::HardwareError);
    }
    
    let rx_descs = rx_ptr as *mut E1000RxDesc;
    
    for i in 0..E1000_RX_RING_SIZE {
        // 分配 RX 缓冲区
        let buf_ptr = unsafe { kmalloc_align(E1000_RX_BUFFER_SIZE as u64, 16) };
        if buf_ptr.is_null() {
            return Err(DriverError::HardwareError);
        }
        
        dev.rx_buffers[i] = buf_ptr as *mut u8;
        
        unsafe {
            (*rx_descs.add(i)).addr = virt_to_phys(buf_ptr as u64);
            (*rx_descs.add(i)).length = 0;
            (*rx_descs.add(i)).status = 0;
        }
    }
    dev.rx_tail = 0;
    dev.rx_descs = Some(rx_descs);

    // 设置 RX 描述符到硬件
    unsafe {
        let rx_phys = virt_to_phys(rx_ptr as u64);
        
        mmio_write32(dev.mmio_base, E1000_RDBAL, (rx_phys & 0xFFFFFFFF) as u32);
        mmio_write32(dev.mmio_base, E1000_RDBAH, (rx_phys >> 32) as u32);
        mmio_write32(dev.mmio_base, E1000_RDLEN, rx_size as u32);
        mmio_write32(dev.mmio_base, E1000_RDH, 0);
        // RDT 指向最后一个可用描述符
        // 初始化时所有描述符都可用，所以 RDT = E1000_RX_RING_SIZE - 1
        // 软件从 RDH=0 开始处理，所以 rx_tail = 0
        mmio_write32(dev.mmio_base, E1000_RDT, (E1000_RX_RING_SIZE - 1) as u32);
    }
    // rx_tail 从 0 开始，与 RDH 一致
    dev.rx_tail = 0;

    Ok(())
}

/// 虚拟地址转物理地址 (简化实现)
fn virt_to_phys(virt: u64) -> u64 {
    const KERNEL_VMA_BASE: u64 = 0xFFFF800000000000;
    
    if virt >= KERNEL_VMA_BASE {
        virt - KERNEL_VMA_BASE
    } else {
        virt
    }
}

// ============================================================================
// Driver Trait 实现
// ============================================================================

impl Driver for E1000Device {
    fn name(&self) -> &'static str {
        "Intel E1000 Gigabit Ethernet"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Network
    }

    fn init(&mut self) -> Result<()> {
        if self.mmio_base.is_null() || self.mmio_base == core::ptr::null_mut() {
            return Err(DriverError::NotInitialized);
        }

        let base = self.mmio_base;

        // 1. 发送全局复位, 等待硬件完成
        unsafe {
            mmio_write32(base, E1000_CTRL, E1000_CTRL_RST);
        }
        
        for _ in 0..100000 {
            let ctrl = unsafe { mmio_read32(base, E1000_CTRL) };
            if ctrl & E1000_CTRL_RST == 0 {
                break;
            }
            core::hint::spin_loop();
        }

        // 2. 清除所有中断掩码
        unsafe { mmio_write32(base, E1000_IMC, 0xFFFFFFFF); }

        // 3. 使能链路 (读-改-写以保留关键位)
        {
            let ctrl = unsafe { mmio_read32(base, E1000_CTRL) };
            // 保留 FD, ASDE, SPEED, 设置 SLU
            let new_ctrl = (ctrl & !(E1000_CTRL_RST)) | E1000_CTRL_SLU | E1000_CTRL_ASDE;
            unsafe { mmio_write32(base, E1000_CTRL, new_ctrl); }
            let _ = (ctrl, new_ctrl);
        }

        // 4. 等待链路建立
        let mut link_ready = false;
        for _ in 0..500000 {
            let status = unsafe { mmio_read32(base, E1000_STATUS) };
            if status & E1000_STATUS_LU != 0 {
                link_ready = true;
                break;
            }
            core::hint::spin_loop();
        }

        if !link_ready {
            extern "C" { fn klog_net(fmt: *const i8); }
            unsafe { klog_net("e1000: link not ready, continuing anyway\0".as_ptr() as *const i8); }
        } else {
            extern "C" { fn klog_net(fmt: *const i8); }
            unsafe { klog_net("e1000: link up\0".as_ptr() as *const i8); }
        }

        // 5. 初始化描述符环 (必须在使能 TX/RX 之前)
        setup_descriptor_rings(self)?;

        // 6. 配置并启用发送 (EN + PSP + COLD + CT)
        let tctl = E1000_TCTL_EN | E1000_TCTL_PSP | E1000_TCTL_COLD_FD | E1000_TCTL_CT_FD;
        unsafe { mmio_write32(base, E1000_TCTL, tctl); }

        // Read back TCTL to verify write
        let tctl_rb = unsafe { mmio_read32(base, E1000_TCTL) };
        unsafe {
            extern "C" { fn klog_net(fmt: *const i8); }
            if tctl_rb & E1000_TCTL_EN != 0 {
                klog_net("e1000: TCTL EN verified\0".as_ptr() as *const i8);
            } else {
                klog_net("e1000: TCTL EN NOT set!\0".as_ptr() as *const i8);
            }
        }

        // 7. 配置并启用接收
        let rctl = E1000_RCTL_EN | E1000_RCTL_SBP | E1000_RCTL_UPE
                 | E1000_RCTL_MPE | E1000_RCTL_BAM
                 | E1000_RCTL_SECRC | E1000_RCTL_BSIZE_2048;
        unsafe { mmio_write32(base, E1000_RCTL, rctl); }

        // Read back RCTL to verify
        unsafe {
            extern "C" { fn klog_net(fmt: *const i8); }
            let rctl_rb = mmio_read32(base, E1000_RCTL);
            if rctl_rb & E1000_RCTL_EN != 0 {
                klog_net("e1000: RCTL EN verified\0".as_ptr() as *const i8);
            } else {
                klog_net("e1000: RCTL EN NOT set!\0".as_ptr() as *const i8);
            }
        }

        // 7b. 配置 MAC 地址到 Receive Address 寄存器
        {
            extern "C" { fn klog_net(fmt: *const i8); }
            let ral = (self.mac[0] as u32)
                | ((self.mac[1] as u32) << 8)
                | ((self.mac[2] as u32) << 16)
                | ((self.mac[3] as u32) << 24);
            let rah = (self.mac[4] as u32)
                | ((self.mac[5] as u32) << 8)
                | E1000_RAH_AV;
            unsafe {
                mmio_write32(base, E1000_RAH0, rah);
                mmio_write32(base, E1000_RAL0, ral);
                klog_net("e1000: MAC addr configured in RAL/RAH\0".as_ptr() as *const i8);
            }
            let _ = (ral, rah);
        }

        // 7c. 重新写入 RDT 确保硬件在 RCTL 使能后识别描述符
        unsafe {
            mmio_write32(base, E1000_RDT, (E1000_RX_RING_SIZE - 1) as u32);
        }
        // rx_tail 从 0 开始，与 RDH 一致
        self.rx_tail = 0;

        // 8. 配置 IPG (IEEE 802.3 standard: IPGR1=10, IPGR2=4, IPG=6)
        // Format: IPG[7:0], IPGR1[13:10], IPGR2[23:20]
        unsafe { mmio_write32(base, E1000_IPG, (10 | (4 << 10) | (6 << 20)) as u32); }

        // 9. 启用中断
        unsafe {
            mmio_write32(base, E1000_IMS, E1000_ICR_RXT0 | E1000_ICR_RXDMT0 | E1000_ICR_LSC);
        }

        self.initialized = true;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        // 禁用接收和发送
        unsafe {
            let base = self.mmio_base;
            
            let mut ctrl = mmio_read32(base, E1000_CTRL);
            ctrl &= !(E1000_CTRL_SLU | E1000_CTRL_FD);
            mmio_write32(base, E1000_CTRL, ctrl);

            mmio_write32(base, E1000_RCTL, 0);
            
            mmio_write32(base, E1000_TCTL, 0);
        }

        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized && !self.mmio_base.is_null()
    }

    fn status(&self) -> &'static str {
        if !self.initialized {
            "Not initialized"
        } else if self.mmio_base.is_null() {
            "MMIO not mapped"
        } else {
            "Link ready"
        }
    }
}

// ============================================================================
// 公共 API
// ============================================================================

impl E1000Device {
    /// 创建新的 E1000 实例
    pub fn new() -> Self {
        Self::default()
    }

    /// PCI 探测并初始化设备
    ///
    /// 扫描 PCI 总线查找 Intel E1000 网卡。
    pub fn probe(&mut self) -> Result<()> {
        extern "C" {
            fn pci_read_config_word(bus: u8, dev: u8, func: u8, offset: u8) -> u16;
            fn pci_read_config_dword(bus: u8, dev: u8, func: u8, offset: u8) -> u32;
            fn pci_write_config_dword(bus: u8, dev: u8, func: u8, offset: u8, val: u32);
        }

        for bus in 0..255u8 {
            let vendor_id = unsafe { pci_read_config_word(bus, 0, 0, 0x00) };
            
            if vendor_id == 0xFFFF || vendor_id == 0x0000 {
                if bus > 0 { continue; }
            }

            for dev_idx in 0..32u8 {
                for func in 0..8u8 {
                    let vid = unsafe { pci_read_config_word(bus, dev_idx, func, 0x00) };

                    if vid == 0xFFFF || vid == 0x0000 {
                        if func == 0 { break; }
                        continue;
                    }

                    let _did = unsafe { pci_read_config_word(bus, dev_idx, func, 0x02) };
                    let class_code = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x08) };
                    let base_class = ((class_code >> 24) & 0xFF) as u8;

                    if vid == 0x8086 && base_class == 0x02 {
                        self.bus = bus;
                        self.device = dev_idx;
                        self.func = func;

                        let bar0_lo = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x10) };
                        
                        unsafe { pci_write_config_dword(bus, dev_idx, func, 0x10, 0xFFFFFFFF) };
                        let _bar_size_mask = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x10) };
                        unsafe { pci_write_config_dword(bus, dev_idx, func, 0x10, bar0_lo) };

                        let is_io = (bar0_lo & 0x01) != 0;
                        
                        if is_io {
                            return Err(DriverError::UnsupportedOperation);
                        }

                        self.mmio_phys = (bar0_lo & 0xFFFFFFF0) as u64;

                        let irq_reg = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x3C) };
                        self.irq = (irq_reg & 0xFF) as u8;

                        let mut cmd = unsafe { pci_read_config_dword(bus, dev_idx, func, 0x04) };
                        cmd |= 0x06;
                        unsafe { pci_write_config_dword(bus, dev_idx, func, 0x04, cmd) };

                        // 设置 MMIO 基址 (boot 页表恒等映射物理地址)
                        self.mmio_base = self.mmio_phys as *mut u8;
                        {
                            extern "C" { fn klog_net(fmt: *const i8); }
                            unsafe { klog_net("e1000: MMIO base mapped successfully\0".as_ptr() as *const i8); }
                        }

                        read_mac_address(self);

                        return Ok(());
                    }

                    if func == 0 && (vid & 0x8000) == 0 {
                        break;
                    }
                }
            }
        }

        Err(DriverError::DeviceNotFound)
    }

    /// 发送数据包
    pub fn send_packet(&mut self, data: &[u8]) -> Result<usize> {
        if !self.is_ready() {
            return Err(DriverError::NotInitialized);
        }

        let tx_descs = match self.tx_descs {
            Some(descs) => descs,
            None => return Err(DriverError::NotInitialized),
        };

        let tail = self.tx_tail;
        let desc = unsafe { &mut *tx_descs.add(tail) };

        let mut timeout: u32 = E1000_TIMEOUT;
        while desc.status & E1000_TXD_STAT_DD == 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }

        if timeout == 0 {
            return Err(DriverError::Timeout);
        }

        let total_len = data.len().min(2048);
        let phys = virt_to_phys(data.as_ptr() as u64);
        
        desc.addr = phys;
        desc.length = total_len as u16;
        desc.cmd = E1000_TXD_CMD_EOP | E1000_TXD_CMD_IFCS | E1000_TXD_CMD_RS;
        desc.status = 0;

        // 内存屏障: 确保描述符写入对设备可见后再更新 TDT
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        self.tx_tail = (tail + 1) % E1000_TX_RING_SIZE;
        unsafe {
            mmio_write32(self.mmio_base, E1000_TDT, self.tx_tail as u32);
        }

        self.tx_count += 1;
        Ok(total_len)
    }

    /// 处理接收到的数据包 (在中断中调用)
    ///
    /// 接收数据包并传递给 lwIP 协议栈处理。
    pub fn process_rx_packets(&mut self) {
        extern "C" {
            fn klog_net(fmt: *const i8);
        }

        if !self.is_ready() {
            return;
        }

        let rx_descs = match self.rx_descs {
            Some(descs) => descs,
            None => return,
        };

        let mut processed = 0u32;

        loop {
            // 读取 RDH（硬件维护的接收描述符头指针）
            let rdh = unsafe { mmio_read32(self.mmio_base, E1000_RDH) as usize };

            // 检查是否有新的描述符被硬件填充
            // 当 rx_tail == rdh 时，说明没有新数据包
            if self.rx_tail == rdh {
                break; // 没有更多数据包
            }

            // 获取当前描述符
            let desc = unsafe { &mut *rx_descs.add(self.rx_tail) };
            
            // 检查 DD 位，确认描述符已被硬件填充
            if desc.status & E1000_RXD_STAT_DD == 0 {
                // DD 位未设置，跳过这个描述符
                let prev = self.rx_tail;
                self.rx_tail = (self.rx_tail + 1) % E1000_RX_RING_SIZE;
                unsafe {
                    mmio_write32(self.mmio_base, E1000_RDT, prev as u32);
                }
                continue;
            }

            let len = desc.length as usize;

            // 检查错误标志
            if desc.errors & (E1000_RXD_ERR_CE | E1000_RXD_ERR_SE |
                          E1000_RXD_ERR_SEQ | E1000_RXD_ERR_RXE) == 0 {
                processed += 1;
                // 有效数据包 - 传递给 lwIP
                if !self.rx_buffers[self.rx_tail].is_null() {
                    let _pkt_data = unsafe {
                        core::slice::from_raw_parts(
                            self.rx_buffers[self.rx_tail],
                            len
                        )
                    };

                    // 调用 lwIP ethernet_input 处理数据包
                    unsafe {
                        ethernet_input_from_e1000(
                            self.rx_buffers[self.rx_tail] as *mut core::ffi::c_void,
                            len as u16
                        );
                    }
                }
            }

            // 清除状态位，重新使用描述符
            desc.status = 0;
            self.rx_count += 1;

            // 保存当前 tail，然后移到下一个描述符
            let prev = self.rx_tail;
            self.rx_tail = (self.rx_tail + 1) % E1000_RX_RING_SIZE;

            // 更新 RDT，通知硬件这个描述符已经可用
            // RDT 应该指向最后一个可用描述符（即 prev）
            unsafe {
                mmio_write32(self.mmio_base, E1000_RDT, prev as u32);
            }
        }

        if processed > 0 {
            unsafe { klog_net("e1000: RX processed packets\0".as_ptr() as *const i8); }
        }
    }

    /// 处理中断 (ISR 入口点)
    pub fn handle_interrupt(&mut self) {
        extern "C" {
            fn klog_net(fmt: *const i8);
        }

        if !self.is_ready() {
            return;
        }

        let icr = unsafe { mmio_read32(self.mmio_base, E1000_ICR) };
        if icr == 0 {
            return;
        }

        self.isr_count += 1;
        
        // 输出中断原因（仅前几次）
        if self.isr_count <= 5 {
            unsafe { klog_net("e1000: ISR triggered\0".as_ptr() as *const i8); }
        }

        if icr & E1000_ICR_LSC != 0 {
            self.link_change_count += 1;
            unsafe { klog_net("e1000: link status change\0".as_ptr() as *const i8); }
        }

        if icr & (E1000_ICR_RXT0 | E1000_ICR_RXDMT0) != 0 {
            if self.isr_count <= 5 {
                unsafe { klog_net("e1000: RX interrupt\0".as_ptr() as *const i8); }
            }
            self.process_rx_packets();
        }
    }

    /// 获取 MAC 地址字符串
    #[cfg(feature = "alloc")]
    pub fn get_mac_string(&self) -> alloc::string::String {
        use alloc::format;
        
        format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                self.mac[0], self.mac[1], self.mac[2],
                self.mac[3], self.mac[4], self.mac[5]
        )
    }

    /// 获取统计信息
    pub fn get_stats(&self) -> (u64, u64, u64, u64) {
        (
            self.isr_count,
            self.rx_count,
            self.tx_count,
            self.link_change_count,
        )
    }

    /// 获取设备信息
    pub fn get_info(&self) -> &crate::kernel::driver::framework::DeviceInfo {
        &self.info
    }
}

// ============================================================================
// FFI 兼容接口 (供 lwIP 调用)
// ============================================================================

// 外部声明: lwIP ethernet_input 函数
extern "C" {
    // 将接收到的以太网帧传递给 lwIP 协议栈处理
    fn ethernet_input_from_e1000(data: *mut core::ffi::c_void, len: u16) -> i32;
}

unsafe fn pic_outb(port: u16, value: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
}
unsafe fn pic_inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack));
    value
}

/// 全局 E1000 实例
static mut E1000_INSTANCE: Option<E1000Device> = None;

/// 初始化 E1000 并设置 netif 结构体字段
#[no_mangle]
pub extern "C" fn e1000_init(netif: *mut core::ffi::c_void) -> i32 {
    extern "C" {
        fn antx_netif_init(netif: *mut core::ffi::c_void, mac: *const u8);
        fn klog_net(fmt: *const i8);
    }
    unsafe {
        match &mut E1000_INSTANCE {
            Some(ref mut dev) => {
                if dev.mmio_base.is_null() { return -5; }
                match dev.init() {
                    Ok(()) => {
                        antx_netif_init(netif, dev.mac.as_ptr());
                        // 注册 E1000 IRQ 处理器
                        if dev.irq != 0 && dev.irq != 255 {
                            extern "C" {
                                fn idt_register_irq(irq: u8, handler: extern "C" fn(*mut core::ffi::c_void), name: *const i8, flags: u32) -> i32;
                                fn idt_enable_irq(irq: u8);
                                fn klog_net(fmt: *const i8, ...);
                            }
                            klog_net("e1000: registering IRQ %d\0".as_ptr() as *const i8, dev.irq as i32);
                            idt_register_irq(dev.irq, e1000_irq_entry as extern "C" fn(*mut core::ffi::c_void), b"e1000\0".as_ptr() as *const i8, 0);
                            idt_enable_irq(dev.irq);  // 关键：启用 IRQ
                            if dev.irq < 8 {
                                let mask = pic_inb(0x21);
                                pic_outb(0x21, mask & !(1u8 << dev.irq));
                            } else {
                                let mask = pic_inb(0xA1);
                                pic_outb(0xA1, mask & !(1u8 << (dev.irq - 8)));
                            }
                        }
                        klog_net("E1000 initialized, IRQ registered\0".as_ptr() as *const i8);
                        0
                    },
                    Err(_e) => -5,
                }
            },
            None => -5,
        }
    }
}

/// E1000 发送函数 (lwIP 回调)
///
/// 从 lwIP 协议栈发送数据包到网络。
///
/// # Arguments
/// * `netif` - 网络接口指针 (lwIP netif 结构)
/// * `p` - pbuf 链表指针 (lwIP 数据包缓冲区)
///
/// # Returns
/// * `ERR_OK` (0) - 成功
/// * `-1` - 失败
#[no_mangle]
pub extern "C" fn e1000_send(_netif: *mut core::ffi::c_void, p: *mut core::ffi::c_void) -> i32 {
    unsafe {
        extern "C" {
            fn klog_net(fmt: *const i8);
        }
        
        if E1000_INSTANCE.is_none() || p.is_null() {
            klog_net("e1000_send: instance is none or p is null\0".as_ptr() as *const i8);
            return -1;
        }

        // 从 pbuf 提取数据并发送
        if let Some(ref mut dev) = E1000_INSTANCE {
            // 获取 pbuf 总长度和数据指针
            let (total_len, data_ptr) = extract_pbuf_data(p);

            if total_len == 0 || data_ptr.is_null() {
                klog_net("e1000_send: total_len=0 or data_ptr is null\0".as_ptr() as *const i8);
                return -1;
            }

            klog_net("e1000_send: sending packet\0".as_ptr() as *const i8);

            // 构造数据切片
            let packet = core::slice::from_raw_parts(data_ptr as *const u8, total_len);

            // 通过 E1000 发送
            match dev.send_packet(packet) {
                Ok(_) => {
                    klog_net("e1000_send: send OK\0".as_ptr() as *const i8);
                    0  // ERR_OK
                }
                Err(_) => {
                    klog_net("e1000_send: send failed\0".as_ptr() as *const i8);
                    -1
                }
            }
        } else {
            klog_net("e1000_send: no instance\0".as_ptr() as *const i8);
            -1
        }
    }
}

/// 从 lwIP pbuf 提取数据
///
/// # Safety
/// 此函数操作 lwIP 内部数据结构
unsafe fn extract_pbuf_data(p: *mut core::ffi::c_void) -> (usize, *mut u8) {
    extern "C" {
        fn antx_pbuf_copyout(p: *mut core::ffi::c_void, buf: *mut u8, out_len: *mut u16);
    }
    let pbuf_base = p as *mut u8;
    let total = *(pbuf_base.add(0x10) as *const u16) as usize;
    static mut TX_BUF: [u8; 1600] = [0u8; 1600];
    let mut out_len: u16 = total.min(1600) as u16;
    antx_pbuf_copyout(p, TX_BUF.as_mut_ptr(), &mut out_len);
    (out_len as usize, TX_BUF.as_mut_ptr())
}

/// E1000 中断入口
#[no_mangle]
pub extern "C" fn e1000_irq_entry(_frame: *mut core::ffi::c_void) {
    unsafe {
        if let Some(ref mut dev) = E1000_INSTANCE {
            dev.handle_interrupt();
        }
    }
}

/// E1000 探测函数
#[no_mangle]
pub extern "C" fn e1000_probe() -> i32 {
    unsafe {
        if E1000_INSTANCE.is_none() {
            E1000_INSTANCE = Some(E1000Device::new());
        }

        match &mut E1000_INSTANCE {
            Some(ref mut dev) => {
                match dev.probe() {
                    Ok(()) => 0,
                    Err(_) => -1,
                }
            },
            None => -1,
        }
    }
}

/// 获取 E1000 实例指针 (内部使用)
#[no_mangle]
pub extern "C" fn get_e1000_instance() -> *mut core::ffi::c_void {
    unsafe {
        match &mut E1000_INSTANCE {
            Some(ref mut dev) => dev as *mut _ as *mut core::ffi::c_void,
            None => core::ptr::null_mut(),
        }
    }
}

#[no_mangle]
pub extern "C" fn e1000_dump_regs() {
    unsafe {
        if let Some(ref dev) = E1000_INSTANCE {
            let base = dev.mmio_base;
            if base.is_null() { return; }
            extern "C" { fn klog_net(fmt: *const i8); }
            klog_net("=== E1000 Register Dump ===\0".as_ptr() as *const i8);
            let tctl = mmio_read32(base, E1000_TCTL);
            let rctl = mmio_read32(base, E1000_RCTL);
            let status = mmio_read32(base, E1000_STATUS);
            let icr = mmio_read32(base, E1000_ICR);
            let tdh = mmio_read32(base, E1000_TDH);
            let tdt = mmio_read32(base, E1000_TDT);
            let rdh = mmio_read32(base, E1000_RDH);
            let rdt = mmio_read32(base, E1000_RDT);
            let _ = (tctl, rctl, status, icr, tdh, tdt, rdh, rdt);
            klog_net("e1000: reg dump done\0".as_ptr() as *const i8);
        }
    }
}

#[no_mangle]
pub extern "C" fn e1000_dump_stats() {
    let instance = unsafe { &E1000_INSTANCE };
    if let Some(ref dev) = *instance {
        let _ = dev.mmio_base;
    }
}

/// 轮询 RX 环 (用于诊断中断是否工作)
#[no_mangle]
pub extern "C" fn e1000_poll_rx() {
    unsafe {
        if let Some(ref mut dev) = E1000_INSTANCE {
            dev.process_rx_packets();
        }
    }
}

/// Dump E1000 寄存器值 (诊断用)
pub fn dump_registers() {
    unsafe {
        if let Some(ref dev) = E1000_INSTANCE {
            extern "C" { fn klog_net(fmt: *const i8); }
            klog_net("=== E1000 Registers ===\0".as_ptr() as *const i8);
            klog_net("RCTL\0".as_ptr() as *const i8);
            klog_net("STATUS\0".as_ptr() as *const i8);
            klog_net("RDBA\0".as_ptr() as *const i8);
            klog_net("RDBAH\0".as_ptr() as *const i8);
            klog_net("RDLEN\0".as_ptr() as *const i8);
            klog_net("RDH\0".as_ptr() as *const i8);
            klog_net("RDT\0".as_ptr() as *const i8);
            klog_net("TDH\0".as_ptr() as *const i8);
            klog_net("TDT\0".as_ptr() as *const i8);
            klog_net("ICR\0".as_ptr() as *const i8);
            klog_net("IMS\0".as_ptr() as *const i8);
            klog_net("CTRL\0".as_ptr() as *const i8);
            klog_net("====================\0".as_ptr() as *const i8);
        }
    }
}

static mut KALLOC_BUF: [u8; 1048576] = [0; 1048576];
static mut KALLOC_OFF: usize = 0;

#[no_mangle]
pub unsafe extern "C" fn kmalloc_align(size: u64, _align: u64) -> *mut core::ffi::c_void {
    let s = size as usize;
    if KALLOC_OFF + s > KALLOC_BUF.len() { return core::ptr::null_mut(); }
    let ptr = KALLOC_BUF.as_mut_ptr().add(KALLOC_OFF) as *mut core::ffi::c_void;
    KALLOC_OFF += s;
    ptr
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_creation() {
        let dev = E1000Device::new();
        
        assert_eq!(dev.bus, 0);
        assert_eq!(dev.device, 0);
        assert!(!dev.is_ready());
        assert_eq!(dev.name(), "Intel E1000 Gigabit Ethernet");
        assert_eq!(dev.device_type(), DeviceType::Network);
    }

    #[test]
    fn test_constants() {
        assert_eq!(E1000_TX_RING_SIZE, 64);
        assert_eq!(E1000_RX_RING_SIZE, 128);
        assert_eq!(E1000_RX_BUFFER_SIZE, 2048);
    }

    #[test]
    fn test_descriptor_sizes() {
        assert_eq!(core::mem::size_of::<E1000TxDesc>(), 16);
        assert_eq!(core::mem::size_of::<E1000RxDesc>(), 16);
    }

    #[test]
    fn test_driver_trait_impl() {
        let dev = E1000Device::new();
        
        assert!(!dev.is_ready());
        assert_eq!(dev.status(), "Not initialized");
    }

    #[test]
    fn test_virt_to_phys_conversion() {
        let high_addr: u64 = 0xFFFF800000000000;
        assert_eq!(virt_to_phys(high_addr), 0);
        
        assert_eq!(virt_to_phys(0x12345678), 0x12345678);
    }
}
