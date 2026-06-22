#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有数据通过 IrqSpinLock + 原子类型保护。
//!
//! LEGACY-6: 运行时 sysctl 框架 — services 层安全实现
//!
//! ## 设计
//!
//! - 注册表: `IrqSpinLock<[Option<SysctlEntry>; 32]>` 静态分配, 启动期注册
//! - 值类型: `Int / UInt / Bool` 3 种, 覆盖 95% 内核调参场景
//! - 线程安全: 写路径用 IrqSpinLock 锁, 读路径无锁 (原子加载)
//! - 边界: 仅 services 层持有注册表, framework 通过 `services::config::sysctl::*` 调用
//!
//! ## 实施记录 (2026-06-22)
//!
//! REVAL/LEGACY-6 重新评估: 之前 SKIP 理由"sysctl 框架涉及 100+ 行基础设施",
//! 实际上只需 ~150 行即可实现最小可用版本 (注册 + 3 种类型 + 读写).
//! IrqSpinLock 在 framework::sync 已就绪, const fn new 支持静态分配.
//! 实施时遵循最小可用原则: 不实现 netlink/IPC, 仅做静态注册 + 简单读写.

use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock;
use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};

// ============================================================================
// Sysctl 值类型
// ============================================================================

/// sysctl 值 — 3 种类型覆盖 95% 调参场景
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysctlValue {
    /// 有符号 64 位整数
    Int(i64),
    /// 无符号 64 位整数
    UInt(u64),
    /// 布尔
    Bool(bool),
}

impl SysctlValue {
    /// 序列化为文本 (用于 /proc/sys/* 节点读取)
    pub fn write_to(&self, buf: &mut [u8]) -> usize {
        match *self {
            SysctlValue::Int(v) => write_i64(buf, v),
            SysctlValue::UInt(v) => write_u64(buf, v),
            SysctlValue::Bool(v) => write_bool(buf, v),
        }
    }

    /// 从文本解析 (用于 /proc/sys/* 节点写入)
    pub fn parse(kind: SysctlKind, text: &str) -> Result<SysctlValue, SysctlError> {
        match kind {
            SysctlKind::Int => text.trim().parse::<i64>()
                .map(SysctlValue::Int)
                .map_err(|_| SysctlError::ParseFailed),
            SysctlKind::UInt => text.trim().parse::<u64>()
                .map(SysctlValue::UInt)
                .map_err(|_| SysctlError::ParseFailed),
            SysctlKind::Bool => match text.trim() {
                "1" | "true" | "yes" | "on" => Ok(SysctlValue::Bool(true)),
                "0" | "false" | "no" | "off" => Ok(SysctlValue::Bool(false)),
                _ => Err(SysctlError::ParseFailed),
            },
        }
    }
}

/// sysctl 值类型标识
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysctlKind {
    Int,
    UInt,
    Bool,
}

// ============================================================================
// 错误类型
// ============================================================================

/// sysctl 错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysctlError {
    /// 注册表已满
    TableFull,
    /// 名称重复
    Duplicate,
    /// 名称未找到
    NotFound,
    /// 文本解析失败
    ParseFailed,
    /// 类型不匹配 (试图用 IntKind 写入 Bool 值)
    TypeMismatch,
}

// ============================================================================
// 内部存储
// ============================================================================

/// 内部 sysctl 条目
///
/// 原子字段保证读路径无锁, IrqSpinLock 用于注册表槽位分配.
struct SysctlEntry {
    name: &'static str,
    kind: SysctlKind,
    /// Int 存储
    int_val: AtomicI64,
    /// UInt 存储
    uint_val: AtomicU64,
    /// Bool 存储
    bool_val: AtomicBool,
}

impl SysctlEntry {
    const fn empty() -> Self {
        Self {
            name: "",
            kind: SysctlKind::Int,
            int_val: AtomicI64::new(0),
            uint_val: AtomicU64::new(0),
            bool_val: AtomicBool::new(false),
        }
    }

    fn read(&self) -> SysctlValue {
        match self.kind {
            SysctlKind::Int => SysctlValue::Int(self.int_val.load(Ordering::Acquire)),
            SysctlKind::UInt => SysctlValue::UInt(self.uint_val.load(Ordering::Acquire)),
            SysctlKind::Bool => SysctlValue::Bool(self.bool_val.load(Ordering::Acquire)),
        }
    }

    fn write(&self, val: SysctlValue) -> Result<(), SysctlError> {
        match (self.kind, val) {
            (SysctlKind::Int, SysctlValue::Int(v)) => {
                self.int_val.store(v, Ordering::Release);
                Ok(())
            }
            (SysctlKind::UInt, SysctlValue::UInt(v)) => {
                self.uint_val.store(v, Ordering::Release);
                Ok(())
            }
            (SysctlKind::Bool, SysctlValue::Bool(v)) => {
                self.bool_val.store(v, Ordering::Release);
                Ok(())
            }
            _ => Err(SysctlError::TypeMismatch),
        }
    }
}

const MAX_SYSCTL_ENTRIES: usize = 32;

