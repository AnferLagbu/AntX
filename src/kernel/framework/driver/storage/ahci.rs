//! AHCI/SATA 驱动 (AHCI/SATA Driver)
//!
//! 提供AHCI (Advanced Host Controller Interface) SATA支持：
//! - **SATA接口**: 传统SATA SSD和HDD
//! - **DMA读写**: 通过PRDT进行DMA传输
//! - **多端口**: 支持多个SATA端口
//! - **LBA48**: 支持大容量磁盘
//!
//! ## 硬件规格
//!
//! ```text
//! AHCI Controller:
//! ├── HBA Memory (ABAR)
//! │   ├── Generic Host Control (GHC)
//! │   ├── Port Registers (0x100 + 0x80*n)
//! │   │   ├── PxCLB: 命令列表基地址
//! │   │   ├── PxFB: FIS基地址
//! │   │   ├── PxIS: 中断状态
//! │   │   ├── PxIE: 中断使能
//! │   │   ├── PxCMD: 命令和状态
//! │   │   ├── PxTFD: 任务文件数据
//! │   │   └── PxCI: 命令发布
//! │   └── ...
//! └── Command List & FIS Buffer (DMA)
//! ```
//!
//! # Safety
//! AHCI驱动涉及MMIO寄存器和DMA操作。

use super::framework::{DeviceInfo, DeviceType, Driver, DriverError, Result};
use crate::kernel::framework::dma::get_dma;
use crate::kernel::framework::iomem::IoMem;
use crate::kernel::framework::mm::{PhysAddr, VirtAddr, PAGE_SIZE};
use crate::klog_info;
use crate::klog_warn;
use alloc::vec::Vec;
use core::ptr;

// ============================================================================
// AHCI 常量定义
// ============================================================================

/// AHCI端口数量
const AHCI_MAX_PORTS: usize = 32;

/// 每个端口命令槽数量
const CMD_SLOTS: usize = 32;

/// 命令头大小 (字节)
const CMD_HEADER_SIZE: usize = 32;

/// 命令列表总大小
const CMD_LIST_SIZE: usize = CMD_SLOTS * CMD_HEADER_SIZE;

/// FIS接收缓冲区大小 (一页)
// 有意窄化: 用户内存代理, 指针/长度上下文保证
#[expect(clippy::cast_possible_truncation)]
const FIS_BUFFER_SIZE: usize = PAGE_SIZE as usize;

/// 命令表大小 (CFIS + ACMD + PRDT)
const CMD_TABLE_SIZE: usize = 256;

/// 单次传输最大扇区数 (1 PRDT, 128 扇区 = 64KB)
const MAX_SECTORS_PER_CMD: u16 = 128;

/// 扇区大小
const SECTOR_SIZE: usize = 512;

// AHCI GHC 寄存器布局 (AHCI Spec §3.1) — 真实使用的 2 个偏移:
//   GHC_GHC (Global Host Control) — 控制器 reset/IRQ/AE
//   GHC_PI  (Ports Implemented)   — 探测活动端口
// 其余 GHC_CAP/GHC_IS/GHC_VS 已删除, 通过 AhciHbaGhc repr(C) 字段直接访问.

// AHCI Port 寄存器布局 (AHCI Spec §3.3, port 区域 = 0x100 + n*0x80) —
// 通过 `AhciPortRegs` repr(C) 直接访问, 不再需要 PORT_CLB / PORT_IS 等 offset 常量.

const GHC_GHC: usize = 0x04;  // u32: Global Host Control
const GHC_PI: usize = 0x0C;   // u32: Ports Implemented

// Port 区域在 ABAR 内的基址
const PORT_REG_BASE: usize = 0x100;
const PORT_REG_STRIDE: usize = 0x80;

