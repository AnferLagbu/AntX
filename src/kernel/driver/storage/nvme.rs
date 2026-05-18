//! NVMe 驱动 (NVMe Driver)
//!
//! 提供NVMe (Non-Volatile Memory Express) SSD支持：
//! - **PCIe接口**: 高速PCIe总线连接
//! - **多队列**: 支持多个I/O队列并行处理
//! - **高性能**: 直接内存访问，低延迟
//! - **命名空间**: 多个逻辑设备支持
//!
//! ## 硬件规格
//!
//! ```text
//! NVMe Controller:
//! ├── PCIe Configuration Space
//! ├── Controller Registers (BAR0)
//! │   ├── Controller Capabilities (CAP)
//! │   ├── Version (VS)
//! │   ├── Interrupt Mask Set (INTMS)
//! │   ├── Interrupt Mask Clear (INTMC)
//! │   ├── Controller Configuration (CC)
//! │   ├── Controller Status (CSTS)
//! │   └── Admin Queue Attributes (AQA)
//! └── Queue Pairs
//!     ├── Admin Submission Queue (ASQ)
//!     ├── Admin Completion Queue (ACQ)
//!     └── I/O Queue Pairs (IOSQ/IOCQ)
//! ```
//!
//! # Safety
//! NVMe驱动涉及PCIe配置、MMIO寄存器和DMA操作。

use super::framework::{Driver, DeviceType, DriverError, Result, DeviceInfo};
use core::ptr;
use alloc::vec::Vec;

// ============================================================================
// NVMe 常量定义
// ============================================================================

/// NVMe管理队列ID
const ADMIN_QUEUE_ID: u16 = 0;

/// NVMe I/O队列起始ID
const IO_QUEUE_START_ID: u16 = 1;

/// 最大队列深度
const MAX_QUEUE_DEPTH: usize = 65536;

/// 最大队列数量
const MAX_QUEUE_COUNT: usize = 65536;

/// 页大小 (4KB)
const PAGE_SIZE: usize = 4096;

// ============================================================================
// NVMe 寄存器定义
// ============================================================================

/// NVMe控制器寄存器
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeControllerRegisters {
    /// 控制器能力
    pub cap: u64,
    /// 版本
    pub vs: u32,
    /// 中断掩码设置
    pub intms: u32,
    /// 中断掩码清除
    pub intmc: u32,
    /// 控制器配置
    pub cc: u32,
    /// 保留
    pub rsvd1: u32,
    /// 控制器状态
    pub csts: u32,
    /// 中断状态
    pub nssr: u32,
    /// Admin队列属性
    pub aqa: u32,
    /// Admin提交队列基地址
    pub asq: u64,
    /// Admin完成队列基地址
    pub acq: u64,
}

/// 控制器能力寄存器字段
mod cap {
    pub const MQES: u64 = 0xFFFF;           // 最大队列条目数
    pub const CQR: u64 = 0xFF << 16;        // 队列轮询支持
    pub const AMS: u64 = 0x3 << 17;         // 仲裁机制支持
    pub const TO: u64 = 0xFF << 24;         // 超时
    pub const DSTRD: u64 = 0xF << 32;       // 门铃步长
    pub const NSSRS: u64 = 1 << 36;         // NVM子系统复位支持
    pub const CSS: u64 = 0xFF << 37;        // 命令集支持
    pub const BPS: u64 = 1 << 45;           // 启动分区支持
    pub const MPSMIN: u64 = 0xF << 48;      // 最小页大小
    pub const MPSMAX: u64 = 0xF << 52;      // 最大页大小
    pub const PMRS: u64 = 1 << 56;          // 持久内存区域支持
    pub const CMBS: u64 = 1 << 57;          // 控制器内存缓冲区支持
}

/// 控制器配置寄存器字段
mod cc {
    pub const EN: u32 = 1 << 0;             // 使能
    pub const CSS: u32 = 0x7 << 4;          // 命令集选择
    pub const MPS: u32 = 0xF << 7;          // 页大小
    pub const AMS: u32 = 0x7 << 11;         // 仲裁机制选择
    pub const SHN: u32 = 0x3 << 14;         // 关机通知
    pub const IOCQES: u32 = 0xF << 20;      // I/O完成队列条目大小
    pub const IOSQES: u32 = 0xF << 24;      // I/O提交队列条目大小
}