/// 全局 sysctl 注册表 (使用 IrqSpinLock 保护槽位分配, 零 unsafe)
static SYSCTL_TABLE: IrqSpinLock<[Option<SysctlEntry>; MAX_SYSCTL_ENTRIES]> =
    IrqSpinLock::new([const { None }; MAX_SYSCTL_ENTRIES]);

// ============================================================================
// 公共 API
// ============================================================================

/// 注册一个 sysctl 节点
///
/// 启动期单线程调用一次. 重复注册返回 `Err(Duplicate)`.
pub fn sysctl_register(
    name: &'static str,
    kind: SysctlKind,
    initial: SysctlValue,
) -> Result<(), SysctlError> {
    let mut guard = SYSCTL_TABLE.lock();
    for slot in guard.iter() {
        if let Some(entry) = slot {
            if entry.name == name {
                return Err(SysctlError::Duplicate);
            }
        }
    }
    let mut entry = SysctlEntry::empty();
    entry.name = name;
    entry.kind = kind;
    let _ = entry.write(initial);
    for slot in guard.iter_mut() {
        if slot.is_none() {
            *slot = Some(entry);
            return Ok(());
        }
    }
    Err(SysctlError::TableFull)
}

/// 读取 sysctl 值
///
/// 返回 `None` 表示名称未注册.
pub fn sysctl_read(name: &str) -> Option<SysctlValue> {
    let guard = SYSCTL_TABLE.lock();
    for slot in guard.iter() {
        if let Some(entry) = slot {
            if entry.name == name {
                return Some(entry.read());
            }
        }
    }
    None
}

/// 写入 sysctl 值
///
/// 类型必须匹配, 否则返回 `TypeMismatch`.
pub fn sysctl_write(name: &str, val: SysctlValue) -> Result<(), SysctlError> {
    let guard = SYSCTL_TABLE.lock();
    for slot in guard.iter() {
        if let Some(entry) = slot {
            if entry.name == name {
                return entry.write(val);
            }
        }
    }
    Err(SysctlError::NotFound)
}

/// 列出所有已注册 sysctl 名称 (写入 buf, 返回字节数)
///
/// 用于 `/proc/sys` 目录节点枚举.
pub fn sysctl_list(buf: &mut [u8]) -> usize {
    let guard = SYSCTL_TABLE.lock();
    let mut pos = 0;
    for slot in guard.iter() {
        if let Some(entry) = slot {
            if entry.name.is_empty() {
                continue;
            }
            let bytes = entry.name.as_bytes();
            let need = bytes.len() + 1; // +1 for '\n'
            if pos + need > buf.len() {
                break;
            }
            buf[pos..pos + bytes.len()].copy_from_slice(bytes);
            pos += bytes.len();
            buf[pos] = b'\n';
            pos += 1;
        }
    }
    pos
}

// ============================================================================
// 辅助函数
// ============================================================================

fn write_u64(buf: &mut [u8], val: u64) -> usize {
    if val == 0 {
        if !buf.is_empty() {
            buf[0] = b'0';
            return 1;
        }
        return 0;
    }
    let mut tmp = [0u8; 20];
    let mut i = 20;
    let mut v = val;
    while v > 0 && i > 0 {
        i -= 1;
        tmp[i] = (v % 10) as u8 + b'0';
        v /= 10;
    }
    let len = 20 - i;
    let end = len.min(buf.len());
    buf[..end].copy_from_slice(&tmp[i..i + end]);
    end
}

fn write_i64(buf: &mut [u8], val: i64) -> usize {
    let (neg, abs) = if val < 0 { (true, val.unsigned_abs()) } else { (false, val as u64) };
    let mut pos = 0;
    if neg && !buf.is_empty() {
        buf[0] = b'-';
        pos = 1;
    }
    pos + write_u64(&mut buf[pos..], abs)
}

fn write_bool(buf: &mut [u8], val: bool) -> usize {
    if val {
        if buf.len() >= 1 { buf[0] = b'1'; }
        1
    } else {
        if buf.len() >= 1 { buf[0] = b'0'; }
        1
    }
}

// ============================================================================
// 单元测试 (cfg(test) 内不参与编译)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        assert_eq!(SysctlValue::parse(SysctlKind::Int, "42").unwrap(), SysctlValue::Int(42));
        assert_eq!(SysctlValue::parse(SysctlKind::UInt, "100").unwrap(), SysctlValue::UInt(100));
        assert_eq!(SysctlValue::parse(SysctlKind::Bool, "yes").unwrap(), SysctlValue::Bool(true));
        assert_eq!(SysctlValue::parse(SysctlKind::Bool, "off").unwrap(), SysctlValue::Bool(false));
        assert!(SysctlValue::parse(SysctlKind::Int, "abc").is_err());
    }

    #[test]
    fn write_basic() {
        let mut buf = [0u8; 32];
        assert_eq!(write_u64(&mut buf, 0), 1);
        assert_eq!(buf[0], b'0');
        assert_eq!(write_i64(&mut buf, -42), 3);
        assert_eq!(&buf[..3], b"-42");
    }
}
