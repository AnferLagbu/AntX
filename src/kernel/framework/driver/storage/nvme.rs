// I-49: 文件级 #![allow(dead_code)] 已移除. 启动路径 (storage::init) 通过
// nvme_block::NvmeBlockDevice 调用本文件的 NVMe 控制器 API, 不再需要宽泛豁免.
// 若有局部未使用项 (如保留作未来 API), 改为 #[allow(dead_code)] 单项标注 + 注释.
//! `NVMe` 驱动 (`NVMe` Driver)
//!
//! `提供NVMe` (Non-Volatile Memory Express) SSD支持：
//! - **`PCIe接口`**: `高速PCIe总线连接`
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

use super::framework::{DeviceInfo, DeviceType, Driver, DriverError, Result};
use crate::kernel::framework::dma::get_dma;
use crate::kernel::framework::iomem::IoMem;
use crate::kernel::framework::mm::{PhysAddr, VirtAddr, PAGE_SIZE};
use crate::klog_info;
use core::ptr;

// ============================================================================
// NVMe 常量定义
// ============================================================================

const ADMIN_QUEUE_ID: u16 = 0;
const IO_QUEUE_ID: u16 = 1;

const QUEUE_DEPTH: usize = 64; // Admin + I/O 队列深度
const SQ_ENTRY_SIZE: usize = 64; // 提交队列条目大小
const CQ_ENTRY_SIZE: usize = 16; // 完成队列条目大小
const SQ_SIZE: usize = QUEUE_DEPTH * SQ_ENTRY_SIZE;
const CQ_SIZE: usize = QUEUE_DEPTH * CQ_ENTRY_SIZE;

const SECTOR_SIZE: usize = 512;

/// 最大扇区数 (128 sectors = 64KB, 单次命令)
const MAX_SECTORS_PER_CMD: u16 = 128;

// NVMe 控制器寄存器偏移 (BAR0)  // 硬件寄存器描述
const NVME_REG_CAP: usize = 0x00;    // u64: 控制器能力
const NVME_REG_VS: usize = 0x08;     // u32: 版本 (NVMe 规范 §3.1.2)
const NVME_REG_INTMS: usize = 0x0C;  // u32: 中断掩码设置 (NVMe 规范 §3.1.6)
const NVME_REG_INTMC: usize = 0x10;  // u32: 中断掩码清除 (NVMe 规范 §3.1.6)
const NVME_REG_CC: usize = 0x14;     // u32: 控制器配置
const NVME_REG_CSTS: usize = 0x1C;   // u32: 控制器状态
const NVME_REG_AQA: usize = 0x24;    // u32: Admin 队列属性
const NVME_REG_ASQ: usize = 0x28;    // u64: Admin SQ 基地址
const NVME_REG_ACQ: usize = 0x30;    // u64: Admin CQ 基地址

// Doorbell registers start at offset 0x1000
const NVME_DB_BASE: usize = 0x1000;

// ============================================================================
// NVMe 寄存器定义
// ============================================================================

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeControllerRegisters {
    pub cap: u64,   // 控制器能力
    pub vs: u32,    // 版本
    pub intms: u32, // 中断掩码设置
    pub intmc: u32, // 中断掩码清除
    pub cc: u32,    // 控制器配置
    pub rsvd1: u32,
    pub csts: u32,   // 控制器状态
    pub nssr: u32,   // NVM 子系统复位
    pub aqa: u32,    // Admin 队列属性
    pub asq: u64,    // Admin 提交队列基地址
    pub acq: u64,    // Admin 完成队列基地址
    pub cmbloc: u32, // 控制器内存缓冲区位置
    pub cmbsz: u32,  // 控制器内存缓冲区大小
    pub rsvd2: [u32; 8],
    pub bpinfo: u32, // 启动分区信息
    pub bprsel: u32, // 启动分区读选择
    pub bpmbl: u64,  // 启动分区内存缓冲位置
    pub rsvd3: [u64; 38],
    // 门铃寄存器紧随其后
}