// ============================================================================
// AHCI 寄存器定义 (repr(C, packed) 与硬件匹配)
// ============================================================================

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct AhciHbaGhc {
    pub cap: u32,
    pub ghc: u32,
    pub is: u32,
    pub pi: u32,
    pub rsvd: [u32; 5],
    pub vs: u32,
    pub ccc_ctl: u32,
    pub ccc_pts: u32,
    pub em_loc: u32,
    pub em_ctl: u32,
    pub cap2: u32,
    pub bohc: u32,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct AhciPortRegs {
    pub clb: u32,
    pub clbu: u32,
    pub fb: u32,
    pub fbu: u32,
    pub is: u32,
    pub ie: u32,
    pub cmd: u32,
    pub rsvd1: u32,
    pub tfd: u32,
    pub sig: u32,
    pub ssts: u32,
    pub sctl: u32,
    pub serr: u32,
    pub sact: u32,
    pub ci: u32,
    pub sntf: u32,
    pub fbs: u32,
    pub rsvd2: [u32; 2],
    pub vs: [u32; 4],
}

/// AHCI 寄存器位域
///
/// AHCI 能力寄存器 (CAP) — AHCI 规范 §3.1.7
/// 当前未使用，直接通过 memread 访问。
/// 位定义供参考: S64A (bit 31: 64位寻址), SNCQ (bit 30: NCQ)
mod cap {}

mod ghc {
    pub const HR: u32 = 1 << 0;
    pub const IE: u32 = 1 << 1;
    pub const AE: u32 = 1 << 31;
}

/// AHCI 端口命令寄存器 (`PxCMD`) — AHCI 规范 §3.3.2
/// 未实现位: ICC [28:31] (接口通信控制)
mod pxcmd {
    pub const ST: u32 = 1 << 0;
    pub const FRE: u32 = 1 << 4;
    pub const FR: u32 = 1 << 14;
    pub const CR: u32 = 1 << 15;
}
mod pxssts {
    pub const DET: u32 = 0xF;
}
mod pxtfd {
    pub const ERR: u32 = 1 << 0;
    pub const DRQ: u32 = 1 << 3;
    pub const BSY: u32 = 1 << 7;
}
mod pxis {
    pub const DPS: u32 = 1 << 5;
    pub const PCS: u32 = 1 << 9;
    pub const DHRS: u32 = 1 << 0;
    pub const TFE: u32 = 1 << 30;
}

// ============================================================================
// SATA 命令定义
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AtaCommand {
    ReadDma = 0x25,  // READ DMA (LBA28) — also used as READ DMA EXT (LBA48)
    WriteDma = 0x35, // WRITE DMA (LBA28) — also used as WRITE DMA EXT (LBA48)
    Identify = 0xEC,
    ReadFpdmaQueued = 0x60,
    WriteFpdmaQueued = 0x61,
}

// ============================================================================
// AHCI 数据结构
// ============================================================================

/// AHCI 命令头
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct AhciCommandHeader {
    pub dw0: u32,   // CFL(5) | A(1) | W(1) | P(1) | R(1) | B(1) | C(1) | PMP(4) | PRDTL(16)
    pub prdtl: u32, // PRDT 字节计数 (高16位) | PRDT 长度 (低16位)
    pub prdbc: u32, // PRDT 已传输字节
    pub ctba: u32,  // 命令表基址低32位
    pub ctbau: u32, // 命令表基址高32位
    pub rsvd: [u32; 4],
}

impl AhciCommandHeader {
    pub fn new() -> Self {
        Self {
            dw0: 0,
            prdtl: 0,
            prdbc: 0,
            ctba: 0,
            ctbau: 0,
            rsvd: [0; 4],
        }
    }
}

/// 物理区域描述符 (PRDT entry)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct PhysicalRegionDescriptor {
    pub dba: u32,  // 数据基址低32位
    pub dbau: u32, // 数据基址高32位
    pub rsvd: u32,
    pub dbc: u32, // 字节计数 (高1位 = 中断完成标记)
}

/// AHCI 命令表
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct AhciCommandTable {
    pub cfis: [u8; 64], // 命令FIS
    pub acmd: [u8; 16], // ATAPI命令
    pub rsvd: [u8; 48],
    pub prdt: [PhysicalRegionDescriptor; 8],
}

// ============================================================================
// FIS 结构
// ============================================================================

/// 主机到设备FIS
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct H2dFis {
    pub fis_type: u8,
    pub flags: u8,
    pub command: u8,
    pub feature0: u8,
    pub lba0: u8,
    pub lba1: u8,
    pub lba2: u8,
    pub lba3: u8,
    pub device: u8,
    pub lba4: u8,
    pub lba5: u8,
    pub feature1: u8,
    pub count0: u8,
    pub count1: u8,
    pub icc: u8,
    pub control: u8,
    pub rsvd: [u32; 4],
}

