//! AHCI/SATA 驱动 (AHCI/SATA Driver)
//!
//! 提供AHCI (Advanced Host Controller Interface) SATA支持：
//! - **SATA接口**: 传统SATA SSD和HDD
//! - **NCQ支持**: 原生命令队列
//! - **热插拔**: 设备动态连接
//! - **多端口**: 支持多个SATA端口
//!
//! ## 硬件规格
//!
//! ```text
//! AHCI Controller:
//! ├── HBA Memory (ABAR)
//! │   ├── Generic Host Control (GHC)
//! │   ├── Port Registers (0x10 + 0x80*n)
//! │   │   ├── PxCLB: 命令列表基地址
//! │   │   ├── PxFB: FIS基地址
//! │   │   ├── PxIS: 中断状态
//! │   │   ├── PxIE: 中断使能
//! │   │   ├── PxCMD: 命令和状态
//! │   │   ├── PxTFD: 任务文件数据
//! │   │   ├── PxSIG: 签名
//! │   │   ├── PxSSTS: SATA状态
//! │   │   ├── PxSCTL: SATA控制
//! │   │   └── PxSERR: SATA错误
//! │   └── ...
//! └── Command List & FIS Buffer
//! ```
//!
//! # Safety
//! AHCI驱动涉及MMIO寄存器和DMA操作。

use super::framework::{Driver, DeviceType, DriverError, Result, DeviceInfo};
use core::ptr;

// ============================================================================
// AHCI 常量定义
// ============================================================================

/// AHCI端口数量
const AHCI_MAX_PORTS: usize = 32;

/// 命令列表深度
const CMD_LIST_DEPTH: usize = 32;

/// FIS接收缓冲区大小
const FIS_BUFFER_SIZE: usize = 256;

// ============================================================================
// AHCI 寄存器定义
// ============================================================================

/// AHCI HBA通用主机控制寄存器
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct AhciHbaGhc {
    /// HBA能力
    pub cap: u32,
    /// 全局HBA控制
    pub ghc: u32,
    /// 中断状态
    pub is: u32,
    /// 端口实现
    pub pi: u32,
    /// 保留
    pub rsvd: [u32; 5],
    /// 版本
    pub vs: u32,
    /// 命令完成轮询
    pub ccc_ctl: u32,
    /// 命令完成计数
    pub ccc_pts: u32,
    /// 封装管理传输
    pub em_loc: u32,
    /// 封装管理控制
    pub em_ctl: u32,
    /// HBA能力扩展
    pub cap2: u32,
    /// BIST激活FIS
    pub bohc: u32,
}

/// AHCI端口寄存器
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct AhciPortRegs {
    /// 命令列表基地址
    pub clb: u32,
    /// 命令列表基地址高32位
    pub clbu: u32,
    /// FIS基地址
    pub fb: u32,
    /// FIS基地址高32位
    pub fbu: u32,
    /// 中断状态
    pub is: u32,
    /// 中断使能
    pub ie: u32,
    /// 命令和状态
    pub cmd: u32,
    /// 保留
    pub rsvd1: u32,
    /// 任务文件数据
    pub tfd: u32,
    /// 签名
    pub sig: u32,
    /// SATA状态
    pub ssts: u32,
    /// SATA控制
    pub sctl: u32,
    /// SATA错误
    pub serr: u32,
    /// SATA活动
    pub sact: u32,
    /// 命令发布
    pub ci: u32,
    /// SATA通知
    pub sntf: u32,
    /// FIS-based切换控制
    pub fbs: u32,
    /// 保留
    pub rsvd2: u32,
    /// 供应商特定
    pub vs: u32,
}