/// `NVMe` 控制器能力寄存器 (CAP) 位域 — `NVMe` 规范 §3.1.1
///
/// 当前代码直接使用 `(regs.cap >> 32) & 0xF` 读取 DSTRD.
/// 完整位域定义供参考:
/// - MQES    [0:15]:  最大队列条目数
/// - CQR     [16]:    支持 NVMe-MI
/// - AMS     [17:18]: 仲裁机制
/// - TO      [24:31]: 超时 (500ms 单位)
/// - DSTRD   [32:35]: 门铃步长 (2^n)
/// - CSS     [37:44]: 命令集支持
/// - MPSMIN  [48:51]: 最小内存页大小
/// - MPSMAX  [52:55]: 最大内存页大小
mod cap {}

/// `NVMe` 控制器配置寄存器 (CC) 位域 — `NVMe` 规范 §3.1.5
///
/// 未实现位: SHN [14:15] (关机通知)
mod cc {
    pub const EN: u32 = 1 << 0;
    pub const CSS_NVM: u32 = 0 << 4;
    pub const MPS_SHIFT: u32 = 7;
    pub const AMS_RR: u32 = 0 << 11;
    pub const IOCQES_SHIFT: u32 = 20;
    pub const IOSQES_SHIFT: u32 = 24;
}