impl H2dFis {
    pub fn new() -> Self {
        Self {
            fis_type: 0,
            flags: 0,
            command: 0,
            feature0: 0,
            feature1: 0,
            lba0: 0,
            lba1: 0,
            lba2: 0,
            lba3: 0,
            lba4: 0,
            lba5: 0,
            device: 0,
            count0: 0,
            count1: 0,
            icc: 0,
            control: 0,
            rsvd: [0; 4],
        }
    }

    /// 创建读DMA FIS (LBA48)
    pub fn read_dma(lba: u64, count: u16) -> Self {
        let mut fis = Self::new();
        fis.fis_type = 0x27; // H2D
        fis.flags = 0x80; // 写命令
        fis.command = 0x25; // READ DMA EXT
        fis.device = 0x40; // LBA 模式
        fis.lba0 = (lba & 0xFF) as u8;
        fis.lba1 = ((lba >> 8) & 0xFF) as u8;
        fis.lba2 = ((lba >> 16) & 0xFF) as u8;
        fis.lba3 = ((lba >> 24) & 0xFF) as u8;
        fis.lba4 = ((lba >> 32) & 0xFF) as u8;
        fis.lba5 = ((lba >> 40) & 0xFF) as u8;
        fis.count0 = (count & 0xFF) as u8;
        fis.count1 = ((count >> 8) & 0xFF) as u8;
        fis
    }

    /// 创建写DMA FIS (LBA48)
    pub fn write_dma(lba: u64, count: u16) -> Self {
        let mut fis = Self::new();
        fis.fis_type = 0x27;
        fis.flags = 0x80;
        fis.command = 0x35; // WRITE DMA EXT
        fis.device = 0x40;
        fis.lba0 = (lba & 0xFF) as u8;
        fis.lba1 = ((lba >> 8) & 0xFF) as u8;
        fis.lba2 = ((lba >> 16) & 0xFF) as u8;
        fis.lba3 = ((lba >> 24) & 0xFF) as u8;
        fis.lba4 = ((lba >> 32) & 0xFF) as u8;
        fis.lba5 = ((lba >> 40) & 0xFF) as u8;
        fis.count0 = (count & 0xFF) as u8;
        fis.count1 = ((count >> 8) & 0xFF) as u8;
        fis
    }

    /// 创建Identify FIS
    pub fn identify() -> Self {
        let mut fis = Self::new();
        fis.fis_type = 0x27;
        fis.flags = 0x80;
        fis.command = 0xEC;
        fis.device = 0xA0;
        fis.count0 = 1;
        fis
    }
}

// ============================================================================
// AHCI 端口 (DMA-backed)
// ============================================================================

/// AHCI 端口 DMA 资源
struct AhciPortDma {
    cmd_list_virt: VirtAddr,
    cmd_list_phys: PhysAddr,
    fis_virt: VirtAddr,
    fis_phys: PhysAddr,
    cmd_table_virt: VirtAddr,
    cmd_table_phys: PhysAddr,
}

impl AhciPortDma {
    fn new() -> Self {
        Self {
            cmd_list_virt: VirtAddr(0),
            cmd_list_phys: PhysAddr(0),
            fis_virt: VirtAddr(0),
            fis_phys: PhysAddr(0),
            cmd_table_virt: VirtAddr(0),
            cmd_table_phys: PhysAddr(0),
        }
    }
}

/// AHCI端口
pub struct AhciPort {
    pub port_num: u8,
    regs: *mut AhciPortRegs,
    pub device_present: bool,
    pub signature: u32,
    dma: AhciPortDma,
    port_initialized: bool,
}

impl AhciPort {
    pub fn new(port_num: u8, regs: *mut AhciPortRegs) -> Self {
        Self {
            port_num,
            regs,
            device_present: false,
            signature: 0,
            dma: AhciPortDma::new(),
            port_initialized: false,
        }
    }