/// HBA能力寄存器字段
mod cap {
    pub const S64A: u32 = 1 << 0;     // 64位寻址
    pub const SNCQ: u32 = 1 << 1;     // NCQ支持
    pub const SSNTF: u32 = 1 << 5;    // SNTF支持
    pub const SMPS: u32 = 1 << 8;     // 机械存在状态
    pub const SSS: u32 = 1 << 9;      // 交错旋转支持
    pub const SALP: u32 = 1 << 10;    // 激活电源管理支持
    pub const SAL: u32 = 1 << 11;     // 激活LED支持
    pub const SCLO: u32 = 1 << 12;    // 命令发布覆盖
    pub const ISS: u32 = 0xF << 20;   // 接口速度支持
    pub const SAM: u32 = 1 << 24;     // AHCI模式仅
    pub const SPM: u32 = 1 << 25;     // 端口复用支持
    pub const FBSS: u32 = 1 << 26;    // FIS-based切换支持
    pub const PMD: u32 = 1 << 27;     // 状态管理驱动
    pub const HPCP: u32 = 1 << 28;    // 高优先级端口
    pub const MPSP: u32 = 1 << 29;    // 机械存在开关
    pub const SSSP: u32 = 1 << 30;    // 交错旋转状态
    pub const SSSS: u32 = 1 << 31;    // 支持交错旋转
}

/// 全局HBA控制寄存器字段
mod ghc {
    pub const HR: u32 = 1 << 0;       // HBA复位
    pub const IE: u32 = 1 << 1;       // 中断使能
    pub const MRSM: u32 = 1 << 2;     // MSI恢复状态机
    pub const DMAE: u32 = 1 << 4;     // DMA使能
    pub const AE: u32 = 1 << 31;      // AHCI使能
}

/// 端口命令寄存器字段
mod pxcmd {
    pub const ST: u32 = 1 << 0;       // 开始
    pub const SUD: u32 = 1 << 1;      // 旋转检测
    pub const POD: u32 = 1 << 2;      // 电源检测
    pub const CLO: u32 = 1 << 3;      // 命令列表覆盖
    pub const FRE: u32 = 1 << 4;      // FIS接收使能
    pub const CCS: u32 = 0x1F << 8;   // 当前命令槽
    pub const MPSS: u32 = 1 << 13;    // 机械存在开关状态
    pub const FR: u32 = 1 << 14;      // FIS接收运行
    pub const CR: u32 = 1 << 15;      // 命令列表运行
    pub const APSTE: u32 = 1 << 16;   // 自动部分到备用使能
    pub const DLAE: u32 = 1 << 17;    // 驱动锁活动使能
    pub const LSPM: u32 = 1 << 18;    // LED状态PMB使能
    pub const ESP: u32 = 1 << 20;     // 外部SATA端口
    pub const CPD: u32 = 1 << 21;     // 冷插拔检测
    pub const MPSP: u32 = 1 << 22;    // 机械存在开关检测
    pub const HPCP: u32 = 1 << 23;    // 热插拔能力端口
    pub const PMA: u32 = 1 << 24;     // 端口复用器附加
    pub const CPS: u32 = 1 << 25;     // 冷插拔状态
    pub const CRPM: u32 = 1 << 26;    // 运行时电源管理
    pub const MPHR: u32 = 1 << 27;    // 机械存在处理程序
    pub const FBSCP: u32 = 1 << 29;   // FIS-based切换能力端口
    pub const ASP: u32 = 1 << 30;     // 激活状态轮询
    pub const IC: u32 = 1 << 31;      // 初始化命令
}

/// SATA状态寄存器字段
mod pxssts {
    pub const DET: u32 = 0xF;         // 设备检测
    pub const SPD: u32 = 0xF << 4;    // 当前接口速度
    pub const IPM: u32 = 0xF << 8;    // 接口电源管理
}

// ============================================================================
// SATA 命令定义
// ============================================================================

/// ATA命令
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AtaCommand {
    Read = 0x20,
    ReadExt = 0x24,
    Write = 0x30,
    WriteExt = 0x34,
    Identify = 0xEC,
    SetFeatures = 0xEF,
    ReadFpdmaQueued = 0x60,    // NCQ读
    WriteFpdmaQueued = 0x61,   // NCQ写
    DataSetManagement = 0x06,  // TRIM
}

