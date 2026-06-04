//! @SAFE: 本文件不含 unsafe 代码。
//!
//! 设备文件系统 (DevFS) — services 层安全代理 (Phase 2.2.2)
//!
//! 在 `kernel/fs/devfs` 基础上提供 100% safe 的公共 API,
//! 封装虚拟设备 (null/zero/console/tty/credo) 的注册与 IO。
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 内部 `DevfsData` 已由 P2.2.3 标记为安全 (Send+Sync)
//! - **类型安全**: 设备类型用 `DevKind` 枚举, 而非裸 `u8`
//! - **薄包装**: 透传 register/unregister/open/read/write/readdir
//! - **可替代**: 原 `kernel/fs/devfs/devfs.rs` 仍存在, 本文件是迁移目标
//!
//! 评估日期: 2026-06-04
//! Phase 2.2.2 任务: 设备文件系统迁移

use crate::kernel::framework::fs::devfs::devfs::{DevfsData, DEVFS_MAX_NAME};

// ============================================================================
// 设备类型
// ============================================================================

/// 设备类型 (强类型枚举, 替代裸 `u8`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DevKind {
    /// 空设备 /dev/null
    Null = 1,
    /// 零字节源 /dev/zero
    Zero = 2,
    /// 控制台 /dev/console
    Console = 3,
    /// TTY /dev/tty
    Tty = 4,
    /// Credo 能力 (PWM) 接口 /dev/credo
    Credo = 5,
}

impl DevKind {
    /// 从 `u8` 解析 (容忍未知值, 返回 `None`)
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Null),
            2 => Some(Self::Zero),
            3 => Some(Self::Console),
            4 => Some(Self::Tty),
            5 => Some(Self::Credo),
            _ => None,
        }
    }
}

// ============================================================================
// 安全设备文件描述符
// ============================================================================

/// DevFS 文件描述符
#[derive(Debug, Clone, Copy)]
pub struct DevFile {
    /// 内部节点索引
    pub index: u32,
    /// 设备类型
    pub kind: DevKind,
}

impl DevFile {
    /// 设备名 (如 "null", "zero", "console")
    pub fn name(&self) -> &'static str {
        match self.kind {
            DevKind::Null => "null",
            DevKind::Zero => "zero",
            DevKind::Console => "console",
            DevKind::Tty => "tty",
            DevKind::Credo => "credo",
        }
    }
}

// ============================================================================
// 安全 DevFS 代理
// ============================================================================

/// DevFS 安全代理 (services 层)。
pub struct SafeDevFs {
    inner: &'static DevfsData,
}

impl SafeDevFs {
    /// 创建全局 DevFS 代理
    pub fn new() -> Self {
        Self {
            inner: &crate::kernel::framework::fs::devfs::devfs::DEVFS_DATA,
        }
    }

    /// 注册设备
    ///
    /// # 返回
    /// 成功: Ok(()) ; 失败: `FsError` (设备已存在/表满)
    pub fn register(&self, name: &str, kind: DevKind) -> Result<(), &'static str> {
        let rc = self.inner.register_device(name, kind as u8);
        match rc {
            0 => Ok(()),
            -17 => Err("device already exists"),
            -28 => Err("devfs table full"),
            _ => Err("register failed"),
        }
    }

    /// 注销设备
    pub fn unregister(&self, name: &str) -> Result<(), &'static str> {
        let rc = self.inner.unregister_device(name);
        if rc == 0 {
            Ok(())
        } else {
            Err("device not found")
        }
    }

    /// 打开设备
    pub fn open(&self, path: &str) -> Result<DevFile, &'static str> {
        match self.inner.open(path) {
            Some((index, dev_type)) => {
                let kind = DevKind::from_u8(dev_type)
                    .ok_or("unknown device type")?;
                Ok(DevFile { index, kind })
            }
            None => Err("device not found"),
        }
    }

    /// 从设备读
    pub fn read(&self, dev: &DevFile, buf: &mut [u8]) -> Result<usize, &'static str> {
        let rc = self.inner.read(dev.kind as u8, buf);
        if rc < 0 {
            Err("read failed")
        } else {
            Ok(rc as usize)
        }
    }

    /// 向设备写
    pub fn write(&self, dev: &DevFile, buf: &[u8]) -> Result<usize, &'static str> {
        let rc = self.inner.write(dev.kind as u8, buf);
        if rc < 0 {
            Err("write failed")
        } else {
            Ok(rc as usize)
        }
    }

    /// 读目录项
    ///
    /// # 返回
    /// 成功: `Some((name, kind))` ; 索引越界: `None`
    pub fn readdir(&self, index: usize) -> Option<(alloc::string::String, DevKind)> {
        let (raw_name, dev_type) = self.inner.readdir(index)?;
        let kind = DevKind::from_u8(dev_type)?;
        let end = raw_name.iter().position(|&b| b == 0).unwrap_or(raw_name.len());
        let name = alloc::string::String::from_utf8_lossy(&raw_name[..end]).into_owned();
        Some((name, kind))
    }

    /// 设备总数
    pub fn device_count(&self) -> u32 {
        self.inner.device_count()
    }
}

impl Default for SafeDevFs {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 全局实例
// ============================================================================

use spin::Once;
static GLOBAL_DEVFS: Once<SafeDevFs> = Once::new();

/// 初始化全局 DevFS
pub fn init_global() {
    GLOBAL_DEVFS.call_once(|| SafeDevFs::new());
}

/// 获取全局 DevFS 引用
///
/// # Safety
/// 调用前需保证 `init_global` 已执行
pub fn global() -> &'static SafeDevFs {
    GLOBAL_DEVFS
        .get()
        .expect("devfs::global() called before init_global()")
}

// ============================================================================
// 便利函数
// ============================================================================

/// 注册设备到全局 DevFS
pub fn register(name: &str, kind: DevKind) -> Result<(), &'static str> {
    global().register(name, kind)
}

/// 打开全局 DevFS 设备
pub fn open(path: &str) -> Result<DevFile, &'static str> {
    global().open(path)
}

/// 设备名最大长度
pub const fn max_name_len() -> usize {
    DEVFS_MAX_NAME
}

// ============================================================================
// 标准设备预注册
// ============================================================================

/// 注册所有标准设备 (null/zero/console/tty)
pub fn register_standard() {
    let _ = register("null", DevKind::Null);
    let _ = register("zero", DevKind::Zero);
    let _ = register("console", DevKind::Console);
    let _ = register("tty", DevKind::Tty);
}