/// 控制器状态寄存器字段
mod csts {
    pub const RDY: u32 = 1 << 0;            // 就绪
    pub const CST: u32 = 1 << 1;            // 控制器关机类型
    pub const SHST: u32 = 0x3 << 2;         // 关机状态
    pub const NSSRO: u32 = 1 << 4;          // NVM子系统复位发生
    pub const PP: u32 = 1 << 5;             // 处理暂停
}

// ============================================================================
// NVMe 命令定义
// ============================================================================

/// NVMe Admin命令操作码
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
    AsynchronousEventRequest = 0x0C,
    NsManage = 0x0D,
    NsAttachment = 0x0E,
    KeepAlive = 0x0F,
}

/// NVMe NVM命令操作码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NvmeNvmOpcode {
    Flush = 0x00,
    Write = 0x01,
    Read = 0x02,
    WriteUncor = 0x04,
    Compare = 0x05,
    WriteZeroes = 0x08,
    DatasetManagement = 0x09,
}

/// NVMe命令标志
#[derive(Debug, Clone, Copy)]
pub struct NvmeCommandFlags {
    pub fused: bool,
    pub psdt: u8,
}

/// NVMe命令 (16字节)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeCommand {
    /// 操作码
    pub opcode: u8,
    /// 标志
    pub flags: u8,
    /// 命令标识符
    pub cid: u16,
    /// 命名空间ID
    pub nsid: u32,
    /// CDW2-3 (保留)
    pub cdw2: u32,
    pub cdw3: u32,
    /// 数据指针
    pub dptr: [u64; 2],
    /// CDW10-15
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

impl NvmeCommand {
    /// 创建读命令
    pub fn read(nsid: u32, slba: u64, nlb: u16, dptr: u64) -> Self {
        Self {
            opcode: NvmeNvmOpcode::Read as u8,
            flags: 0,
            cid: 0,
            nsid,
            cdw2: 0,
            cdw3: 0,
            dptr: [dptr, 0],
            cdw10: (slba & 0xFFFFFFFF) as u32,
            cdw11: ((slba >> 32) & 0xFFFFFFFF) as u32,
            cdw12: (nlb as u32) & 0xFFFF,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }
    
    /// 创建写命令
    pub fn write(nsid: u32, slba: u64, nlb: u16, dptr: u64) -> Self {
        Self {
            opcode: NvmeNvmOpcode::Write as u8,
            flags: 0,
            cid: 0,
            nsid,
            cdw2: 0,
            cdw3: 0,
            dptr: [dptr, 0],
            cdw10: (slba & 0xFFFFFFFF) as u32,
            cdw11: ((slba >> 32) & 0xFFFFFFFF) as u32,
            cdw12: (nlb as u32) & 0xFFFF,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }
    
    /// 创建Identify命令
    pub fn identify(nsid: u32, cns: u8, dptr: u64) -> Self {
        Self {
            opcode: NvmeAdminOpcode::Identify as u8,
            flags: 0,
            cid: 0,
            nsid,
            cdw2: 0,
            cdw3: 0,
            dptr: [dptr, 0],
            cdw10: cns as u32,
            cdw11: 0,
            cdw12: 0,
            cdw13: 0,
            cdw14: 0,
            cdw15: 0,
        }
    }
}

/// NVMe完成队列条目 (16字节)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeCompletion {
    /// 命令特定
    pub cdw0: u32,
    /// 保留
    pub rsvd1: u32,
    /// 提交队列头指针
    pub sqhd: u16,
    /// 提交队列标识符
    pub sqid: u16,
    /// 命令标识符
    pub cid: u16,
    /// 状态字段
    pub status: u16,
}

impl NvmeCompletion {
    /// 检查是否完成
    pub fn is_completed(&self) -> bool {
        self.status & 0x01 != 0
    }
    
    /// 获取状态码
    pub fn status_code(&self) -> u16 {
        (self.status >> 1) & 0x7FF
    }
    
