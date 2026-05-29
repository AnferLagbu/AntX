//! NVMe 驱动 (NVMe Driver)
//!
//! 提供NVMe (Non-Volatile Memory Express) SSD支持：
//! - **PCIe接口**: 高速PCIe总线连接
//! - **DMA读写**: Admin队列 + I/O队列提交
//! - **PRP寻址**: 物理区域页寻址
//! - **命名空间**: 多命名空间支持
//!
//! ## 硬件规格
//!
//! ```text
//! NVMe Controller:
//! ├── PCIe Configuration Space
//! ├── Controller Registers (BAR0)
//! │   ├── CAP, VS, INTMS, INTMC
//! │   ├── CC, CSTS, NSSR
//! │   ├── AQA, ASQ, ACQ
//! │   └── Doorbell Registers
//! │       ├── SQ0TDBL (Admin SQ Tail)
//! │       └── CQ0HDBL (Admin CQ Head)
//! └── Queue Pairs (DMA allocated)
//!     ├── Admin SQ / CQ
//!     └── I/O SQ / CQ
//! ```
//!
//! # Safety
//! NVMe驱动涉及PCIe配置、MMIO寄存器和DMA操作。

use super::framework::{Driver, DeviceType, DriverError, Result, DeviceInfo};
use crate::kernel::mm::{PhysAddr, VirtAddr};
use crate::kernel::dma::engine::get_dma;
use core::ptr;
use crate::klog_info;

// ============================================================================
// NVMe 常量定义
// ============================================================================

const ADMIN_QUEUE_ID: u16 = 0;
const IO_QUEUE_ID: u16 = 1;

const QUEUE_DEPTH: usize = 64;      // Admin + I/O 队列深度
const SQ_ENTRY_SIZE: usize = 64;    // 提交队列条目大小
const CQ_ENTRY_SIZE: usize = 16;    // 完成队列条目大小
const SQ_SIZE: usize = QUEUE_DEPTH * SQ_ENTRY_SIZE;
const CQ_SIZE: usize = QUEUE_DEPTH * CQ_ENTRY_SIZE;

const PAGE_SIZE: u64 = 4096;
const SECTOR_SIZE: usize = 512;

/// 最大扇区数 (128 sectors = 64KB, 单次命令)
const MAX_SECTORS_PER_CMD: u16 = 128;

// ============================================================================
// NVMe 寄存器定义
// ============================================================================

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeControllerRegisters {
    pub cap: u64,       // 控制器能力
    pub vs: u32,        // 版本
    pub intms: u32,     // 中断掩码设置
    pub intmc: u32,     // 中断掩码清除
    pub cc: u32,        // 控制器配置
    pub rsvd1: u32,
    pub csts: u32,      // 控制器状态
    pub nssr: u32,      // NVM 子系统复位
    pub aqa: u32,       // Admin 队列属性
    pub asq: u64,       // Admin 提交队列基地址
    pub acq: u64,       // Admin 完成队列基地址
    pub cmbloc: u32,    // 控制器内存缓冲区位置
    pub cmbsz: u32,     // 控制器内存缓冲区大小
    pub rsvd2: [u32; 8],
    pub bpinfo: u32,    // 启动分区信息
    pub bprsel: u32,    // 启动分区读选择
    pub bpmbl: u64,     // 启动分区内存缓冲位置
    pub rsvd3: [u64; 38],
    // 门铃寄存器紧随其后
}

// 寄存器位域
mod cap {
    pub const MQES: u64 = 0xFFFF;
    pub const CQR: u64 = 1 << 16;
    pub const AMS: u64 = 0x3 << 17;
    pub const TO: u64 = 0xFF << 24;
    pub const DSTRD: u64 = 0xF << 32;
    pub const CSS: u64 = 0xFF << 37;
    pub const MPSMIN: u64 = 0xF << 48;
    pub const MPSMAX: u64 = 0xF << 52;
}

mod cc {
    pub const EN: u32 = 1 << 0;
    pub const CSS_NVM: u32 = 0 << 4;
    pub const MPS_SHIFT: u32 = 7;
    pub const AMS_RR: u32 = 0 << 11;
    pub const SHN: u32 = 0x3 << 14;
    pub const IOCQES_SHIFT: u32 = 20;
    pub const IOSQES_SHIFT: u32 = 24;
}

mod csts {
    pub const RDY: u32 = 1 << 0;
    pub const CSTS_NSSRO: u32 = 1 << 4;
}

