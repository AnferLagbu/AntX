#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! HvFS (Hypervisor File System) — services 层安全代理 (Phase 2.2.4)
//!
//! 封装 `kernel/fs/hvfs/hvfs.rs` 顶层 API, 把 i32 错误码 → Result,
//! 把 `pwm: u64` 凭据 → `PwmCapability` 类型化, 提供 100% safe 的高级
//! 文件系统操作 (open/close/read/write/mkdir/unlink/stat/.../sync)。
//!
//! ## 内部 unsafe 范围
//!
//! 内部 ZFS 风格子系统 (ARC/DMU/ZIL/SPA/TXG/BP) 仍保留 unsafe 池,
//! 但全部封装在 `kernel/fs/hvfs/` 内部, services 层只调用
//! 已经验证过的顶层 `HvfsData` 方法。
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 顶层 API 已经过 `init()` 后的所有权 + 锁保护
//! - **类型安全**: 错误码 → `HvFsError`, 模式 → `FileMode`
//! - **薄包装**: 透传 open/close/read/write/mkdir/unlink/stat/sync/...
//!
//! 评估日期: 2026-06-04
//! Phase 2.2.4 任务: 磁盘文件系统迁移 (最复杂)

use crate::kernel::framework::fs::hvfs::hvfs::HvfsData;
use crate::kernel::framework::fs::vfs::types::KernelError;

// ============================================================================
// 错误码
// ============================================================================

/// HvFS 服务层错误 (强类型, 替代内核 `i32`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HvFsError {
    /// 路径无效
    NotFound,
    /// 已存在
    AlreadyExists,
    /// 不是目录
    NotADirectory,
    /// 是目录
    IsADirectory,
    /// 权限不足
    PermissionDenied,
    /// 无效参数
    InvalidArgument,
    /// 文件描述符无效
    BadFd,
    /// IO 失败
    Io,
    /// 设备满
    NoSpace,
    /// 文件系统未初始化
    NotInitialized,
    /// 文件系统已损坏
    Corrupted,
    /// 不支持的操作
    NotSupported,
    /// 其他
    Other(i32),
}

impl HvFsError {
    pub fn from_i32(rc: i32) -> Self {
        match rc {
            -2 => Self::NotFound,
            -17 => Self::AlreadyExists,
            -20 => Self::NotADirectory,
            -21 => Self::IsADirectory,
            -1 => Self::PermissionDenied,
            -22 => Self::InvalidArgument,
            -9 => Self::BadFd,
            -5 => Self::Io,
            -28 => Self::NoSpace,
            -6 => Self::NotInitialized,
            -84 => Self::Corrupted,
            -38 => Self::NotSupported,
            rc => Self::Other(rc),
        }
    }
}

// ============================================================================
// 打开模式
// ============================================================================

/// 文件打开模式 (强类型)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
    /// 只读
    ReadOnly = 0,
    /// 只写
    WriteOnly = 1,
    /// 读写
    ReadWrite = 2,
    /// 创建 (不存在时新建)
    Create = 64,
    /// 截断
    Truncate = 512,
    /// 追加
    Append = 1024,
}

impl FileMode {
    pub fn to_flags(self) -> u32 {
        self as u32
    }
}

// ============================================================================
// 凭据封装
// ============================================================================

/// 进程能力 (PWM) 凭据的零拷贝视图
///
/// 不持有数据, 只是引用 — 跟随 `&'a Ctx` 生命周期。
#[derive(Debug, Clone, Copy)]
pub struct PwmCapability<'a> {
    raw: u64,
    _ctx: core::marker::PhantomData<&'a ()>,
}

impl<'a> PwmCapability<'a> {
    /// 从 `pwm` 整数直接构造
    pub fn from_raw(pwm: u64) -> Self {
        Self {
            raw: pwm,
            _ctx: core::marker::PhantomData,
        }
    }

    /// 根用户凭据 (uid 0) — services 内部使用
    pub fn root() -> Self {
        Self::from_raw(0)
    }

    /// 原始 pwm 值
    pub fn raw(&self) -> u64 {
        self.raw
    }
}

// ============================================================================
// 句柄
// ============================================================================

/// HvFS 安全文件句柄
#[derive(Debug, Clone, Copy)]
pub struct HvFile {
    /// 内部 fd
    pub fd: u32,
}

// ============================================================================
// 安全 HvFS 代理
// ============================================================================

/// HvFS 安全代理 (services 层)。
pub struct SafeHvFs {
    inner: &'static HvfsData,
}

impl SafeHvFs {
    /// 创建全局 HvFS 代理
    pub fn new() -> Self {
        Self {
            inner: crate::kernel::framework::fs::hvfs::hvfs::get_hvfs(),
        }
    }

    /// 是否已初始化
    pub fn is_initialized(&self) -> bool {
        self.inner.is_initialized()
    }