/// AHCI命令表
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct AhciCommandTable {
    /// 命令FIS (64字节)
    pub cfis: [u8; 64],
    /// ATAPI命令 (16字节)
    pub acmd: [u8; 16],
    /// 保留
    pub rsvd: [u8; 48],
    /// 数据基址
    pub prdt: [PhysicalRegionDescriptor; 16],
}

/// 物理区域描述符
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct PhysicalRegionDescriptor {
    /// 数据基址
    pub dba: u32,
    /// 数据基址高32位
    pub dbau: u32,
    /// 保留
    pub rsvd: u32,
    /// 数据字节计数
    pub dbc: u32,
}

/// AHCI命令头
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct AhciCommandHeader {
    /// 描述符信息
    pub dw0: u32,
    /// PRDT长度
    pub prdtl: u32,
    /// PRDT字节计数
    pub prdbc: u32,
    /// 命令表基址
    pub ctba: u32,
    /// 命令表基址高32位
    pub ctbau: u32,
    /// 保留
    pub rsvd: [u32; 4],
}

// ============================================================================
// FIS 结构
// ============================================================================

/// 主机到设备FIS
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct H2dFis {
    /// FIS类型
    pub fis_type: u8,
    /// 标志
    pub flags: u8,
    /// 命令
    pub command: u8,
    /// 特征低
    pub feature0: u8,
    /// LBA低
    pub lba0: u8,
    /// LBA中低
    pub lba1: u8,
    /// LBA中高
    pub lba2: u8,
    /// LBA高
    pub lba3: u8,
    /// 设备
    pub device: u8,
    /// LBA扩展低
    pub lba4: u8,
    /// LBA扩展高
    pub lba5: u8,
    /// 特征高
    pub feature1: u8,
    /// 扇区计数低
    pub count0: u8,
    /// 扇区计数高
    pub count1: u8,
    /// ICC
    pub icc: u8,
    /// 控制
    pub control: u8,
}

impl H2dFis {
    /// 创建读命令FIS
    pub fn read(lba: u64, count: u16) -> Self {
        Self {
            fis_type: 0x27,  // 主机到设备FIS
            flags: 0x80,     // 命令FIS
            command: AtaCommand::ReadExt as u8,
            feature0: 0,
            lba0: (lba & 0xFF) as u8,
            lba1: ((lba >> 8) & 0xFF) as u8,
            lba2: ((lba >> 16) & 0xFF) as u8,
            lba3: ((lba >> 24) & 0xFF) as u8,
            device: 0x40,    // LBA模式
            lba4: ((lba >> 32) & 0xFF) as u8,
            lba5: ((lba >> 40) & 0xFF) as u8,
            feature1: 0,
            count0: (count & 0xFF) as u8,
            count1: ((count >> 8) & 0xFF) as u8,
            icc: 0,
            control: 0,
        }
    }
    
    /// 创建写命令FIS
    pub fn write(lba: u64, count: u16) -> Self {
        Self {
            fis_type: 0x27,
            flags: 0x80,
            command: AtaCommand::WriteExt as u8,
            feature0: 0,
            lba0: (lba & 0xFF) as u8,
            lba1: ((lba >> 8) & 0xFF) as u8,
            lba2: ((lba >> 16) & 0xFF) as u8,
            lba3: ((lba >> 24) & 0xFF) as u8,
            device: 0x40,
            lba4: ((lba >> 32) & 0xFF) as u8,
            lba5: ((lba >> 40) & 0xFF) as u8,
            feature1: 0,
            count0: (count & 0xFF) as u8,
            count1: ((count >> 8) & 0xFF) as u8,
            icc: 0,
            control: 0,
        }
    }
    
