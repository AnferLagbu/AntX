# 文件系统 Rust 重写方案

## 一、概述

本文档描述将 AntX 文件系统模块从 C 语言重写为 Rust 的详细方案。文件系统是内核中数据结构最复杂、最容易出错的模块之一，使用 Rust 可以显著提高内存安全性和代码可维护性。

## 二、当前架构分析

### 2.1 现有模块

| 模块 | 文件 | 功能 |
|------|------|------|
| VFS | `src/fs/vfs.c` | 虚拟文件系统抽象层 |
| RamFS | `src/fs/ramfs.c` | 内存文件系统 |
| DiskFS | `src/fs/diskfs.c` | 磁盘文件系统 |
| DevFS | `src/fs/devfs.c` | 设备文件系统 |
| ProcFS | `src/fs/procfs.c` | 进程信息文件系统 |
| HvFS | `src/hvfs/hvfs.c` | 高级虚拟文件系统 |

### 2.2 现有问题

1. **内存安全**: 大量指针操作，容易发生内存泄漏和悬垂指针
2. **错误处理**: 使用整数错误码，缺乏类型安全
3. **状态管理**: 文件状态机复杂，难以维护
4. **并发安全**: 缺乏统一的锁机制

## 三、Rust 架构设计

### 3.1 核心类型定义

```rust
// src/fs/mod.rs

#![no_std]

use core::path::Path;

pub type FileDescriptor = u32;
pub type InodeNumber = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    RegularFile,
    Directory,
    SymbolicLink,
    BlockDevice,
    CharDevice,
    Pipe,
}

#[derive(Debug)]
pub struct FileMetadata {
    pub file_type: FileType,
    pub permissions: Permissions,
    pub size: u64,
    pub inode: InodeNumber,
    pub links: u32,
}

#[derive(Debug)]
pub enum FsError {
    NotFound,
    PermissionDenied,
    NotADirectory,
    IsADirectory,
    FileExists,
    NoSpaceLeft,
    IoError,
    InvalidPath,
    TooManyOpenFiles,
}

pub type FsResult<T> = Result<T, FsError>;
```

### 3.2 VFS Trait 定义

```rust
// src/fs/vfs.rs

use core::path::Path;

pub trait FileSystem: Send + Sync {
    fn name(&self) -> &'static str;
    
    fn mount(&mut self, mount_point: &Path) -> FsResult<()>;
    fn unmount(&mut self) -> FsResult<()>;
    
    fn open(&mut self, path: &Path, flags: OpenFlags) -> FsResult<File>;
    fn create(&mut self, path: &Path, file_type: FileType) -> FsResult<File>;
    fn mkdir(&mut self, path: &Path) -> FsResult<()>;
    fn remove(&mut self, path: &Path) -> FsResult<()>;
    
    fn read(&self, file: &File, buf: &mut [u8]) -> FsResult<usize>;
    fn write(&mut self, file: &File, buf: &[u8]) -> FsResult<usize>;
    fn seek(&mut self, file: &mut File, offset: i64, whence: SeekWhence) -> FsResult<u64>;
    
    fn readdir(&self, dir: &File) -> FsResult<DirectoryIterator>;
    fn stat(&self, path: &Path) -> FsResult<FileMetadata>;
}

#[derive(Debug, Clone, Copy)]
pub struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    pub create: bool,
    pub truncate: bool,
    pub exclusive: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum SeekWhence {
    Set,
    Current,
    End,
}

pub struct File {
    pub inode: InodeNumber,
    pub position: u64,
    pub flags: OpenFlags,
    pub filesystem: &'static str,
}

pub struct DirectoryIterator {
    current: usize,
    entries: [DirectoryEntry; 64],
}
```

### 3.3 RamFS 实现