// ============================================================================
// NVMe 命令定义
// ============================================================================

/// Admin 命令操作码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NvmeAdminOpcode {
    DeleteSq = 0x00,
    CreateSq = 0x01,
    GetLogPage = 0x02,
    DeleteCq = 0x04,
    CreateCq = 0x05,
    Identify = 0x06,
    Abort = 0x08,
    SetFeatures = 0x09,
    GetFeatures = 0x0A,
}

/// NVM I/O 命令操作码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NvmeNvmOpcode {
    Flush = 0x00,
    Write = 0x01,
    Read = 0x02,
}

/// NVMe 命令 (64字节)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeCommand {
    pub opcode: u8,
    pub flags: u8,
    pub cid: u16,
    pub nsid: u32,
    pub cdw2: u32,
    pub cdw3: u32,
    pub mptr: u64,        // PRP1 / SGL entry 1
    pub prp2: u64,        // PRP2 / SGL entry 2
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

impl NvmeCommand {
    pub fn new() -> Self {
        Self {
            opcode: 0, flags: 0, cid: 0, nsid: 0,
            cdw2: 0, cdw3: 0, mptr: 0, prp2: 0,
            cdw10: 0, cdw11: 0, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0,
        }
    }

    /// 创建读命令
    pub fn read(nsid: u32, slba: u64, nlb: u16, prp1: u64) -> Self {
        Self {
            opcode: NvmeNvmOpcode::Read as u8,
            flags: 0,
            cid: 0,
            nsid,
            cdw2: 0, cdw3: 0,
            mptr: prp1,
            prp2: 0,
            cdw10: (slba & 0xFFFFFFFF) as u32,
            cdw11: ((slba >> 32) & 0xFFFFFFFF) as u32,
            cdw12: (nlb as u32 - 1) & 0xFFFF,    // NLB = #blocks - 1
            cdw13: 0, cdw14: 0, cdw15: 0,
        }
    }

    /// 创建写命令
    pub fn write(nsid: u32, slba: u64, nlb: u16, prp1: u64) -> Self {
        Self {
            opcode: NvmeNvmOpcode::Write as u8,
            flags: 0,
            cid: 0,
            nsid,
            cdw2: 0, cdw3: 0,
            mptr: prp1,
            prp2: 0,
            cdw10: (slba & 0xFFFFFFFF) as u32,
            cdw11: ((slba >> 32) & 0xFFFFFFFF) as u32,
            cdw12: (nlb as u32 - 1) & 0xFFFF,
            cdw13: 0, cdw14: 0, cdw15: 0,
        }
    }

    /// 创建 Identify 命令
    pub fn identify(nsid: u32, cns: u8, prp1: u64) -> Self {
        Self {
            opcode: NvmeAdminOpcode::Identify as u8,
            flags: 0,
            cid: 0,
            nsid,
            cdw2: 0, cdw3: 0,
            mptr: prp1,
            prp2: 0,
            cdw10: cns as u32,    // CNS (Controller/Namespace)
            cdw11: 0, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0,
        }
    }

    /// 创建 Create I/O Completion Queue 命令
    pub fn create_cq(qid: u16, cq_phys: u64) -> Self {
        Self {
            opcode: NvmeAdminOpcode::CreateCq as u8,
            flags: 0,
            cid: 0,
            nsid: 0,
            cdw2: 0, cdw3: 0,
            mptr: cq_phys,
            prp2: 0,
            cdw10: ((QUEUE_DEPTH as u32 - 1) << 16) | (qid as u32),
            cdw11: 1,    // PC: physically contiguous, IEN: enable
            cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0,
        }
    }

    /// 创建 Create I/O Submission Queue 命令
    pub fn create_sq(qid: u16, cqid: u16, sq_phys: u64) -> Self {
        Self {
            opcode: NvmeAdminOpcode::CreateSq as u8,
            flags: 0,
            cid: 0,
            nsid: 0,
            cdw2: 0, cdw3: 0,
            mptr: sq_phys,
            prp2: 0,
            cdw10: ((QUEUE_DEPTH as u32 - 1) << 16) | (qid as u32),
            cdw11: (cqid as u32) << 16 | 1,   // CQID | PC
            cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0,
        }
    }
}

/// NVMe 完成队列条目 (16字节)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeCompletion {
    pub cdw0: u32,
    pub rsvd1: u32,
    pub sqhd: u16,
    pub sqid: u16,
    pub cid: u16,
    pub status: u16,
}