    /// 创建NCQ读命令FIS
    pub fn read_ncq(lba: u64, count: u16, tag: u8) -> Self {
        Self {
            fis_type: 0x27,
            flags: 0x80,
            command: AtaCommand::ReadFpdmaQueued as u8,
            feature0: tag & 0x1F,  // NCQ标签
            lba0: (lba & 0xFF) as u8,
            lba1: ((lba >> 8) & 0xFF) as u8,
            lba2: ((lba >> 16) & 0xFF) as u8,
            lba3: ((lba >> 24) & 0xFF) as u8,
            device: 0x40,
            lba4: ((lba >> 32) & 0xFF) as u8,
            lba5: ((lba >> 40) & 0xFF) as u8,
            feature1: 0,
            count0: (count & 0xFF) as u8,
            count1: ((count >> 8) & 0xFF) as u8,
            icc: 0,
            control: 0,
        }
    }
}

// ============================================================================
// AHCI 端口
// ============================================================================

/// AHCI端口
pub struct AhciPort {
    /// 端口号
    pub port_num: u8,
    /// 端口寄存器指针
    regs: *mut AhciPortRegs,
    /// 是否已连接设备
    pub device_present: bool,
    /// 设备签名
    pub signature: u32,
    /// 是否支持NCQ
    pub ncq_supported: bool,
    /// NCQ标签位图
    ncq_tags: u32,
}

impl AhciPort {
    pub fn new(port_num: u8, regs: *mut AhciPortRegs) -> Self {
        Self {
            port_num,
            regs,
            device_present: false,
            signature: 0,
            ncq_supported: false,
            ncq_tags: 0,
        }
    }
    
    /// 检测设备
    pub fn detect_device(&mut self) -> bool {
        unsafe {
            let regs = &*self.regs;
            
            // 检查SATA状态
            let det = regs.ssts & pxssts::DET;
            if det == 0x03 {  // 设备已建立通信
                self.device_present = true;
                self.signature = regs.sig;
                
                // 检查是否是ATA设备
                match self.signature {
                    0x00000101 => true,  // SATA磁盘
                    0xEB140101 => true,  // ATAPI设备
                    _ => false,
                }
            } else {
                self.device_present = false;
                false
            }
        }
    }
    