```rust
// src/fs/ramfs.rs

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

pub struct RamFileSystem {
    root: Mutex<RamNode>,
    mount_point: Option<String>,
    open_files: Mutex<BTreeMap<FileDescriptor, File>>,
    next_fd: Mutex<FileDescriptor>,
}

struct RamNode {
    name: String,
    file_type: FileType,
    content: Vec<u8>,
    children: BTreeMap<String, RamNode>,
    metadata: FileMetadata,
}

impl RamFileSystem {
    pub fn new() -> Self {
        Self {
            root: Mutex::new(RamNode {
                name: String::from("/"),
                file_type: FileType::Directory,
                content: Vec::new(),
                children: BTreeMap::new(),
                metadata: FileMetadata {
                    file_type: FileType::Directory,
                    permissions: Permissions { read: true, write: true, execute: true },
                    size: 0,
                    inode: 1,
                    links: 2,
                },
            }),
            mount_point: None,
            open_files: Mutex::new(BTreeMap::new()),
            next_fd: Mutex::new(3),
        }
    }
    
    fn traverse_path(&self, path: &Path) -> FsResult<&RamNode> {
        let mut current = self.root.lock();
        
        for component in path.components() {
            match component {
                core::path::Component::Normal(name) => {
                    let name_str = name.to_str().ok_or(FsError::InvalidPath)?;
                    current = current.children.get(name_str)
                        .ok_or(FsError::NotFound)?;
                }
                core::path::Component::ParentDir => {
                    // Handle ".." - need parent reference
                }
                _ => {}
            }
        }
        
        Ok(current)
    }
}

impl FileSystem for RamFileSystem {
    fn name(&self) -> &'static str {
        "ramfs"
    }
    
    fn open(&mut self, path: &Path, flags: OpenFlags) -> FsResult<File> {
        let node = self.traverse_path(path)?;
        
        let fd = {
            let mut next_fd = self.next_fd.lock();
            let fd = *next_fd;
            *next_fd += 1;
            fd
        };
        
        let file = File {
            inode: node.metadata.inode,
            position: 0,
            flags,
            filesystem: "ramfs",
        };
        
        self.open_files.lock().insert(fd, file.clone());
        Ok(file)
    }
    
    fn read(&self, file: &File, buf: &mut [u8]) -> FsResult<usize> {
        // Implementation
        Ok(0)
    }
    
    fn write(&mut self, file: &File, buf: &[u8]) -> FsResult<usize> {
        // Implementation
        Ok(0)
    }
    
    // ... 其他方法实现
}
```

### 3.4 文件系统注册表

```rust
// src/fs/registry.rs

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use spin::Mutex;

pub struct FileSystemRegistry {
    filesystems: Mutex<BTreeMap<String, Arc<dyn FileSystem>>>,
    mount_table: Mutex<BTreeMap<String, MountInfo>>,
}

pub struct MountInfo {
    pub mount_point: String,
    pub filesystem: Arc<dyn FileSystem>,
    pub flags: MountFlags,
}

#[derive(Debug, Clone, Copy)]
pub struct MountFlags {
    pub read_only: bool,
    pub no_exec: bool,
    pub no_suid: bool,
}

impl FileSystemRegistry {
    pub const fn new() -> Self {
        Self {
            filesystems: Mutex::new(BTreeMap::new()),
            mount_table: Mutex::new(BTreeMap::new()),
        }
    }
    
    pub fn register(&self, name: &str, fs: Arc<dyn FileSystem>) -> FsResult<()> {
        self.filesystems.lock().insert(String::from(name), fs);
        Ok(())
    }
    
    pub fn mount(&self, fs_name: &str, mount_point: &str) -> FsResult<()> {
        let fs = self.filesystems.lock()
            .get(fs_name)
            .ok_or(FsError::NotFound)?
            .clone();
        
        let mount_info = MountInfo {
            mount_point: String::from(mount_point),
            filesystem: fs,
            flags: MountFlags::default(),
        };
        
        self.mount_table.lock().insert(String::from(mount_point), mount_info);
        Ok(())
    }
    
    pub fn resolve_path(&self, path: &Path) -> FsResult<(Arc<dyn FileSystem>, &Path)> {
        // Find the mounted filesystem for this path
        // Implementation
        todo!()
    }
}

// 全局文件系统注册表
pub static FS_REGISTRY: FileSystemRegistry = FileSystemRegistry::new();
```

## 四、迁移计划

### 4.1 阶段一：基础设施 (1 周)

1. **配置 Rust 构建环境**
   - 添加 `Cargo.toml`
   - 配置 `no_std` 环境
   - 添加必要的 crate 依赖

```toml
# Cargo.toml
[package]
name = "antx-kernel"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["staticlib"]

[profile.dev]
panic = "abort"

[profile.release]
panic = "abort"

[dependencies]
spin = "0.9"
bitflags = "2.4"
x86_64 = "0.14"

[dependencies.alloc-cortex-m]
version = "0.4"
```