impl NvmeCompletion {
    /// 阶段标记匹配 = 完成
    pub fn is_completed(&self, phase: u16) -> bool {
        (self.status & 0x01) as u16 == phase
    }

    pub fn status_code(&self) -> u16 {
        (self.status >> 1) & 0x7FF
    }

    pub fn is_success(&self) -> bool {
        self.status_code() == 0
    }
}

// ============================================================================
// NVMe Identify 数据结构 (精简版)
// ============================================================================

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeIdentifyController {
    pub vid: u16,
    pub ssvid: u16,
    pub sn: [u8; 20],
    pub mn: [u8; 40],
    pub fr: [u8; 8],
    pub rsvd1: [u8; 444],    // 跳过大部分字段
    pub nn: u32,              // offset 516: 命名空间数量
    pub rsvd2: [u8; 3756],
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeIdentifyNamespace {
    pub nsze: u64,
    pub ncap: u64,
    pub nuse: u64,
    pub nsfeat: u8,
    pub nlbaf: u8,
    pub flbas: u8,
    pub rsvd: [u8; 4085],
}

// 重新导出保持 API 兼容性
pub use super::ahci::AtaCommand;

/// NVMe 队列对 (为驱动保持 struct 名兼容)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeQueuePair {
    pub qid: u16,
    pub sq: *mut NvmeCommand,
    pub cq: *mut NvmeCompletion,
    pub depth: u32,
    pub sq_tail: u32,
    pub cq_head: u32,
    pub db_stride: u32,
    pub created: bool,
}

impl NvmeQueuePair {
    pub const fn new(qid: u16, depth: u32, db_stride: u32) -> Self {
        Self {
            qid,
            sq: ptr::null_mut(),
            cq: ptr::null_mut(),
            depth,
            sq_tail: 0,
            cq_head: 0,
            db_stride,
            created: false,
        }
    }
}

// ============================================================================
// NVMe 控制器 (DMA-backed)
// ============================================================================

/// NVMe 队列 DMA 资源
struct QueueDma {
    virt: VirtAddr,
    phys: PhysAddr,
    is_cq: bool,
    phase: u16,  // CQ 阶段标记
}

/// NVMe 控制器驱动
pub struct NvmeController {
    mmio_base: usize,
    regs: *mut NvmeControllerRegisters,
    doorbell_base: usize,            // 门铃寄存器基地址
    db_stride: u32,                  // 门铃步长

    // Admin 队列 (DMA 分配的 SQ/CQ)
    admin_sq_dma: QueueDma,
    admin_cq_dma: QueueDma,
    admin_sq_tail: u32,
    admin_cq_head: u32,
    admin_cid: u16,

    // I/O 队列
    io_sq_dma: QueueDma,
    io_cq_dma: QueueDma,
    io_sq_tail: u32,
    io_cq_head: u32,
    io_cid: u16,
    io_phase: u16,

    // Device info
    namespace_count: u32,
    namespace_size_lba: u64,         // 命名空间大小 (LBA)
    lba_format_size: u16,            // LBA 格式字节数
    info: DeviceInfo,
    initialized: bool,
}

impl NvmeController {
    pub fn new(mmio_base: usize) -> Self {
        Self {
            mmio_base,
            regs: ptr::null_mut(),
            doorbell_base: 0,
            db_stride: 0,
            admin_sq_dma: QueueDma { virt: VirtAddr(0), phys: PhysAddr(0), is_cq: false, phase: 0 },
            admin_cq_dma: QueueDma { virt: VirtAddr(0), phys: PhysAddr(0), is_cq: true, phase: 1 },
            admin_sq_tail: 0,
            admin_cq_head: 0,
            admin_cid: 0,
            io_sq_dma: QueueDma { virt: VirtAddr(0), phys: PhysAddr(0), is_cq: false, phase: 0 },
            io_cq_dma: QueueDma { virt: VirtAddr(0), phys: PhysAddr(0), is_cq: true, phase: 1 },
            io_sq_tail: 0,
            io_cq_head: 0,
            io_cid: 0,
            io_phase: 1,
            namespace_count: 0,
            namespace_size_lba: 0,
            lba_format_size: SECTOR_SIZE as u16,
            info: DeviceInfo::new("nvme", DeviceType::Block),
            initialized: false,
        }
    }