    /// 是否处于磁盘模式
    pub fn is_disk_mode(&self) -> bool {
        self.inner.is_disk_mode()
    }

    /// 打开文件
    pub fn open<'a>(
        &self,
        path: &str,
        mode: FileMode,
        pwm: &PwmCapability<'a>,
    ) -> Result<HvFile, HvFsError> {
        match self.inner.open(path, mode.to_flags(), pwm.raw()) {
            Ok(fd) => Ok(HvFile { fd: fd as u32 }),
            Err(e) => Err(translate_kernel_error(e)),
        }
    }

    /// 关闭文件
    pub fn close(&self, file: HvFile) -> Result<(), HvFsError> {
        let rc = self.inner.close(file.fd);
        if rc == 0 {
            Ok(())
        } else {
            Err(HvFsError::from_i32(rc))
        }
    }

    /// 读取数据
    ///
    /// # 返回
    /// 实际读取的字节数
    pub fn read(&self, file: HvFile, buf: &mut [u8]) -> Result<usize, HvFsError> {
        let rc = self.inner.read(file.fd, buf, buf.len() as u32);
        if rc < 0 {
            Err(HvFsError::from_i32(rc))
        } else {
            Ok(rc as usize)
        }
    }

    /// 写入数据
    pub fn write(&self, file: HvFile, buf: &[u8]) -> Result<usize, HvFsError> {
        let rc = self.inner.write(file.fd, buf, buf.len() as u32);
        if rc < 0 {
            Err(HvFsError::from_i32(rc))
        } else {
            Ok(rc as usize)
        }
    }

    /// seek
    ///
    /// # 参数
    /// - `whence`: 0=Set, 1=Cur, 2=End
    pub fn seek(&self, file: HvFile, offset: i64, whence: u32) -> Result<u64, HvFsError> {
        let new_off = self.inner.seek(file.fd, offset, whence);
        if new_off < 0 {
            Err(HvFsError::from_i32(new_off as i32))
        } else {
            Ok(new_off as u64)
        }
    }

    /// 同步磁盘
    pub fn sync(&self) -> Result<(), HvFsError> {
        let rc = self.inner.sync();
        if rc == 0 {
            Ok(())
        } else {
            Err(HvFsError::from_i32(rc))
        }
    }

    /// 状态查询
    pub fn stats(&self) -> (u64, u64, u64, u64) {
        self.inner.get_stats()
    }

    // ── 目录/文件管理 (需要 PWM 凭据) ──

    /// 创建目录
    pub fn mkdir<'a>(&self, path: &str, pwm: &PwmCapability<'a>) -> Result<(), HvFsError> {
        let rc = self.inner.mkdir(path, pwm.raw());
        if rc == 0 {
            Ok(())
        } else {
            Err(HvFsError::from_i32(rc))
        }
    }

    /// 删除文件/目录
    pub fn unlink<'a>(&self, path: &str, pwm: &PwmCapability<'a>) -> Result<(), HvFsError> {
        let rc = self.inner.unlink(path, pwm.raw());
        if rc == 0 {
            Ok(())
        } else {
            Err(HvFsError::from_i32(rc))
        }
    }

    /// stat
    ///
    /// # 返回
    /// 成功: 元数据字节序列化长度
    pub fn stat<'a>(&self, path: &str, pwm: &PwmCapability<'a>) -> Result<u64, HvFsError> {
        match self.inner.stat(path, pwm.raw()) {
            Some(_obj) => Ok(0), // 简化为存在/不存在语义
            None => Err(HvFsError::NotFound),
        }
    }
}

impl Default for SafeHvFs {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 全局实例
// ============================================================================

use spin::Once;
static GLOBAL_HVFS: Once<SafeHvFs> = Once::new();

/// 初始化全局 HvFS
pub fn init_global() {
    GLOBAL_HVFS.call_once(SafeHvFs::new);
}

/// 获取全局 HvFS 引用
pub fn global() -> &'static SafeHvFs {
    GLOBAL_HVFS
        .get()
        .expect("hvfs::global() called before init_global()")
}

// ============================================================================
// 工具
// ============================================================================

fn translate_kernel_error(e: KernelError) -> HvFsError {
    match e {
        KernelError::NotFound => HvFsError::NotFound,
        KernelError::AlreadyExists => HvFsError::AlreadyExists,
        KernelError::NotADirectory => HvFsError::NotADirectory,
        KernelError::IsDirectory => HvFsError::IsADirectory,
        KernelError::PermissionDenied => HvFsError::PermissionDenied,
        KernelError::InvalidArgument => HvFsError::InvalidArgument,
        KernelError::IoError => HvFsError::Io,
        KernelError::NoSpace => HvFsError::NoSpace,
        _ => HvFsError::Other(0),
    }
}