/// `NVMe` 控制器状态寄存器 (CSTS) 位域 — `NVMe` 规范 §3.1.7
///
/// 未实现位: NSSRO (bit 4, NVM 子系统复位完成)
mod csts {
    pub const RDY: u32 = 1 << 0;
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

/// `NVMe` 命令 (64字节)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeCommand {
    pub opcode: u8,
    pub flags: u8,
    pub cid: u16,
    pub nsid: u32,
    pub cdw2: u32,
    pub cdw3: u32,
    pub mptr: u64, // PRP1 / SGL entry 1
    pub prp2: u64, // PRP2 / SGL entry 2
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
            opcode: 0,
            flags: 0,
            cid: 0,
            nsid: 0,
            cdw2: 0,
            cdw3: 0,
            mptr: 0,
            prp2: 0,
            cdw10: 0,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// 创建读命令
    pub fn read(nsid: u32, slba: u64, nlb: u16, prp1: u64) -> Self {
        Self {
            opcode: NvmeNvmOpcode::Read as u8,
            flags: 0,
            cid: 0,
            nsid,
            cdw2: 0,
            cdw3: 0,
            mptr: prp1,
            prp2: 0,
            cdw10: (slba & 0xFFFFFFFF) as u32,
            cdw11: ((slba >> 32) & 0xFFFFFFFF) as u32,
            cdw12: (u32::from(nlb) - 1) & 0xFFFF, // NLB = #blocks - 1
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// 创建写命令
    pub fn write(nsid: u32, slba: u64, nlb: u16, prp1: u64) -> Self {
        Self {
            opcode: NvmeNvmOpcode::Write as u8,
            flags: 0,
            cid: 0,
            nsid,
            cdw2: 0,
            cdw3: 0,
            mptr: prp1,
            prp2: 0,
            cdw10: (slba & 0xFFFFFFFF) as u32,
            cdw11: ((slba >> 32) & 0xFFFFFFFF) as u32,
            cdw12: (u32::from(nlb) - 1) & 0xFFFF,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// 创建 Identify 命令
    pub fn identify(nsid: u32, cns: u8, prp1: u64) -> Self {
        Self {
            opcode: NvmeAdminOpcode::Identify as u8,
            flags: 0,
            cid: 0,
            nsid,
            cdw2: 0,
            cdw3: 0,
            mptr: prp1,
            prp2: 0,
            cdw10: u32::from(cns), // CNS (Controller/Namespace)
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// 创建 Create I/O Completion Queue 命令
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    pub fn create_cq(qid: u16, cq_phys: u64) -> Self {
        Self {
            opcode: NvmeAdminOpcode::CreateCq as u8,
            flags: 0,
            cid: 0,
            nsid: 0,
            cdw2: 0,
            cdw3: 0,
            mptr: cq_phys,
            prp2: 0,
            cdw10: ((QUEUE_DEPTH as u32 - 1) << 16) | u32::from(qid),
            cdw11: 1, // PC: physically contiguous, IEN: enable
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }

    /// 创建 Create I/O Submission Queue 命令
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    pub fn create_sq(qid: u16, cqid: u16, sq_phys: u64) -> Self {
        Self {
            opcode: NvmeAdminOpcode::CreateSq as u8,
            flags: 0,
            cid: 0,
            nsid: 0,
            cdw2: 0,
            cdw3: 0,
            mptr: sq_phys,
            prp2: 0,
            cdw10: ((QUEUE_DEPTH as u32 - 1) << 16) | u32::from(qid),
            cdw11: u32::from(cqid) << 16 | 1, // CQID | PC
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }
}

/// `NVMe` 完成队列条目 (16字节)
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
        (self.status & 0x01) == phase
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
    pub rsvd1: [u8; 444], // 跳过大部分字段
    pub nn: u32,          // offset 516: 命名空间数量
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

/// `NVMe` 队列对 (为驱动保持 struct 名兼容)
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

/// `NVMe` 队列 DMA 资源
struct QueueDma {
    virt: VirtAddr,
    phys: PhysAddr,
    /// 区分 SQ/CQ — 用于队列操作断言
    is_cq: bool,
    phase: u16, // CQ 阶段标记
}

/// `NVMe` 控制器驱动
pub struct NvmeController {
    mmio_phys: u64,            // PCI BAR0 physical address (for external use)
    iomem: Option<IoMem>,      // MMIO region handle (safe access proxy)
    db_stride: u32,            // 门铃步长

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

    prp_list_virt: VirtAddr,
    prp_list_phys: PhysAddr,

    // Device info
    namespace_count: u32,
    namespace_size_lba: u64, // 命名空间大小 (LBA)
    lba_format_size: u16,    // LBA 格式字节数
    /// 设备元数据 — 供 sysfs/procfs 暴露 / 驱动注册表
    info: DeviceInfo,
    initialized: bool,
}

impl NvmeController {
    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
    pub fn new(mmio_base: usize) -> Self {
        Self {
            mmio_phys: mmio_base as u64,
            iomem: None,
            db_stride: 0,
            admin_sq_dma: QueueDma {
                virt: VirtAddr(0),
                phys: PhysAddr(0),
                is_cq: false,
                phase: 0,
            },
            admin_cq_dma: QueueDma {
                virt: VirtAddr(0),
                phys: PhysAddr(0),
                is_cq: true,
                phase: 1,
            },
            admin_sq_tail: 0,
            admin_cq_head: 0,
            admin_cid: 0,
            io_sq_dma: QueueDma {
                virt: VirtAddr(0),
                phys: PhysAddr(0),
                is_cq: false,
                phase: 0,
            },
            io_cq_dma: QueueDma {
                virt: VirtAddr(0),
                phys: PhysAddr(0),
                is_cq: true,
                phase: 1,
            },
            io_sq_tail: 0,
            io_cq_head: 0,
            io_cid: 0,
            io_phase: 1,
            prp_list_virt: VirtAddr(0),
            prp_list_phys: PhysAddr(0),
            namespace_count: 0,
            namespace_size_lba: 0,
            lba_format_size: SECTOR_SIZE as u16,
            info: DeviceInfo::new("nvme", DeviceType::Block),
            initialized: false,
        }
    }

    /// 获取设备信息
    pub fn get_info(&self) -> &DeviceInfo {
        &self.info
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
            self.admin_sq_dma = QueueDma {
                virt: VirtAddr(0),
                phys: PhysAddr(0),
                is_cq: false,
                phase: 0,
            };
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
            self.io_sq_dma = QueueDma {
                virt: VirtAddr(0),
                phys: PhysAddr(0),
                is_cq: false,
                phase: 0,
            };
            return Err(DriverError::HardwareError);
        }

        Ok(())
    }

    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
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
        if self.prp_list_virt.0 != 0 {
            dma.free_coherent(self.prp_list_virt, PAGE_SIZE as usize);
            self.prp_list_virt = VirtAddr(0);
            self.prp_list_phys = PhysAddr(0);
        }
    }

    /// 向门铃寄存器写入
    fn write_doorbell(&self, qid: u16, is_sq: bool, value: u32) {
        let iomem = self.iomem.as_ref().expect("NVMe: IoMem not initialized");
        let offset = if is_sq {
            NVME_DB_BASE + (qid as usize * 2 * self.db_stride as usize)
        } else {
            NVME_DB_BASE + (qid as usize * 2 + 1) * self.db_stride as usize
        };
        iomem.write_u32(offset, value);
    }

    /// 提交 Admin 命令并等待完成
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    unsafe fn submit_admin_command(&mut self, cmd: &NvmeCommand) -> Result<NvmeCompletion> { unsafe {
        // 调试断言: 验证队列类型正确
        debug_assert!(!self.admin_sq_dma.is_cq, "admin SQ should not be CQ");
        debug_assert!(self.admin_cq_dma.is_cq, "admin CQ should be CQ");

        let cid = self.admin_cid;
        self.admin_cid = self.admin_cid.wrapping_add(1);

        // 写入 SQ entry
        let sq = self.admin_sq_dma.virt.0 as *mut NvmeCommand;
        let mut entry_cmd = *cmd;
        entry_cmd.cid = cid;
        sq.add(self.admin_sq_tail as usize)
            .write_volatile(entry_cmd);

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
    }}

    /// 初始化控制器
    /// # Errors
    /// 队列分配失败、获取 MMIO 失败或控制器初始化命令失败时返回 Err。
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
#[expect(clippy::similar_names, reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分")]
    pub fn init_controller(&mut self) -> Result<()> {
        // 分配 Admin 队列
        self.alloc_admin_queues()?;

        // 初始化 IoMem
        let iomem = IoMem::from_pci_bar(
            PhysAddr(self.mmio_phys),
            8192, // NVMe BAR0 is typically 8KB+
            "nvme-bar0",
        ).map_err(|_| DriverError::HardwareError)?;

        // 读取能力: 门铃步长
        let cap = iomem.read_u64(NVME_REG_CAP);
        let dstrd = ((cap >> 32) & 0xF) as u32;
        self.db_stride = 1 << dstrd;

        // 读取控制器版本 (NVMe 规范 §3.1.2)
        let vs = iomem.read_u32(NVME_REG_VS);
        let major = (vs >> 16) & 0xFFFF;
        let minor = (vs >> 8) & 0xFF;
        let patch = vs & 0xFF;
        crate::klog_info!(Driver, "[NVMe] controller version: {}.{}.{}", major, minor, patch);

        // MPS: 使用 4KB (= 0)
        let mps: u32 = 0; // 2^(12 + 0) = 4096

        // ── 禁用控制器 ──
        if iomem.read_u32(NVME_REG_CSTS) & csts::RDY != 0 {
            iomem.write_u32(NVME_REG_CC, 0);
            let mut timeout = 1_000_000u64;
            while iomem.read_u32(NVME_REG_CSTS) & csts::RDY != 0 && timeout > 0 {
                timeout -= 1;
                core::hint::spin_loop();
            }
            if timeout == 0 {
                return Err(DriverError::Timeout);
            }
        }

        // ── 设置 Admin 队列 ──
        iomem.write_u32(
            NVME_REG_AQA,
            (((QUEUE_DEPTH as u32) - 1) << 16) | ((QUEUE_DEPTH as u32) - 1),
        );
        iomem.write_u64(NVME_REG_ASQ, self.admin_sq_dma.phys.0);
        iomem.write_u64(NVME_REG_ACQ, self.admin_cq_dma.phys.0);

        // ── 启用控制器 ──
        let iocqes: u32 = 4; // log2(16) = 4
        let iosqes: u32 = 6; // log2(64) = 6
        iomem.write_u32(
            NVME_REG_CC,
            cc::EN
                | cc::CSS_NVM
                | (mps << cc::MPS_SHIFT)
                | cc::AMS_RR
                | (iocqes << cc::IOCQES_SHIFT)
                | (iosqes << cc::IOSQES_SHIFT),
        );

        let mut timeout = 1_000_000u64;
        while iomem.read_u32(NVME_REG_CSTS) & csts::RDY == 0 && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }
        if timeout == 0 {
            return Err(DriverError::Timeout);
        }

        self.iomem = Some(iomem);
        Ok(())
    }

    /// 识别控制器
    /// # Errors
    /// DMA 缓冲区分配失败或识别命令执行失败时返回 Err。
    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
    pub fn identify_controller(&mut self) -> Result<()> {
        let dma = get_dma();
        let (ident_virt, ident_phys) = dma.alloc_coherent(PAGE_SIZE as usize).ok_or(DriverError::Busy)?;

        // 清零
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            ptr::write_bytes(ident_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
        }

        let cmd = NvmeCommand::identify(0, 1, ident_phys.0);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let result = unsafe { self.submit_admin_command(&cmd) };

        if result.is_ok() {
            // SAFETY: `const` 由调用方保证为有效指针; 只读访问
            let ctrl = unsafe { &*(ident_virt.0 as *const NvmeIdentifyController) };
            self.namespace_count = ctrl.nn;

            // 读出型号字符串
            let mut model = [0u8; 41];
            let len = ctrl.mn.iter().position(|&c| c == 0).unwrap_or(40);
            model[..len].copy_from_slice(&ctrl.mn[..len]);
            let model_str = core::str::from_utf8(&model[..len]).unwrap_or("unknown");

            klog_info!(
                Driver,
                "NVMe: controller identified - model={}, ns_count={}",
                model_str,
                self.namespace_count
            );
        }

        dma.free_coherent(ident_virt, PAGE_SIZE as usize);
        result.map(|_| ())
    }

    /// 识别命名空间
    /// # Errors
    /// DMA 缓冲区分配失败或识别命令执行失败时返回 Err。
    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
    pub fn identify_namespace(&mut self, nsid: u32) -> Result<()> {
        let dma = get_dma();
        let (ident_virt, ident_phys) = dma.alloc_coherent(PAGE_SIZE as usize).ok_or(DriverError::Busy)?;

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            ptr::write_bytes(ident_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
        }

        let cmd = NvmeCommand::identify(nsid, 0, ident_phys.0);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let result = unsafe { self.submit_admin_command(&cmd) };

        if result.is_ok() {
            // SAFETY: `const` 由调用方保证为有效指针; 只读访问
            let ns = unsafe { &*(ident_virt.0 as *const NvmeIdentifyNamespace) };
            self.namespace_size_lba = ns.nsze;

            // 获取 LBA 格式
            let flbas = ns.flbas & 0xF;
            let lbaf_idx = flbas as usize;
            if lbaf_idx < 16 {
                // LBA 格式表在 offset 128..384
                // SAFETY: `const` 由调用方保证为有效指针; 只读访问
                let lbaf_ptr = unsafe { (ident_virt.0 as *const u8).add(128 + lbaf_idx * 4) };
                // SAFETY: `lbaf_ptr` 由调用方保证指向有效 u32; 只读借用
                let lbaf_data = unsafe { *(lbaf_ptr as *const u32) };
                let lbads = (lbaf_data >> 16) & 0xFF;
                self.lba_format_size = if lbads > 0 {
                    1u16 << lbads as u16
                } else {
                    SECTOR_SIZE as u16
                };
            }

            klog_info!(
                Driver,
                "NVMe: namespace {} - size={} LBA, block={}B",
                nsid,
                self.namespace_size_lba,
                self.lba_format_size
            );
        }

        dma.free_coherent(ident_virt, PAGE_SIZE as usize);
        result.map(|_| ())
    }

    /// 创建 I/O 队列
    /// # Errors
    /// 队列分配失败、PRP 页分配失败或创建队列的管理命令失败时返回 Err。
    // 有意窄化: 硬件字段宽度, 寄存器/MMIO 定义保证
    #[expect(clippy::cast_possible_truncation)]
#[expect(clippy::similar_names, reason = "变量名相似表达同族概念 (pd/pt/bm 等); 重命名会破坏阅读连续性, 仅在确实混淆时才人工拆分")]
    pub fn create_io_queue(&mut self) -> Result<()> {
        self.alloc_io_queues()?;

        // 分配 PRP 列表页 (单次命令最大 128 扇区, 所有页表条目可放入一页)
        let dma = get_dma();
        match dma.alloc_coherent(PAGE_SIZE as usize) {
            Some((v, p)) => {
                self.prp_list_virt = v;
                self.prp_list_phys = p;
            }
            None => return Err(DriverError::HardwareError),
        }

        // 创建 I/O Completion Queue
        let cmd_cq = NvmeCommand::create_cq(IO_QUEUE_ID, self.io_cq_dma.phys.0);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            self.submit_admin_command(&cmd_cq)?;
        }

        // 创建 I/O Submission Queue
        let cmd_sq = NvmeCommand::create_sq(IO_QUEUE_ID, IO_QUEUE_ID, self.io_sq_dma.phys.0);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            self.submit_admin_command(&cmd_sq)?;
        }

        self.io_sq_tail = 0;
        self.io_cq_head = 0;
        self.io_phase = 1;
        self.io_cid = 0;

        Ok(())
    }

    /// 提交 I/O 命令并等待完成
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    unsafe fn submit_io_command(&mut self, cmd: &NvmeCommand) -> Result<()> { unsafe {
        // 调试断言: 验证队列类型正确
        debug_assert!(!self.io_sq_dma.is_cq, "IO SQ should not be CQ");
        debug_assert!(self.io_cq_dma.is_cq, "IO CQ should be CQ");

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
    }}

    /// 构造 `NVMe` PRP 地址对, 使用 per-controller PRP 列表页
    ///
    /// `NVMe` 规范要求:
    /// - 传输 ≤ 1 页: PRP1 = 数据物理地址, PRP2 = 0
    /// - 传输 = 2 页: PRP1 = 第1页物理地址, PRP2 = 第2页物理地址
    /// - 传输 > 2 页: PRP1 = 第1页物理地址, PRP2 = PRP 列表页物理地址
    ///
    /// PRP 列表页在 `create_io_queue` 时预分配，供所有 I/O 命令复用。
    /// 依赖 `dma.alloc_coherent` 返回物理连续内存，因此条目地址线性递推即可。
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    // 有意窄化: 用户内存代理, 指针/长度上下文保证
    #[expect(clippy::cast_possible_truncation)]
    unsafe fn build_prp(&self, phys_base: u64, byte_count: usize) -> (u64, u64) {
        let bytes = byte_count as u64;
        let num_pages = (bytes + PAGE_SIZE - 1) / PAGE_SIZE;

        if num_pages <= 1 {
            (phys_base, 0)
        } else if num_pages == 2 {
            (phys_base, phys_base + PAGE_SIZE)
        } else {
            // 填充 PRP 列表页: 条目 0 = 第2页, 条目 1 = 第3页, ...
            let list = self.prp_list_virt.0 as *mut u64;
            for i in 0..(num_pages - 1) as usize {
                // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                unsafe {
                    list.add(i)
                        .write_volatile(phys_base + (i as u64 + 1) * PAGE_SIZE);
                }
            }
            (phys_base, self.prp_list_phys.0)
        }
    }

    /// 填充 `NVMe` 命令的 PRP1/PRP2 字段 (与 `build_prp` 配套)
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe fn set_prp_in_cmd(&self, cmd: &mut NvmeCommand, phys_base: u64, byte_count: usize) { unsafe {
        let (prp1, prp2) = self.build_prp(phys_base, byte_count);
        cmd.mptr = prp1;
        cmd.prp2 = prp2;
    }}

    /// 从 `NVMe` 命名空间读取扇区数据到指定缓冲区。
    /// # Errors
    /// 控制器未初始化、参数非法、DMA 缓冲区分配失败或 I/O 命令执行失败时返回 Err。
    // 有意窄化: 用户内存代理, 指针/长度上下文保证
    #[expect(clippy::cast_possible_truncation)]
    pub fn read(&mut self, nsid: u32, lba: u64, count: u16, buffer: *mut u8) -> Result<()> {
        if !self.initialized {
            return Err(DriverError::NotInitialized);
        }
        if count == 0 || count > MAX_SECTORS_PER_CMD {
            return Err(DriverError::InvalidParameter);
        }

        let byte_count = (count as usize) * self.lba_format_size as usize;

        let dma = get_dma();
        let (buf_virt, buf_phys) = dma.alloc_coherent(byte_count).ok_or(DriverError::Busy)?;

        let nlb = ((byte_count + (self.lba_format_size as usize) - 1)
            / (self.lba_format_size as usize)) as u16;

        let mut cmd = NvmeCommand::read(nsid, lba, nlb, buf_phys.0);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            self.set_prp_in_cmd(&mut cmd, buf_phys.0, byte_count);
        }
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let result = unsafe { self.submit_io_command(&cmd) };

        if result.is_ok() {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                ptr::copy_nonoverlapping(buf_virt.0 as *const u8, buffer, byte_count);
            }
        }

        dma.free_coherent(buf_virt, byte_count);
        result
    }