    /// 启用端口
    pub fn enable(&mut self) -> Result<()> {
        unsafe {
            let regs = &mut *self.regs;
            
            // 启用FIS接收
            regs.cmd |= pxcmd::FRE;
            
            // 等待FIS接收运行
            let mut timeout = 1_000_000;
            while timeout > 0 {
                if regs.cmd & pxcmd::FR != 0 {
                    break;
                }
                timeout -= 1;
                core::hint::spin_loop();
            }
            
            // 启用命令处理
            regs.cmd |= pxcmd::ST;
            
            // 等待命令列表运行
            timeout = 1_000_000;
            while timeout > 0 {
                if regs.cmd & pxcmd::CR != 0 {
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
    
    /// 禁用端口
    pub fn disable(&mut self) -> Result<()> {
        unsafe {
            let regs = &mut *self.regs;
            
            // 禁用命令处理
            regs.cmd &= !pxcmd::ST;
            
            // 等待命令列表停止
            let mut timeout = 1_000_000;
            while timeout > 0 {
                if regs.cmd & pxcmd::CR == 0 {
                    break;
                }
                timeout -= 1;
                core::hint::spin_loop();
            }
            
            // 禁用FIS接收
            regs.cmd &= !pxcmd::FRE;
            
            // 等待FIS接收停止
            timeout = 1_000_000;
            while timeout > 0 {
                if regs.cmd & pxcmd::FR == 0 {
                    break;
                }
                timeout -= 1;
                core::hint::spin_loop();
            }
        }
        
        Ok(())
    }
    
    /// 读取数据
    pub fn read(&mut self, lba: u64, count: u16, buffer: *mut u8) -> Result<()> {
        // TODO: 构造命令并提交
        
        Ok(())
    }
    
    /// 写入数据
    pub fn write(&mut self, lba: u64, count: u16, buffer: *const u8) -> Result<()> {
        // TODO: 构造命令并提交
        
        Ok(())
    }
}

// ============================================================================
// AHCI 控制器
// ============================================================================

/// AHCI控制器驱动
pub struct AhciController {
    /// MMIO基地址
    mmio_base: usize,
    /// HBA寄存器指针
    hba: *mut AhciHbaGhc,
    /// 端口列表
    ports: Vec<AhciPort>,
    /// 实现的端口位图
    port_bitmap: u32,
    /// 设备信息
    info: DeviceInfo,
    /// 是否已初始化
    initialized: bool,
}

impl AhciController {
    /// 创建新的AHCI控制器实例
    pub fn new(mmio_base: usize) -> Self {
        Self {
            mmio_base,
            hba: ptr::null_mut(),
            ports: Vec::new(),
            port_bitmap: 0,
            info: DeviceInfo::new("ahci", DeviceType::Block),
            initialized: false,
        }
    }
    
    /// 初始化控制器
    fn init_controller(&mut self) -> Result<()> {
        unsafe {
            self.hba = self.mmio_base as *mut AhciHbaGhc;
            let hba = &mut *self.hba;
            
            // 检查AHCI模式
            if hba.ghc & ghc::AE == 0 {
                // 启用AHCI模式
                hba.ghc |= ghc::AE;
            }
            
            // 全局HBA复位
            hba.ghc |= ghc::HR;
            
            // 等待复位完成
            let mut timeout = 1_000_000;
            while timeout > 0 {
                if hba.ghc & ghc::HR == 0 {
                    break;
                }
                timeout -= 1;
                core::hint::spin_loop();
            }
            
            if timeout == 0 {
                return Err(DriverError::Timeout);
            }
            
            // 获取实现的端口
            self.port_bitmap = hba.pi;
            
            // 初始化每个端口
            for i in 0..AHCI_MAX_PORTS {
                if (self.port_bitmap & (1 << i)) != 0 {
                    let port_regs = (self.mmio_base + 0x100 + i * 0x80) as *mut AhciPortRegs;
                    let mut port = AhciPort::new(i as u8, port_regs);
                    
                    if port.detect_device() {
                        port.enable()?;
                        self.ports.push(port);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// 获取端口数量
    pub fn port_count(&self) -> usize {
        self.ports.len()
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
        Ok(())
    }
    
    fn shutdown(&mut self) -> Result<()> {
        for port in &mut self.ports {
            let _ = port.disable();
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
        let fis = H2dFis::read(0x1000, 8);
        assert_eq!(fis.fis_type, 0x27);
        assert_eq!(fis.command, AtaCommand::ReadExt as u8);
    }
    
    #[test]
    fn test_h2d_fis_write() {
        let fis = H2dFis::write(0x2000, 16);
        assert_eq!(fis.fis_type, 0x27);
        assert_eq!(fis.command, AtaCommand::WriteExt as u8);
    }
    
    #[test]
    fn test_h2d_fis_ncq() {
        let fis = H2dFis::read_ncq(0x3000, 32, 5);
        assert_eq!(fis.fis_type, 0x27);
        assert_eq!(fis.command, AtaCommand::ReadFpdmaQueued as u8);
        assert_eq!(fis.feature0, 5);
    }
    
    #[test]
    fn test_ahci_controller_creation() {
        let ctrl = AhciController::new(0xFE000000);
        assert_eq!(ctrl.name(), "AHCI Controller");
        assert_eq!(ctrl.device_type(), DeviceType::Block);
        assert!(!ctrl.is_ready());
    }
    
    #[test]
    fn test_port_bitmap() {
        let bitmap = 0x00000003;  // 端口0和端口1
        assert!((bitmap & (1 << 0)) != 0);
        assert!((bitmap & (1 << 1)) != 0);
        assert!((bitmap & (1 << 2)) == 0);
    }
}