    /// 分配 DMA 内存并设置寄存器
    /// # Errors
    /// DMA 引擎未初始化或 DMA 内存分配失败时返回 Err。
    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
    pub fn setup_dma(&mut self) -> Result<()> {
        let dma_engine = get_dma();
        if !dma_engine.is_initialized() {
            return Err(DriverError::NotInitialized);
        }

        // 分配命令列表 (1KB, 对齐 1KB)
        if let Some((v, p)) = dma_engine.alloc_coherent(CMD_LIST_SIZE) {
            self.dma.cmd_list_virt = v;
            self.dma.cmd_list_phys = p;
        } else {
            return Err(DriverError::HardwareError);
        }

        // 分配 FIS 接收缓冲区 (4KB, 对齐 4KB)
        if let Some((v, p)) = dma_engine.alloc_coherent(FIS_BUFFER_SIZE) {
            self.dma.fis_virt = v;
            self.dma.fis_phys = p;
        } else {
            dma_engine.free_coherent(self.dma.cmd_list_virt, CMD_LIST_SIZE);
            return Err(DriverError::HardwareError);
        }

        // 分配命令表 (256B)
        if let Some((v, p)) = dma_engine.alloc_coherent(CMD_TABLE_SIZE) {
            self.dma.cmd_table_virt = v;
            self.dma.cmd_table_phys = p;
        } else {
            dma_engine.free_coherent(self.dma.cmd_list_virt, CMD_LIST_SIZE);
            dma_engine.free_coherent(self.dma.fis_virt, FIS_BUFFER_SIZE);
            return Err(DriverError::HardwareError);
        }

        // 写入寄存器
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let regs = &mut *self.regs;
            regs.clb = self.dma.cmd_list_phys.0 as u32;
            regs.clbu = (self.dma.cmd_list_phys.0 >> 32) as u32;
            regs.fb = self.dma.fis_phys.0 as u32;
            regs.fbu = (self.dma.fis_phys.0 >> 32) as u32;
        }