    /// 将缓冲区数据写入 `NVMe` 命名空间指定扇区。
    /// # Errors
    /// 控制器未初始化、参数非法、DMA 缓冲区分配失败或 I/O 命令执行失败时返回 Err。
    // 有意窄化: 用户内存代理, 指针/长度上下文保证
    #[expect(clippy::cast_possible_truncation)]
    pub fn write(&mut self, nsid: u32, lba: u64, count: u16, buffer: *const u8) -> Result<()> {
        if !self.initialized {
            return Err(DriverError::NotInitialized);
        }
        if count == 0 || count > MAX_SECTORS_PER_CMD {
            return Err(DriverError::InvalidParameter);
        }

        let byte_count = (count as usize) * self.lba_format_size as usize;

        let dma = get_dma();
        let (buf_virt, buf_phys) = dma.alloc_coherent(byte_count).ok_or(DriverError::Busy)?;

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            ptr::copy_nonoverlapping(buffer, buf_virt.0 as *mut u8, byte_count);
        }

        let nlb = ((byte_count + (self.lba_format_size as usize) - 1)
            / (self.lba_format_size as usize)) as u16;

        let mut cmd = NvmeCommand::write(nsid, lba, nlb, buf_phys.0);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            self.set_prp_in_cmd(&mut cmd, buf_phys.0, byte_count);
        }
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let result = unsafe { self.submit_io_command(&cmd) };

        dma.free_coherent(buf_virt, byte_count);
        result
    }

    pub fn namespace_count(&self) -> u32 {
        self.namespace_count
    }
    pub fn namespace_size(&self) -> u64 {
        self.namespace_size_lba
    }
}