    /// 分配 Admin 队列 DMA 内存
    fn alloc_admin_queues(&mut self) -> Result<()> {
        let dma = get_dma();
        if !dma.is_initialized() {
            return Err(DriverError::NotInitialized);
        }

        if let Some((v, p)) = dma.alloc_coherent(SQ_SIZE) {
            self.admin_sq_dma.virt = v;
            self.admin_sq_dma.phys = p;
        } else {
            return Err(DriverError::HardwareError);
        }

        if let Some((v, p)) = dma.alloc_coherent(CQ_SIZE) {
            self.admin_cq_dma.virt = v;
            self.admin_cq_dma.phys = p;
        } else {
            dma.free_coherent(self.admin_sq_dma.virt, SQ_SIZE);
            self.admin_sq_dma = QueueDma { virt: VirtAddr(0), phys: PhysAddr(0), is_cq: false, phase: 0 };
            return Err(DriverError::HardwareError);
        }

        Ok(())
    }

    /// 分配 I/O 队列 DMA 内存
    fn alloc_io_queues(&mut self) -> Result<()> {
        let dma = get_dma();
        if !dma.is_initialized() {
            return Err(DriverError::NotInitialized);
        }

        if let Some((v, p)) = dma.alloc_coherent(SQ_SIZE) {
            self.io_sq_dma.virt = v;
            self.io_sq_dma.phys = p;
        } else {
            return Err(DriverError::HardwareError);
        }

        if let Some((v, p)) = dma.alloc_coherent(CQ_SIZE) {
            self.io_cq_dma.virt = v;
            self.io_cq_dma.phys = p;
        } else {
            dma.free_coherent(self.io_sq_dma.virt, SQ_SIZE);
            self.io_sq_dma = QueueDma { virt: VirtAddr(0), phys: PhysAddr(0), is_cq: false, phase: 0 };
            return Err(DriverError::HardwareError);
        }

        Ok(())
    }

    fn free_queues(&mut self) {
        let dma = get_dma();
        if self.admin_sq_dma.virt.0 != 0 {
            dma.free_coherent(self.admin_sq_dma.virt, SQ_SIZE);
        }
        if self.admin_cq_dma.virt.0 != 0 {
            dma.free_coherent(self.admin_cq_dma.virt, CQ_SIZE);
        }
        if self.io_sq_dma.virt.0 != 0 {
            dma.free_coherent(self.io_sq_dma.virt, SQ_SIZE);
        }
        if self.io_cq_dma.virt.0 != 0 {
            dma.free_coherent(self.io_cq_dma.virt, CQ_SIZE);
        }
    }

    /// 向门铃寄存器写入
    unsafe fn write_doorbell(&self, qid: u16, is_sq: bool, value: u32) {
        let offset = if is_sq {
            0x1000 + (qid as usize * 2 * self.db_stride as usize)
        } else {
            0x1000 + (qid as usize * 2 + 1) * self.db_stride as usize
        };
        let ptr = (self.doorbell_base + offset) as *mut u32;
        ptr.write_volatile(value);
    }

    /// 提交 Admin 命令并等待完成
    unsafe fn submit_admin_command(&mut self, cmd: &NvmeCommand) -> Result<NvmeCompletion> {
        let cid = self.admin_cid;
        self.admin_cid = self.admin_cid.wrapping_add(1);

        // 写入 SQ entry
        let sq = self.admin_sq_dma.virt.0 as *mut NvmeCommand;
        let mut entry_cmd = *cmd;
        entry_cmd.cid = cid;
        sq.add(self.admin_sq_tail as usize).write_volatile(entry_cmd);

        // 更新尾指针并敲门铃
        self.admin_sq_tail = (self.admin_sq_tail + 1) % (QUEUE_DEPTH as u32);
        self.write_doorbell(ADMIN_QUEUE_ID, true, self.admin_sq_tail);

        // 等待完成
        let cq = self.admin_cq_dma.virt.0 as *const NvmeCompletion;
        let mut timeout = 5_000_000u64;
        loop {
            let entry = cq.add(self.admin_cq_head as usize).read_volatile();
            if entry.is_completed(self.admin_cq_dma.phase) {
                // 更新头指针
                let new_head = (self.admin_cq_head + 1) % (QUEUE_DEPTH as u32);
                self.admin_cq_head = new_head;
                if new_head == 0 {
                    self.admin_cq_dma.phase ^= 1;
                }

                // 敲响 CQ 门铃
                self.write_doorbell(ADMIN_QUEUE_ID, false, new_head);

                if !entry.is_success() {
                    return Err(DriverError::HardwareError);
                }
                return Ok(entry);
            }

            timeout -= 1;
            if timeout == 0 {
                return Err(DriverError::Timeout);
            }
            core::hint::spin_loop();
        }
    }