    /// 检查是否成功
    pub fn is_success(&self) -> bool {
        self.status_code() == 0
    }
}

// ============================================================================
// NVMe Identify 数据结构
// ============================================================================

/// NVMe控制器Identify数据
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeIdentifyController {
    /// PCI厂商ID
    pub vid: u16,
    /// PCI子系统厂商ID
    pub ssvid: u16,
    /// 序列号
    pub sn: [u8; 20],
    /// 型号
    pub mn: [u8; 40],
    /// 固件版本
    pub fr: [u8; 8],
    /// 推荐仲裁突发
    pub rab: u8,
    /// IEEE OUI标识符
    pub ieee: [u8; 3],
    /// 控制器多路径和共享功能
    pub cmic: u8,
    /// 最大数据传输大小
    pub mdts: u8,
    /// 控制器ID
    pub cntlid: u16,
    /// 版本
    pub ver: u32,
    /// RTD3恢复延迟
    pub rtd3r: u32,
    /// RTD3入口延迟
    pub rtd3e: u32,
    /// 可选异步命令支持
    pub oaes: u32,
    /// 控制器属性
    pub ctratt: u32,
    /// RRLs
    pub rrls: u16,
    /// 保留
    pub rsvd1: [u8; 9],
    /// 控制器类型
    pub cntrltype: u8,
    /// FGUID
    pub fguid: [u8; 16],
    /// CRDT
    pub crdt: [u16; 3],
    /// 保留
    pub rsvd2: [u8; 106],
    /// 支持的Admin命令
    pub sacs: u32,
    /// 支持的NVM命令
    pub oncs: u32,
    /// 支持的电源管理功能
    pub fuses: u32,
    /// 格式化NVM属性
    pub fna: u8,
    /// 卷属性
    pub vwc: u8,
    /// 原子写入单元正常
    pub awun: u16,
    /// 原子写入单元电源故障
    pub awupf: u16,
    /// NVM供应商格式
    pub nvscc: u8,
    /// 命名空间写保护
    pub nwpc: u8,
    /// 保留
    pub rsvd3: [u8; 2],
    /// 提交队列条目大小
    pub sqes: u8,
    /// 完成队列条目大小
    pub cqes: u8,
    /// 最大命名空间数
    pub maxcmd: u16,
    /// 命名空间数量
    pub nn: u32,
    /// 控制器电源范围
    pub oncs_opt: u16,
    /// 控制器电源范围
    pub oncs_nvm: u16,
    /// 保留
    pub rsvd4: [u8; 16],
    /// 子系统命名空间数量
    pub nsnum: u32,
    /// 子系统命名空间使用
    pub nsnum_opt: u32,
    /// 保留
    pub rsvd5: [u8; 224],
}

/// NVMe命名空间Identify数据
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct NvmeIdentifyNamespace {
    /// 命名空间大小
    pub nsze: u64,
    /// 命名空间容量
    pub ncap: u64,
    /// 命名空间使用
    pub nuse: u64,
    /// 命名空间属性
    pub nsfeat: u8,
    /// 命名空间数量
    pub nlbaf: u8,
    /// 格式化逻辑块大小
    pub flbas: u8,
    /// 元数据能力
    pub mc: u8,
    /// 端到端数据保护类型
    pub dpc: u8,
    /// 端到端数据保护动作
    pub dps: u8,
    /// 命名空间多路径和共享功能
    pub nmic: u8,
    /// 命名空间重新分配能力
    pub rescap: u8,
    /// 格式化进度
    pub fpi: u8,
    /// 分配对齐
    pub dalb: u8,
    /// 命名空间原子写入单元正常
    pub nawun: u16,
    /// 命名空间原子写入单元电源故障
    pub nawupf: u16,
    /// 命名空间原子比较和写入单元
    pub nacwu: u16,
    /// 命名空间原子边界大小正常
    pub nabsn: u16,
    /// 保留
    pub rsvd1: [u8; 12],
    /// NVM集标识符
    pub nvmsetid: u16,
    /// 端点组标识符
    pub endgid: u16,
    /// 保留
    pub rsvd2: [u8; 10],
    /// LBA格式
    pub lbaf: [LbaFormat; 16],
}

/// LBA格式描述符
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct LbaFormat {
    /// 元数据大小
    pub ms: u16,
    /// 数据大小
    pub lbads: u8,
    /// 相对性能
    pub rp: u8,
}

// ============================================================================
// NVMe 队列管理
// ============================================================================

/// NVMe队列对
pub struct NvmeQueuePair {
    /// 队列ID
    pub qid: u16,
    /// 提交队列
    pub sq: *mut NvmeCommand,
    /// 完成队列
    pub cq: *mut NvmeCompletion,
    /// 队列深度
    pub depth: u32,
    /// 提交队列尾指针
    pub sq_tail: u32,
    /// 完成队列头指针
    pub cq_head: u32,
    /// 门铃寄存器偏移
    pub db_stride: u32,
    /// 是否已创建
    pub created: bool,
}

impl NvmeQueuePair {
    pub fn new(qid: u16, depth: u32, db_stride: u32) -> Self {
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
    
    /// 提交命令
    pub fn submit(&mut self, cmd: NvmeCommand) -> u16 {
        unsafe {
            let cid = self.sq_tail as u16;
            let entry = self.sq.add(self.sq_tail as usize);
            (*entry) = cmd;
            (*entry).cid = cid;
            
            self.sq_tail = (self.sq_tail + 1) % self.depth;
            
            cid
        }
    }
    
    /// 处理完成
    pub fn process_completion(&mut self) -> Option<NvmeCompletion> {
        unsafe {
            let entry = &*self.cq.add(self.cq_head as usize);
            
            if entry.is_completed() {
                let completion = *entry;
                
                // 更新头指针
                self.cq_head = (self.cq_head + 1) % self.depth;
                
                Some(completion)
            } else {
                None
            }
        }
    }
}

// ============================================================================
// NVMe 控制器
// ============================================================================

/// NVMe控制器驱动
pub struct NvmeController {
    /// PCIe MMIO基地址
    mmio_base: usize,
    /// 控制器寄存器指针
    regs: *mut NvmeControllerRegisters,
    /// Admin队列对
    admin_queue: NvmeQueuePair,
    /// I/O队列对列表
    io_queues: Vec<NvmeQueuePair>,
    /// 控制器Identify数据
    identify_ctrl: Option<NvmeIdentifyController>,
    /// 命名空间数量
    namespace_count: u32,
    /// 设备信息
    info: DeviceInfo,
    /// 是否已初始化
    initialized: bool,
}

impl NvmeController {
    /// 创建新的NVMe控制器实例
    pub fn new(mmio_base: usize) -> Self {
        Self {
            mmio_base,
            regs: ptr::null_mut(),
            admin_queue: NvmeQueuePair::new(ADMIN_QUEUE_ID, 64, 4),
            io_queues: Vec::new(),
            identify_ctrl: None,
            namespace_count: 0,
            info: DeviceInfo::new("nvme", DeviceType::Block),
            initialized: false,
        }
    }
    
