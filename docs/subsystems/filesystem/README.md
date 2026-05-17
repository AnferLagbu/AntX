# 文件系统子系统

> VFS框架与多种文件系统支持

---

## 🎯 概述

AntX支持多种文件系统：
- **VFS**: 虚拟文件系统（抽象层）
- **RamFS**: 内存文件系统
- **HvFS**: 混合文件系统
- **DevFS**: 设备文件系统
- **ProcFS**: 进程文件系统

---

## 📦 VFS抽象层

### 核心接口

```rust
pub trait FileSystem: Send + Sync {
    fn mount(&self, path: &str) -> Result<(), FsError>;
    fn unmount(&self, path: &str) -> Result<(), FsError>;
    fn open(&self, path: &str, flags: u32) -> Result<u32, FsError>;
    fn close(&self, fd: u32) -> Result<(), FsError>;
    fn read(&self, fd: u32, buf: &mut [u8]) -> Result<usize, FsError>;
    fn write(&self, fd: u32, buf: &[u8]) -> Result<usize, FsError>;
}
```

### Inode结构

```rust
pub struct Inode {
    pub ino: u64,              // Inode号
    pub mode: u16,             // 权限模式
    pub uid: u64,              // 所有者PWID
    pub size: u64,             // 文件大小
    pub atime: u64,            // 访问时间
    pub mtime: u64,            // 修改时间
    pub ctime: u64,            // 创建时间
    pub file_type: FileType,   // 文件类型
}

pub enum FileType {
    RegularFile,
    Directory,
    Symlink,
    Device,
}
```

---

## 📂 RamFS

内存文件系统，高性能易失性存储。

**特性**:
- 所有数据在内存中
- 读写速度极快
- 重启后数据丢失
- 适合临时文件和测试

---

## 📂 HvFS

混合文件系统，支持内存和磁盘模式。

**特性**:
- 内存模式：高性能
- 磁盘模式：持久化
- 运行时切换
- ZFS风格（COW、快照）

---

## 📂 DevFS

设备文件系统，动态设备节点。

**挂载点**: `/dev`

**设备节点**:
- `/dev/null` - 空设备
- `/dev/zero` - 零设备
- `/dev/console` - 控制台
- `/dev/tty0` - 终端

---

## 📂 ProcFS

进程文件系统，进程信息展示。

**挂载点**: `/proc`

**文件**:
- `/proc/[pid]/status` - 进程状态
- `/proc/[pid]/mem` - 内存映射
- `/proc/cpuinfo` - CPU信息
- `/proc/meminfo` - 内存信息

---

**最后更新**: 2026-05-18