    /// 初始化控制器
    pub fn init_controller(&mut self) -> Result<()> {
        // 分配 Admin 队列
        self.alloc_admin_queues()?;

        unsafe {
            self.regs = self.mmio_base as *mut NvmeControllerRegisters;
            let regs = &mut *self.regs;

            // 读取能力: 门铃步长
            let dstrd = ((regs.cap >> 32) & 0xF) as u32;
            self.db_stride = 1 << dstrd;

            // 门铃寄存器基地址 (紧随 regs struct)
            self.doorbell_base = self.mmio_base + 0x1000;

            // MPS: 使用 4KB (= 0)
            let mps: u32 = 0; // 2^(12 + 0) = 4096

            // ── 禁用控制器 ──
            if regs.csts & csts::RDY != 0 {
                regs.cc = 0;
                let mut timeout = 1_000_000u64;
                while regs.csts & csts::RDY != 0 && timeout > 0 {
                    timeout -= 1;
                    core::hint::spin_loop();
                }
                if timeout == 0 {
                    return Err(DriverError::Timeout);
                }
            }

            // ── 设置 Admin 队列 ──
            regs.aqa = (((QUEUE_DEPTH as u32) - 1) << 16) | ((QUEUE_DEPTH as u32) - 1);
            regs.asq = self.admin_sq_dma.phys.0;
            regs.acq = self.admin_cq_dma.phys.0;

            // ── 启用控制器 ──
            let iocqes: u32 = 4; // log2(16) = 4
            let iosqes: u32 = 6; // log2(64) = 6
            regs.cc = cc::EN | cc::CSS_NVM | (mps << cc::MPS_SHIFT) | cc::AMS_RR
                | (iocqes << cc::IOCQES_SHIFT) | (iosqes << cc::IOSQES_SHIFT);

            let mut timeout = 1_000_000u64;
            while regs.csts & csts::RDY == 0 && timeout > 0 {
                timeout -= 1;
                core::hint::spin_loop();
            }
            if timeout == 0 {
                return Err(DriverError::Timeout);
            }
        }

        Ok(())
    }

    /// 识别控制器
    pub fn identify_controller(&mut self) -> Result<()> {
        let dma = get_dma();
        let (ident_virt, ident_phys) = dma.alloc_coherent(4096)
            .ok_or(DriverError::Busy)?;

        // 清零
        unsafe {
            ptr::write_bytes(ident_virt.0 as *mut u8, 0, 4096);
        }

        let cmd = NvmeCommand::identify(0, 1, ident_phys.0);
        let result = unsafe { self.submit_admin_command(&cmd) };

        if result.is_ok() {
            let ctrl = unsafe { &*(ident_virt.0 as *const NvmeIdentifyController) };
            self.namespace_count = ctrl.nn;

            // 读出型号字符串
            let mut model = [0u8; 41];
            let len = ctrl.mn.iter().position(|&c| c == 0).unwrap_or(40);
            model[..len].copy_from_slice(&ctrl.mn[..len]);
            let model_str = core::str::from_utf8(&model[..len]).unwrap_or("unknown");

            klog_info!(Driver,
                "NVMe: controller identified - model={}, ns_count={}",
                model_str, self.namespace_count
            );
        }

        dma.free_coherent(ident_virt, 4096);
        result.map(|_| ())
    }