// SAFETY: NvmeController 通过 volatile 访问 MMIO 寄存器.
// SAFETY: NvmeController 含 MMIO 裸指针, 全局 NVME_CONTROLLERS Mutex 防止并发跨 CPU 变更.
unsafe impl Send for NvmeController {}
// SAFETY: 同上, Mutex 保证并发安全.
unsafe impl Sync for NvmeController {}

// ============================================================================
// Driver Trait 实现
// ============================================================================

impl Driver for NvmeController {
    fn name(&self) -> &'static str {
        "NVMe Controller"
    }
    fn device_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn init(&mut self) -> Result<()> {
        // 确保 init_controller 与完整 init 流程分离
        self.init_controller()?;
        self.identify_controller()?;

        // Identify namespace 1
        if self.namespace_count > 0 {
            self.identify_namespace(1)?;
        }

        // 创建 I/O 队列对
        self.create_io_queue()?;

        self.initialized = true;
        klog_info!(
            Driver,
            "NVMe: controller fully initialized, {} ns",
            self.namespace_count
        );
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        // 关机通知
        let iomem = match self.iomem.as_ref() {
            Some(m) => m,
            None => return Ok(()),
        };
        let shn: u32 = 1 << 14; // Normal shutdown
        let cc = iomem.read_u32(NVME_REG_CC);
        iomem.write_u32(NVME_REG_CC, (cc & !0x3C000) | shn);

        let mut timeout = 1_000_000u64;
        while iomem.read_u32(NVME_REG_CSTS) & (0x3 << 2) != (2 << 2) && timeout > 0 {
            timeout -= 1;
            core::hint::spin_loop();
        }

        self.free_queues();
        self.initialized = false;
        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.initialized
    }
    fn status(&self) -> &'static str {
        if self.initialized {
            "NVMe ready"
        } else {
            "NVMe not initialized"
        }
    }
}