    /// 初始化控制器
    fn init_controller(&mut self) -> Result<()> {
        unsafe {
            self.regs = self.mmio_base as *mut NvmeControllerRegisters;
            let regs = &mut *self.regs;
            
            // 检查控制器是否就绪
            if regs.csts & csts::RDY != 0 {
                // 控制器已启用，需要先禁用
                regs.cc &= !cc::EN;
                
                // 等待控制器禁用
                let mut timeout = 1_000_000;
                while timeout > 0 {
                    if regs.csts & csts::RDY == 0 {
                        break;
                    }
                    timeout -= 1;
                    core::hint::spin_loop();
                }
            }
            
            // 配置Admin队列
            let aqa = ((64 - 1) << 16) | (64 - 1);  // ASQS | ACQS
            regs.aqa = aqa;
            
            // 设置Admin队列基地址 (需要分配DMA内存)
            // TODO: 分配DMA内存并设置ASQ/ACQ
            
            // 启用控制器
            regs.cc |= cc::EN;
            
            // 等待控制器就绪
            let mut timeout = 1_000_000;
            while timeout > 0 {
                if regs.csts & csts::RDY != 0 {
                    break;
                }
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
    fn identify_controller(&mut self) -> Result<()> {
        // TODO: 发送Identify命令获取控制器信息
        
        // 模拟数据
        let mut ctrl = NvmeIdentifyController {
            vid: 0x8086,
            ssvid: 0x8086,
            sn: [0; 20],
            mn: [0; 40],
            fr: [0; 8],
            rab: 0,
            ieee: [0; 3],
            cmic: 0,
            mdts: 5,  // 32KB最大传输
            cntlid: 0,
            ver: 0x00010400,  // NVMe 1.4
            rtd3r: 0,
            rtd3e: 0,
            oaes: 0,
            ctratt: 0,
            rrls: 0,
            rsvd1: [0; 9],
            cntrltype: 0,
            fguid: [0; 16],
            crdt: [0; 3],
            rsvd2: [0; 106],
            sacs: 0,
            oncs: 0,
            fuses: 0,
            fna: 0,
            vwc: 1,  // 易失性写缓存启用
            awun: 0,
            awupf: 0,
            nvscc: 0,
            nwpc: 0,
            rsvd3: [0; 2],
            sqes: 0x66,  // 最小6，最大6 (64字节)
            cqes: 0x44,  // 最小4，最大4 (16字节)
            maxcmd: 0,
            nn: 1,  // 1个命名空间
            oncs_opt: 0,
            oncs_nvm: 0,
            rsvd4: [0; 16],
            nsnum: 1,
            nsnum_opt: 1,
            rsvd5: [0; 224],
        };
        
        // 设置序列号和型号
        let sn = b"NVME001";
        ctrl.sn[..sn.len()].copy_from_slice(sn);
        
        let mn = b"AntX NVMe Controller";
        ctrl.mn[..mn.len()].copy_from_slice(mn);
        
        self.identify_ctrl = Some(ctrl);
        self.namespace_count = ctrl.nn;
        
        Ok(())
    }
    
    /// 创建I/O队列对
    pub fn create_io_queue(&mut self, depth: u32) -> Result<u16> {
        let qid = (self.io_queues.len() as u16) + IO_QUEUE_START_ID;
        
        let queue = NvmeQueuePair::new(qid, depth, 4);
        self.io_queues.push(queue);
        
        // TODO: 发送Create SQ/CQ命令
        
        Ok(qid)
    }
    
    /// 读取数据
    pub fn read(&mut self, nsid: u32, lba: u64, count: u16, buffer: *mut u8) -> Result<()> {
        // TODO: 提交读命令到I/O队列
        
        Ok(())
    }
    
    /// 写入数据
    pub fn write(&mut self, nsid: u32, lba: u64, count: u16, buffer: *const u8) -> Result<()> {
        // TODO: 提交写命令到I/O队列
        
        Ok(())
    }
    
    /// TRIM命令 (数据集管理)
    pub fn trim(&mut self, nsid: u32, lba: u64, count: u16) -> Result<()> {
        // TODO: 提交Dataset Management命令
        
        Ok(())
    }
    
    /// 获取命名空间数量
    pub fn namespace_count(&self) -> u32 {
        self.namespace_count
    }
}

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
        self.init_controller()?;
        self.identify_controller()?;
        self.initialized = true;
        Ok(())
    }
    
    fn shutdown(&mut self) -> Result<()> {
        unsafe {
            let regs = &mut *self.regs;
            regs.cc &= !cc::EN;
        }
        
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
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_nvme_command_creation() {
        let cmd = NvmeCommand::read(1, 0, 1, 0x1000);
        assert_eq!(cmd.opcode, NvmeNvmOpcode::Read as u8);
        assert_eq!(cmd.nsid, 1);
    }
    
    #[test]
    fn test_nvme_completion_status() {
        let completion = NvmeCompletion {
            cdw0: 0,
            rsvd1: 0,
            sqhd: 0,
            sqid: 0,
            cid: 0,
            status: 0x0001,  // 成功
        };
        
        assert!(completion.is_completed());
        assert!(completion.is_success());
        assert_eq!(completion.status_code(), 0);
    }
    
    #[test]
    fn test_nvme_controller_creation() {
        let ctrl = NvmeController::new(0xFE000000);
        assert_eq!(ctrl.name(), "NVMe Controller");
        assert_eq!(ctrl.device_type(), DeviceType::Block);
        assert!(!ctrl.is_ready());
    }
    
    #[test]
    fn test_queue_pair_creation() {
        let queue = NvmeQueuePair::new(1, 64, 4);
        assert_eq!(queue.qid, 1);
        assert_eq!(queue.depth, 64);
        assert_eq!(queue.sq_tail, 0);
        assert_eq!(queue.cq_head, 0);
    }
}