2. **创建 C-Rust FFI 接口**
   - 定义导出函数
   - 设置链接脚本

```rust
// src/lib.rs
#![no_std]
#![no_main]

mod fs;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn rust_fs_init() -> i32 {
    // Initialize filesystem
    0
}
```

### 4.2 阶段二：RamFS 重写 (1 周)

1. 实现 `RamFileSystem` 结构
2. 实现 `FileSystem` trait
3. 添加单元测试
4. 与 C 代码集成测试

### 4.3 阶段三：VFS 层重写 (1 周)

1. 实现 `FileSystemRegistry`
2. 实现路径解析
3. 实现文件描述符管理
4. 与现有文件系统集成

### 4.4 阶段四：其他文件系统 (1 周)

1. DevFS 重写
2. ProcFS 重写
3. DiskFS 重写（如需要）

## 五、C-Rust 互操作

### 5.1 从 C 调用 Rust

```c
// src/include/fs_rust.h

#ifndef _FS_RUST_H
#define _FS_RUST_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// Rust 文件系统接口
int rust_fs_init(void);
int rust_fs_mount(const char *fs_name, const char *mount_point);
int rust_fs_open(const char *path, int flags);
int rust_fs_read(int fd, void *buf, uint64_t count);
int rust_fs_write(int fd, const void *buf, uint64_t count);
int rust_fs_close(int fd);

#ifdef __cplusplus
}
#endif

#endif // _FS_RUST_H
```

### 5.2 从 Rust 调用 C

```rust
// src/fs/c_bindings.rs

extern "C" {
    pub fn serial_puts(port: u16, s: *const i8);
    pub fn pmm_alloc_page() -> *mut u8;
    pub fn pmm_free_page(addr: *mut u8);
    pub fn vmm_map_page(pml4: u64, virt: u64, phys: u64, flags: u64);
}

// 安全包装
pub fn log_message(msg: &str) {
    unsafe {
        serial_puts(0x3F8, msg.as_ptr() as *const i8);
    }
}
```

## 六、测试策略

### 6.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ramfs_create_file() {
        let mut fs = RamFileSystem::new();
        let path = Path::new("/test.txt");
        
        let result = fs.create(path, FileType::RegularFile);
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_ramfs_write_read() {
        let mut fs = RamFileSystem::new();
        let path = Path::new("/test.txt");
        
        fs.create(path, FileType::RegularFile).unwrap();
        let file = fs.open(path, OpenFlags::write()).unwrap();
        
        let data = b"Hello, World!";
        fs.write(&file, data).unwrap();
        
        let mut buf = [0u8; 13];
        fs.read(&file, &mut buf).unwrap();
        
        assert_eq!(&buf, data);
    }
}
```

### 6.2 集成测试

```rust
// tests/fs_integration.rs

use antx_kernel::fs::*;

#[test]
fn test_vfs_mount() {
    let ramfs = Arc::new(RamFileSystem::new());
    FS_REGISTRY.register("ramfs", ramfs).unwrap();
    FS_REGISTRY.mount("ramfs", "/").unwrap();
    
    let file = FS_REGISTRY.open("/test.txt", OpenFlags::create()).unwrap();
    assert!(file.inode > 0);
}
```

## 七、风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 构建复杂度增加 | 中 | 使用 `cargo-xbuild` 或 `xargo` |
| FFI 边界开销 | 低 | 批量操作减少调用次数 |
| 调试困难 | 中 | 保留 C 版本作为参考 |
| 内存分配器兼容 | 高 | 使用统一的内核分配器 |

## 八、预期收益

1. **内存安全**: 消除 use-after-free、buffer overflow 等漏洞
2. **错误处理**: 类型安全的 `Result<T, E>` 替代错误码
3. **可维护性**: 清晰的 trait 抽象，易于扩展新文件系统
4. **并发安全**: `Arc<Mutex<T>>` 提供线程安全

## 九、参考资源

- [Rust OSDev Community](https://rust-osdev.com/)
- [Writing an OS in Rust](https://os.phil-opp.com/)
- [The Rust Programming Language](https://doc.rust-lang.org/book/)
- [Embedded Rust Book](https://docs.rust-embedded.org/book/)