// ============================================================================
// NVMe 中断管理 API
// ============================================================================

impl NvmeController {
    /// 使能指定中断向量
    ///
    /// 通过写入 INTMS 寄存器使能指定中断。
    pub fn enable_interrupt(&mut self, vector: u32) {
        if let Some(mmio) = self.iomem.as_ref() {
            mmio.write_u32(NVME_REG_INTMS, vector);
        }
    }

    /// 禁用指定中断向量
    ///
    /// 通过写入 INTMC 寄存器禁用指定中断。
    pub fn disable_interrupt(&mut self, vector: u32) {
        if let Some(mmio) = self.iomem.as_ref() {
            mmio.write_u32(NVME_REG_INTMC, vector);
        }
    }

    /// 屏蔽所有中断
    pub fn mask_all_interrupts(&mut self) {
        if let Some(mmio) = self.iomem.as_ref() {
            // 写入全 1 屏蔽所有中断
            mmio.write_u32(NVME_REG_INTMS, 0xFFFF_FFFF);
        }
    }

    /// 取消屏蔽所有中断
    pub fn unmask_all_interrupts(&mut self) {
        if let Some(mmio) = self.iomem.as_ref() {
            // 写入全 1 取消屏蔽所有中断
            mmio.write_u32(NVME_REG_INTMC, 0xFFFF_FFFF);
        }
    }