    /// 识别命名空间
    pub fn identify_namespace(&mut self, nsid: u32) -> Result<()> {
        let dma = get_dma();
        let (ident_virt, ident_phys) = dma.alloc_coherent(4096)
            .ok_or(DriverError::Busy)?;

        unsafe {
            ptr::write_bytes(ident_virt.0 as *mut u8, 0, 4096);
        }

        let cmd = NvmeCommand::identify(nsid, 0, ident_phys.0);
        let result = unsafe { self.submit_admin_command(&cmd) };

        if result.is_ok() {
            let ns = unsafe { &*(ident_virt.0 as *const NvmeIdentifyNamespace) };
            self.namespace_size_lba = ns.nsze;

            // 获取 LBA 格式
            let flbas = ns.flbas & 0xF;
            let lbaf_idx = flbas as usize;
            if lbaf_idx < 16 {
                // LBA 格式表在 offset 128..384
                let lbaf_ptr = unsafe { (ident_virt.0 as *const u8).add(128 + lbaf_idx * 4) };
                let lbaf_data = unsafe { *(lbaf_ptr as *const u32) };
                let lbads = (lbaf_data >> 16) & 0xFF;
                self.lba_format_size = if lbads > 0 { 1u16 << lbads as u16 } else { SECTOR_SIZE as u16 };
            }

            klog_info!(Driver,
                "NVMe: namespace {} - size={} LBA, block={}B",
                nsid, self.namespace_size_lba, self.lba_format_size
            );
        }

        dma.free_coherent(ident_virt, 4096);
        result.map(|_| ())
    }

    /// 创建 I/O 队列
    pub fn create_io_queue(&mut self) -> Result<()> {
        self.alloc_io_queues()?;

        // 创建 I/O Completion Queue
        let cmd_cq = NvmeCommand::create_cq(IO_QUEUE_ID, self.io_cq_dma.phys.0);
        unsafe { self.submit_admin_command(&cmd_cq)?; }

        // 创建 I/O Submission Queue
        let cmd_sq = NvmeCommand::create_sq(IO_QUEUE_ID, IO_QUEUE_ID, self.io_sq_dma.phys.0);
        unsafe { self.submit_admin_command(&cmd_sq)?; }

        self.io_sq_tail = 0;
        self.io_cq_head = 0;
        self.io_phase = 1;
        self.io_cid = 0;

        Ok(())
    }

    /// 提交 I/O 命令并等待完成
    unsafe fn submit_io_command(&mut self, cmd: &NvmeCommand) -> Result<()> {
        let cid = self.io_cid;
        self.io_cid = self.io_cid.wrapping_add(1);

        let sq = self.io_sq_dma.virt.0 as *mut NvmeCommand;
        let mut entry_cmd = *cmd;
        entry_cmd.cid = cid;
        sq.add(self.io_sq_tail as usize).write_volatile(entry_cmd);

        let new_tail = (self.io_sq_tail + 1) % (QUEUE_DEPTH as u32);
        self.io_sq_tail = new_tail;

        self.write_doorbell(IO_QUEUE_ID, true, new_tail);

        // 等待完成
        let cq = self.io_cq_dma.virt.0 as *const NvmeCompletion;
        let mut timeout = 5_000_000u64;
        loop {
            let entry = cq.add(self.io_cq_head as usize).read_volatile();
            if entry.is_completed(self.io_phase) {
                let new_head = (self.io_cq_head + 1) % (QUEUE_DEPTH as u32);
                self.io_cq_head = new_head;
                if new_head == 0 {
                    self.io_phase ^= 1;
                }

                self.write_doorbell(IO_QUEUE_ID, false, new_head);

                if !entry.is_success() {
                    return Err(DriverError::HardwareError);
                }
                return Ok(());
            }

            timeout -= 1;
            if timeout == 0 {
                return Err(DriverError::Timeout);
            }
            core::hint::spin_loop();
        }
    }

    /// 读取扇区
    pub fn read(&mut self, nsid: u32, lba: u64, count: u16, buffer: *mut u8) -> Result<()> {
        if !self.initialized {
            return Err(DriverError::NotInitialized);
        }
        if count == 0 || count > MAX_SECTORS_PER_CMD {
            return Err(DriverError::InvalidParameter);
        }

        let byte_count = (count as usize) * self.lba_format_size as usize;

        // 分配 DMA buffer
        let dma = get_dma();
        let (buf_virt, buf_phys) = dma.alloc_coherent(byte_count)
            .ok_or(DriverError::Busy)?;

        let nlb = ((byte_count + (self.lba_format_size as usize) - 1)
            / (self.lba_format_size as usize)) as u16;

        let cmd = NvmeCommand::read(nsid, lba, nlb, buf_phys.0);
        let result = unsafe { self.submit_io_command(&cmd) };

        if result.is_ok() {
            unsafe {
                ptr::copy_nonoverlapping(buf_virt.0 as *const u8, buffer, byte_count);
            }
        }

        dma.free_coherent(buf_virt, byte_count);
        result
    }