        Ok(())
    }

    /// 释放 DMA 资源
    fn free_dma(&mut self) {
        let dma_engine = get_dma();
        if self.dma.cmd_list_virt.0 != 0 {
            dma_engine.free_coherent(self.dma.cmd_list_virt, CMD_LIST_SIZE);
            self.dma.cmd_list_virt = VirtAddr(0);
        }
        if self.dma.fis_virt.0 != 0 {
            dma_engine.free_coherent(self.dma.fis_virt, FIS_BUFFER_SIZE);
            self.dma.fis_virt = VirtAddr(0);
        }
        if self.dma.cmd_table_virt.0 != 0 {
            dma_engine.free_coherent(self.dma.cmd_table_virt, CMD_TABLE_SIZE);
            self.dma.cmd_table_virt = VirtAddr(0);
        }
    }

    /// 检测设备
    pub fn detect_device(&mut self) -> bool {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let regs = &*self.regs;
            let det = regs.ssts & pxssts::DET;
            if det == 0x03 {
                self.device_present = true;
                self.signature = regs.sig;
                matches!(self.signature, 0x00000101 | 0xEB140101)
            } else {
                self.device_present = false;
                false
            }
        }
    }

    /// 启动端口 (启用 FIS 接收 + 命令处理)
    /// # Errors
    /// DMA 设置失败或端口寄存器等待确认超时时返回 Err。
    pub fn enable(&mut self) -> Result<()> {
        // 先分配 DMA
        self.setup_dma()?;

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let regs = &mut *self.regs;

            // 清零中断状态
            regs.is = 0xFFFFFFFF;

            // 启用 FIS 接收
            regs.cmd |= pxcmd::FRE;

            // 等待 FRE 确认
            let mut timeout = 1_000_000u64;
            while regs.cmd & pxcmd::FR == 0 && timeout > 0 {
                timeout -= 1;
                core::hint::spin_loop();
            }
            if timeout == 0 {
                return Err(DriverError::Timeout);
            }

            // 启动命令处理
            regs.cmd |= pxcmd::ST;

            // 等待 CR 确认
            timeout = 1_000_000;
            while regs.cmd & pxcmd::CR == 0 && timeout > 0 {
                timeout -= 1;
                core::hint::spin_loop();
            }
            if timeout == 0 {
                return Err(DriverError::Timeout);
            }
        }

        self.port_initialized = true;
        Ok(())
    }

    /// 停止端口
    /// # Errors
    /// 端口停止操作失败时返回 Err。
    pub fn disable(&mut self) -> Result<()> {
        if !self.port_initialized {
            return Ok(());
        }

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let regs = &mut *self.regs;

            // 停止命令处理
            regs.cmd &= !pxcmd::ST;
            let mut timeout = 1_000_000u64;
            while regs.cmd & pxcmd::CR != 0 && timeout > 0 {
                timeout -= 1;
                core::hint::spin_loop();
            }

            // 停止 FIS 接收
            regs.cmd &= !pxcmd::FRE;
            timeout = 1_000_000;
            while regs.cmd & pxcmd::FR != 0 && timeout > 0 {
                timeout -= 1;
                core::hint::spin_loop();
            }
        }

        self.free_dma();
        self.port_initialized = false;
        Ok(())
    }

    /// 提交 DMA 命令并等待完成
    ///
    /// # Safety
    /// `buffer` 必须是有效的 DMA-coherent 内存指针
    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
    unsafe fn submit_dma_command(
        &mut self,
        fis: &H2dFis,
        buffer_phys: PhysAddr,
        byte_count: u32,
        is_write: bool,
    ) -> Result<()> {
        // SAFETY: 调用方 (read_dma/write_dma) 保证 self.regs 指向有效 MMIO,
        // buffer_phys 是 dma_engine 分配的 DMA-coherent 物理地址, byte_count 不超过缓冲区大小
        unsafe {
        let regs = &mut *self.regs;
        let slot = 0u32; // 使用 slot 0

        // ── 设置命令表 ──
        let cmd_table = self.dma.cmd_table_virt.0 as *mut AhciCommandTable;
        let fis_bytes = core::slice::from_raw_parts(
            fis as *const _ as *const u8,
            core::mem::size_of::<H2dFis>(),
        );

        // 复制 FIS 到命令表 (CFIS 位于偏移 0)
        ptr::copy_nonoverlapping(fis_bytes.as_ptr(), cmd_table as *mut u8, fis_bytes.len());

        // 设置 PRDT entry 0
        (*cmd_table).prdt[0] = PhysicalRegionDescriptor {
            dba: buffer_phys.0 as u32,
            dbau: (buffer_phys.0 >> 32) as u32,
            rsvd: 0,
            dbc: (byte_count - 1) | (1 << 31), // IOC (中断完成)
        };
        // 清零其余 PRDT
        for i in 1..8 {
            (*cmd_table).prdt[i] = PhysicalRegionDescriptor {
                dba: 0,
                dbau: 0,
                rsvd: 0,
                dbc: 0,
            };
        }

        // ── 设置命令头 ──
        let cmd_hdr = self.dma.cmd_list_virt.0 as *mut AhciCommandHeader;
        let flags: u32 = 5u32 // command FIS length (5 DWORDs = 20 bytes)
            | (if is_write { 1 << 6 } else { 0 }); // W bit
        (*cmd_hdr.add(slot as usize)).dw0 = flags | 1; // PRDTL = 1
        (*cmd_hdr.add(slot as usize)).prdtl = 0u32;
        (*cmd_hdr.add(slot as usize)).prdbc = 0;
        (*cmd_hdr.add(slot as usize)).ctba = self.dma.cmd_table_phys.0 as u32;
        (*cmd_hdr.add(slot as usize)).ctbau = (self.dma.cmd_table_phys.0 >> 32) as u32;

        // ── 等待端口空闲 ──
        let mut timeout = 1_000_000u64;
        while regs.tfd & (pxtfd::BSY | pxtfd::DRQ) != 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
        if timeout == 0 {
            return Err(DriverError::Timeout);
        }

        // 清零中断状态
        regs.is = 0xFFFFFFFF;

        // ── 发布命令 ──
        // sfence: 确保内存写入对设备可见
        crate::arch!(fence_w());
        regs.ci = 1 << slot;

        // ── 等待完成 ──
        timeout = 5_000_000; // 5M iterations (~1s at 5GHz)
        while timeout > 0 {
            // 检查 D2H 寄存器 FIS 中断
            if regs.is & pxis::DHRS != 0 {
                break;
            }
            // 检查位 FIS 中断 (由 IOC 标志触发)
            if regs.is & (pxis::DPS | pxis::PCS) != 0 {
                break;
            }
            timeout -= 1;
            core::hint::spin_loop();
        }

        if timeout == 0 {
            return Err(DriverError::Timeout);
        }

        // ── 检查错误 ──
        if regs.is & pxis::TFE != 0 {
            return Err(DriverError::HardwareError);
        }
        if regs.tfd & pxtfd::ERR != 0 {
            return Err(DriverError::HardwareError);
        }

        Ok(())
    }}

    /// 读取扇区 (DMA)
    /// # Errors
    /// 端口未初始化、设备不存在、参数非法、DMA 缓冲区分配失败或硬件错误时返回 Err。
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    pub fn read(&mut self, lba: u64, count: u16, buffer: *mut u8) -> Result<()> {
        if !self.port_initialized || !self.device_present {
            return Err(DriverError::NotInitialized);
        }
        if count == 0 || count > MAX_SECTORS_PER_CMD {
            return Err(DriverError::InvalidParameter);
        }

        let byte_count = u32::from(count) * SECTOR_SIZE as u32;

        // 分配 DMA buffer
        let dma_engine = get_dma();
        let (buf_virt, buf_phys) = dma_engine
            .alloc_coherent(byte_count as usize)
            .ok_or(DriverError::Busy)?;

        let fis = H2dFis::read_dma(lba, count);

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let result = unsafe { self.submit_dma_command(&fis, buf_phys, byte_count, false) };

        // 复制数据到用户 buffer
        if result.is_ok() {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                ptr::copy_nonoverlapping(buf_virt.0 as *const u8, buffer, byte_count as usize);
            }
        }

        dma_engine.free_coherent(buf_virt, byte_count as usize);
        result
    }

    /// 写入扇区 (DMA)
    /// # Errors
    /// 端口未初始化、设备不存在、参数非法、DMA 缓冲区分配失败或硬件错误时返回 Err。
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    pub fn write(&mut self, lba: u64, count: u16, buffer: *const u8) -> Result<()> {
        if !self.port_initialized || !self.device_present {
            return Err(DriverError::NotInitialized);
        }
        if count == 0 || count > MAX_SECTORS_PER_CMD {
            return Err(DriverError::InvalidParameter);
        }

        let byte_count = u32::from(count) * SECTOR_SIZE as u32;

        // 分配 DMA buffer
        let dma_engine = get_dma();
        let (buf_virt, buf_phys) = dma_engine
            .alloc_coherent(byte_count as usize)
            .ok_or(DriverError::Busy)?;

        // 复制数据到 DMA buffer
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            ptr::copy_nonoverlapping(buffer, buf_virt.0 as *mut u8, byte_count as usize);
        }

        let fis = H2dFis::write_dma(lba, count);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let result = unsafe { self.submit_dma_command(&fis, buf_phys, byte_count, true) };

        dma_engine.free_coherent(buf_virt, byte_count as usize);
        result
    }
}