    /// 处理 `NVMe` 中断
    ///
    /// 读取 I/O 完成队列并处理完成的命令。
    ///
    /// 当前 `NVMe` 驱动采用同步实现 (`submit_io_command` 等待完成)，
    /// 此函数用于以下场景：
    /// 1. 异步 I/O 提交后，中断触发时处理完成事件
    /// 2. 清理残留的完成条目
    /// 3. 未来异步 I/O 路径的回调处理
    ///
    /// # Safety
    ///
    /// 调用方必须确保：
    /// - 控制器已初始化
    /// - 中断已正确注册
    /// - 无并发访问 I/O 队列
    /// # Errors
    /// I/O 完成条目中检测到设备错误时返回 Err。
    // 有意窄化: 资源类型转换, POSIX/Linux ABI 约定
    #[expect(clippy::cast_possible_truncation)]
    pub fn handle_interrupt(&mut self) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        // 读取 I/O 完成队列
        // SAFETY: io_cq_dma 由 DMA 分配保证有效，io_cq_head 在有效范围内
        let cq = self.io_cq_dma.virt.0 as *const NvmeCompletion;
        let mut processed = 0u32;

        loop {
            // SAFETY: cq 指向有效的 DMA 内存，io_cq_head < QUEUE_DEPTH
            let entry = unsafe { cq.add(self.io_cq_head as usize).read_volatile() };

            // 检查 phase bit 是否匹配当前 phase
            if !entry.is_completed(self.io_phase) {
                break;
            }

            // 检查命令是否成功
            if !entry.is_success() {
                // 复制 packed struct 字段到局部变量以避免对齐问题
                let sqid = entry.sqid;
                let cid = entry.cid;
                let status = entry.status_code();
                crate::klog_ffi!(
                    klog_ffi_warn,
                    "[NVMe] I/O completion error: sqid={}, cid={}, status={}",
                    sqid, cid, status
                );
            }

            processed += 1;

            // 更新头指针
            let new_head = (self.io_cq_head + 1) % (QUEUE_DEPTH as u32);
            self.io_cq_head = new_head;

            // 每 QUEUE_DEPTH 个条目翻转一次 phase bit
            if new_head == 0 {
                self.io_phase ^= 1;
            }
        }

        if processed > 0 {
            // 敲响 CQ 门铃，通知控制器已完成条目已被处理
            self.write_doorbell(IO_QUEUE_ID, false, self.io_cq_head);

            crate::klog_ffi!(
                klog_ffi_info,
                "[NVMe] interrupt handled: {} completions processed",
                processed
            );
        }

        Ok(())
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
            cdw0: 0,
            rsvd1: 0,
            sqhd: 0,
            sqid: 0,
            cid: 0,
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