    /// 写入扇区
    pub fn write(&mut self, nsid: u32, lba: u64, count: u16, buffer: *const u8) -> Result<()> {
        if !self.initialized {
            return Err(DriverError::NotInitialized);
        }
        if count == 0 || count > MAX_SECTORS_PER_CMD {
            return Err(DriverError::InvalidParameter);
        }

        let byte_count = (count as usize) * self.lba_format_size as usize;

        let dma = get_dma();
        let (buf_virt, buf_phys) = dma.alloc_coherent(byte_count)
            .ok_or(DriverError::Busy)?;

        // 复制数据到 DMA buffer
        unsafe {
            ptr::copy_nonoverlapping(buffer, buf_virt.0 as *mut u8, byte_count);
        }

        let nlb = ((byte_count + (self.lba_format_size as usize) - 1)
            / (self.lba_format_size as usize)) as u16;

        let cmd = NvmeCommand::write(nsid, lba, nlb, buf_phys.0);
        let result = unsafe { self.submit_io_command(&cmd) };

        dma.free_coherent(buf_virt, byte_count);
        result
    }

    pub fn namespace_count(&self) -> u32 { self.namespace_count }
    pub fn namespace_size(&self) -> u64 { self.namespace_size_lba }
}

// SAFETY: NvmeController uses MMIO registers via volatile access.
// Global NVME_CONTROLLERS Mutex protects concurrent cross-CPU mutation.
unsafe impl Send for NvmeController {}
unsafe impl Sync for NvmeController {}

// ============================================================================
// Driver Trait 实现
// ============================================================================

impl Driver for NvmeController {
    fn name(&self) -> &'static str { "NVMe Controller" }
    fn device_type(&self) -> DeviceType { DeviceType::Block }

    fn init(&mut self) -> Result<()> {
        // Ensure init_controller is separate from the full init flow
        self.init_controller()?;
        self.identify_controller()?;

        // Identify namespace 1
        if self.namespace_count > 0 {
            self.identify_namespace(1)?;
        }

        // Create I/O queue pair
        self.create_io_queue()?;

        self.initialized = true;
        klog_info!(Driver, "NVMe: controller fully initialized, {} ns", self.namespace_count);
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        // 关机通知
        unsafe {
            let regs = &mut *self.regs;
            let shn: u32 = 1 << 14; // Normal shutdown
            regs.cc = (regs.cc & !0x3C000) | shn;

            let mut timeout = 1_000_000u64;
            while regs.csts & (0x3 << 2) != (2 << 2) && timeout > 0 {
                timeout -= 1;
                core::hint::spin_loop();
            }
        }

        self.free_queues();
        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool { self.initialized }
    fn status(&self) -> &'static str {
        if self.initialized { "NVMe ready" } else { "NVMe not initialized" }
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nvme_command_read() {
        let cmd = NvmeCommand::read(1, 0, 1, 0x1000);
        assert_eq!(cmd.opcode, NvmeNvmOpcode::Read as u8);
        assert_eq!(cmd.nsid, 1);
        assert_eq!(cmd.cdw12, 0); // NLB-1 = 0
    }

    #[test]
    fn test_nvme_command_write() {
        let cmd = NvmeCommand::write(1, 100, 8, 0x2000);
        assert_eq!(cmd.opcode, NvmeNvmOpcode::Write as u8);
        assert_eq!(cmd.cdw10, 100);
        assert_eq!(cmd.cdw12, 7); // 8 NLB -> 7
    }

    #[test]
    fn test_nvme_completion() {
        let mut cq = NvmeCompletion {
            cdw0: 0, rsvd1: 0, sqhd: 0, sqid: 0, cid: 0,
            status: 0x0001, // Phase=1, Status=0
        };
        assert!(cq.is_completed(1));
        assert!(cq.is_success());

        cq.status = 0x0003; // Phase=1, Status=1
        assert!(!cq.is_success());
        assert_eq!(cq.status_code(), 1);
    }

    #[test]
    fn test_nvme_controller_creation() {
        let ctrl = NvmeController::new(0xFE000000);
        assert_eq!(ctrl.name(), "NVMe Controller");
        assert_eq!(ctrl.device_type(), DeviceType::Block);
        assert!(!ctrl.is_ready());
    }

    #[test]
    fn test_command_sizes() {
        assert_eq!(core::mem::size_of::<NvmeCommand>(), 64);
        assert_eq!(core::mem::size_of::<NvmeCompletion>(), 16);
    }
}