impl Drop for AhciPort {
    fn drop(&mut self) {
        let _ = self.disable();
    }
}

// SAFETY: AhciController 通过 volatile 访问 MMIO 寄存器.
// SAFETY: AhciController 含 MMIO 裸指针, 全局 AHCI_CONTROLLERS Mutex 防止并发跨 CPU 变更.
unsafe impl Send for AhciController {}
// SAFETY: 同上, Mutex 保证并发安全.
unsafe impl Sync for AhciController {}

// ============================================================================
// AHCI 控制器
// ============================================================================

pub struct AhciController {
    mmio_phys: u64,            // PCI BAR physical address (for external use)
    iomem: Option<IoMem>,      // MMIO region handle (safe access proxy)
    ports: Vec<AhciPort>,
    port_bitmap: u32,
    // I-49: 设备元数据 (驱动名/类型), 供 hotplug/procfs 导出.
    info: DeviceInfo,
    initialized: bool,
}

impl AhciController {
    pub fn new(mmio_phys: usize) -> Self {
        Self {
            mmio_phys: mmio_phys as u64,
            iomem: None,
            ports: Vec::new(),
            port_bitmap: 0,
            info: DeviceInfo::new("ahci", DeviceType::Block),
            initialized: false,
        }
    }

    /// 获取设备信息
    pub fn get_info(&self) -> &DeviceInfo {
        &self.info
    }

    /// 初始化控制器
    /// # Errors
    /// 获取 MMIO 失败、HBA 复位超时或端口初始化失败时返回 Err。
    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
    pub fn init_controller(&mut self) -> Result<()> {
        // 初始化 IoMem
        let iomem = IoMem::from_pci_bar(
            PhysAddr(self.mmio_phys),
            8192, // ABAR is typically 8KB
            "ahci-abar",
        ).map_err(|_| DriverError::HardwareError)?;

        // 确保 AHCI 模式已启用
        let mut ghc_val = iomem.read_u32(GHC_GHC);
        if ghc_val & ghc::AE == 0 {
            ghc_val |= ghc::AE;
            iomem.write_u32(GHC_GHC, ghc_val);
        }

        // HBA 复位
        iomem.write_u32(GHC_GHC, ghc_val | ghc::HR);
        let mut timeout = 1_000_000u64;
        while iomem.read_u32(GHC_GHC) & ghc::HR != 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
        if timeout == 0 {
            return Err(DriverError::Timeout);
        }

        // 启中断
        iomem.write_u32(GHC_GHC, iomem.read_u32(GHC_GHC) | ghc::IE);

        // 获取已实现的端口
        self.port_bitmap = iomem.read_u32(GHC_PI);

        // 初始化每个端口
        for i in 0..AHCI_MAX_PORTS {
            if self.port_bitmap & (1u32 << i) == 0 {
                continue;
            }

            // SAFETY: IoMem 确保 MMIO 区域已正确映射.
            // 端口寄存器偏移在 BAR 范围内.
            let port_regs = unsafe {
                let base = iomem.virt_ptr();
                base.add(PORT_REG_BASE + i * PORT_REG_STRIDE) as *mut AhciPortRegs
            };
            let mut port = AhciPort::new(i as u8, port_regs);

            if port.detect_device() {
                match port.enable() {
                    Ok(()) => {
                        klog_info!(
                            Driver,
                            "AHCI: port {} enabled (sig={:08X})",
                            i,
                            port.signature
                        );
                        self.ports.push(port);
                    }
                    Err(e) => {
                        klog_warn!(Driver, "AHCI: port {} enable failed: {:?}", i, e);
                    }
                }
            }
        }

        self.iomem = Some(iomem);
        Ok(())
    }

    pub fn port_count(&self) -> usize {
        self.ports.len()
    }

    /// 获取端口 (用于读写)
    pub fn get_port(&mut self, index: usize) -> Option<&mut AhciPort> {
        self.ports.get_mut(index)
    }
}

// ============================================================================
// Driver Trait 实现
// ============================================================================

impl Driver for AhciController {
    fn name(&self) -> &'static str {
        "AHCI Controller"
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn init(&mut self) -> Result<()> {
        self.init_controller()?;
        self.initialized = true;
        klog_info!(
            Driver,
            "AHCI: controller initialized, {} port(s) active",
            self.ports.len()
        );
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        for port in &mut self.ports {
            let _ = port.disable();
        }
        if let Some(iomem) = self.iomem.as_ref() {
            let ghc = iomem.read_u32(GHC_GHC);
            iomem.write_u32(GHC_GHC, ghc & !ghc::AE);
        }
        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }
    fn status(&self) -> &'static str {
        if self.initialized {
            "AHCI ready"
        } else {
            "AHCI not initialized"
        }
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_h2d_fis_read() {
        let fis = H2dFis::read_dma(0x1000, 8);
        assert_eq!(fis.fis_type, 0x27);
        assert_eq!(fis.command, 0x25);
        assert_eq!(fis.count0, 8);
    }

    #[test]
    fn test_h2d_fis_write() {
        let fis = H2dFis::write_dma(0x2000, 16);
        assert_eq!(fis.fis_type, 0x27);
        assert_eq!(fis.command, 0x35);
        assert_eq!(fis.count0, 16);
    }

    #[test]
    fn test_cmd_header_structure() {
        assert_eq!(core::mem::size_of::<AhciCommandHeader>(), 32);
        assert_eq!(core::mem::size_of::<PhysicalRegionDescriptor>(), 16);
        assert_eq!(
            core::mem::size_of::<AhciCommandTable>(),
            64 + 16 + 48 + 8 * 16
        );
    }

    #[test]
    fn test_prdt_structure() {
        assert_eq!(core::mem::size_of::<PhysicalRegionDescriptor>(), 16);
    }

    #[test]
    fn test_ahci_controller_creation() {
        let ctrl = AhciController::new(0xFE000000);
        assert_eq!(ctrl.name(), "AHCI Controller");
        assert_eq!(ctrl.device_type(), DeviceType::Block);
        assert!(!ctrl.is_ready());
    }
}
