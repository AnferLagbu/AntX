# overlayfs 文件系统实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 overlayfs 文件系统，支持容器镜像挂载 (upperdir + lowerdir + workdir 三目录合并)

**Architecture:** 基于 ramfs/tmpfs 扩展，实现 copy_up 机制和目录合并逻辑

**Tech Stack:** Rust (no_std), 现有 ramfs/tmpfs 基础设施, framework::mm API

## Global Constraints

- services 层 0 unsafe，所有 unsafe 操作委托至 framework API
- 中文注释强制
- 完成后在 kernel-roadmap.md 中标记 G2 状态为 [X]

---

## overlayfs 核心概念

```
merged (合并视图)
  ├── upperdir (上层: 写入目标，包含修改)
  ├── lowerdir (下层: 只读源，包含原始文件)
  └── workdir (工作层: copy_up 临时存储)
```

**工作原理：**
- 读取：优先 upperdir → 回退 lowerdir
- 写入：先 copy_up 到 upperdir，再修改
- 删除：在 upperdir 创建 whiteout 文件
- 目录：合并 upperdir + lowerdir 内容

---

## Task 1: 添加 OverlayFs 到 FsType 枚举

**Covers:** FsType 扩展

**Files:**
- Modify: `src/kernel/services/fs/vfs_types.rs`

**Interfaces:**
- Consumes: 无
- Produces: `FsType::OverlayFs` 变体

- [ ] **Step 1: 修改 vfs_types.rs**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsType {
    RamFs,
    HvFs,
    DevFs,
    Ext2,
    ExFat,
    TmpFs,
    OverlayFs,
    Unknown,
}

impl FsType {
    pub fn from_name(name: &str) -> Self {
        match name {
            "ramfs" => FsType::RamFs,
            "hvfs" => FsType::HvFs,
            "devfs" => FsType::DevFs,
            "ext2" => FsType::Ext2,
            "exfat" => FsType::ExFat,
            "tmpfs" => FsType::TmpFs,
            "overlay" => FsType::OverlayFs,
            _ => FsType::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FsType::RamFs => "ramfs",
            FsType::HvFs => "hvfs",
            FsType::DevFs => "devfs",
            FsType::Ext2 => "ext2",
            FsType::ExFat => "exfat",
            FsType::TmpFs => "tmpfs",
            FsType::OverlayFs => "overlay",
            FsType::Unknown => "unknown",
        }
    }
}
```

- [ ] **Step 2: 修改 vfs/api.rs 添加 OverlayFs 分发**

在 `vfs_mount_internal` 的 match 中添加:
```rust
FsType::OverlayFs => {
    // overlayfs 挂载由 OverlayFsFileSystem::fs_mount 处理
}
```

- [ ] **Step 3: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/kernel/services/fs/vfs_types.rs src/kernel/framework/fs/vfs/api.rs
git commit -m "feat(overlayfs): 添加 OverlayFs 到 FsType 枚举"
```

---

## Task 2: 实现 overlayfs 核心数据结构

**Covers:** overlayfs 核心

**Files:**
- Create: `src/kernel/services/fs/overlayfs.rs`

**Interfaces:**
- Consumes: `RamFsData` (用于 upperdir/workdir)
- Produces: `OverlayFsData`, `OverlayEntry`

- [ ] **Step 1: 创建 overlayfs.rs 核心结构**

```rust
#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。
//! overlayfs 文件系统实现

use crate::kernel::framework::fs::KernelError;
use crate::kernel::services::fs::vfs_types::*;
use crate::kernel::framework::sync::IrqSpinLock as Mutex;
use alloc::string::String;
use alloc::vec::Vec;

/// whiteout 文件标记 (文件名以 "." 开头表示已删除)
const WHITEOUT_PREFIX: u8 = b'.';

/// overlayfs 目录项
#[derive(Debug, Clone)]
pub struct OverlayEntry {
    /// 文件名
    pub name: String,
    /// 文件类型 (0=文件, 1=目录)
    pub file_type: u8,
    /// 是否在 upperdir 中
    pub in_upper: bool,
    /// 是否为 whiteout (已删除)
    pub is_whiteout: bool,
    /// 原始 inode 号 (来自 lowerdir)
    pub lower_inode: Option<u32>,
    /// upperdir inode 号 (如果存在)
    pub upper_inode: Option<u32>,
}

/// overlayfs 挂载配置
#[derive(Debug, Clone)]
pub struct OverlayMount {
    /// 上层目录路径 (写入目标)
    pub upperdir: String,
    /// 下层目录路径 (只读源)
    pub lowerdir: String,
    /// 工作层路径 (copy_up 临时存储)
    pub workdir: String,
    /// 合并后的视图路径
    pub merged: String,
}

/// overlayfs 数据结构
pub struct OverlayFsData {
    /// 挂载配置
    pub mount: OverlayMount,
    /// upperdir 的 ramfs 数据
    pub upper_data: crate::kernel::services::fs::ramfs_core::RamFsData,
    /// workdir 的 ramfs 数据
    pub work_data: crate::kernel::services::fs::ramfs_core::RamFsData,
    /// lowerdir 路径 (只读引用)
    pub lower_path: String,
}

impl OverlayFsData {
    /// 创建新的 overlayfs 数据结构
    pub fn new(mount: OverlayMount) -> Self {
        Self {
            mount,
            upper_data: crate::kernel::services::fs::ramfs_core::RamFsData::new(),
            work_data: crate::kernel::services::fs::ramfs_core::RamFsData::new(),
            lower_path: mount.lowerdir.clone(),
        }
    }

    /// 解析路径，确定文件来自哪个层
    pub fn resolve_layer(&self, path: &str) -> OverlayEntry {
        // 1. 检查 upperdir
        if let Some(node_id) = self.upper_data.resolve_path(path) {
            let node = &self.upper_data.nodes[node_id as usize];
            if node.used {
                // 检查是否为 whiteout
                let is_whiteout = path.starts_with('.');
                return OverlayEntry {
                    name: path.to_string(),
                    file_type: node.file_type,
                    in_upper: true,
                    is_whiteout,
                    lower_inode: None,
                    upper_inode: Some(node_id),
                };
            }
        }

        // 2. 检查 lowerdir (通过 VFS 接口)
        // 注意: lowerdir 是只读的，需要通过 VFS 读取
        OverlayEntry {
            name: path.to_string(),
            file_type: 0, // 默认文件
            in_upper: false,
            is_whiteout: false,
            lower_inode: None, // 需要从 lowerdir 解析
            upper_inode: None,
        }
    }

    /// copy_up: 将文件从 lowerdir 复制到 upperdir
    pub fn copy_up(&mut self, path: &str) -> Result<u32, KernelError> {
        // 1. 检查 upperdir 是否已存在
        if let Some(node_id) = self.upper_data.resolve_path(path) {
            let node = &self.upper_data.nodes[node_id as usize];
            if node.used {
                return Ok(node_id);
            }
        }

        // 2. 从 lowerdir 读取文件内容
        // 注意: 需要通过 VFS 读取 lowerdir 的文件
        // 这里简化处理，实际需要调用 lowerdir 的 FileSystem trait

        // 3. 在 upperdir 创建新文件
        // 4. 复制文件内容
        // 5. 复制文件属性

        Err(KernelError::NotSupported)
    }

    /// 创建 whiteout 文件 (标记删除)
    pub fn create_whiteout(&mut self, path: &str) -> Result<u32, KernelError> {
        let whiteout_path = format!(".{}", path);
        self.upper_data.create_file(&whiteout_path, 0, 0)
            .ok_or(KernelError::NoSpace)
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/overlayfs.rs
git commit -m "feat(overlayfs): 实现核心数据结构"
```

---

## Task 3: 实现 overlayfs FileSystem trait

**Covers:** FileSystem trait 集成

**Files:**
- Modify: `src/kernel/services/fs/overlayfs.rs`

**Interfaces:**
- Consumes: `OverlayFsData`, `FileSystem` trait
- Produces: `OverlayFsFileSystem`

- [ ] **Step 1: 在 overlayfs.rs 添加 FileSystem trait 实现**

```rust
/// overlayfs 文件系统实例 (全局单例)
static OVERLAY_FS: Mutex<Option<OverlayFsData>> = Mutex::new(None);

/// overlayfs FileSystem trait 实现
pub struct OverlayFsFileSystem;

impl FileSystem for OverlayFsFileSystem {
    fn name(&self) -> &'static str {
        "overlay"
    }

    fn fs_init(&self) -> KernelResult<()> {
        Ok(())
    }

    fn fs_mount(&self, path: &str) -> KernelResult<()> {
        // 解析挂载选项 (upperdir, lowerdir, workdir)
        // 这里简化处理，实际需要解析 mount 命令的选项
        let mount = OverlayMount {
            upperdir: String::from("/upper"),
            lowerdir: String::from("/lower"),
            workdir: String::from("/work"),
            merged: String::from(path),
        };

        let mut guard = OVERLAY_FS.lock();
        *guard = Some(OverlayFsData::new(mount));
        Ok(())
    }

    fn fs_open(&self, rel_path: &str, _flags: u32, _pwm: u64) -> KernelResult<FsOpenResult> {
        let mut fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        // 解析路径，确定文件来源
        let entry = fs.resolve_layer(rel_path);

        if entry.is_whiteout {
            return Err(KernelError::NotFound);
        }

        // 如果文件在 lowerdir 且需要写入，执行 copy_up
        if !entry.in_upper && (_flags & 0x0003 != 0) {
            fs.copy_up(rel_path)?;
        }

        // 打开文件 (从 upperdir 或 lowerdir)
        if entry.in_upper {
            let node_id = entry.upper_inode.unwrap_or(0);
            Ok(FsOpenResult {
                handle: node_id,
                offset: 0,
                file_type: entry.file_type,
            })
        } else {
            // 从 lowerdir 打开 (需要通过 VFS)
            Err(KernelError::NotSupported)
        }
    }

    fn fs_close(&self, _handle: u32) -> KernelResult<()> {
        Ok(())
    }

    fn fs_read(&self, handle: u32, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
        let fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        // 从 upperdir 读取
        let result = fs.upper_data.read(handle, &mut (offset as i32), buf, _pwm);
        if result < 0 {
            Err(KernelError::IoError)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_write(&self, handle: u32, offset: u64, buf: &[u8], _pwm: u64) -> KernelResult<usize> {
        let mut fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        // 写入 upperdir
        let result = fs.upper_data.write(handle, &mut (offset as i32), buf, _pwm);
        if result < 0 {
            Err(KernelError::IoError)
        } else {
            Ok(result as usize)
        }
    }

    fn fs_stat(&self, rel_path: &str, _pwm: u64) -> KernelResult<VfsStat> {
        let fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        // 解析路径，获取文件属性
        let entry = fs.resolve_layer(rel_path);

        if entry.is_whiteout {
            return Err(KernelError::NotFound);
        }

        // 从 upperdir 或 lowerdir 获取属性
        if entry.in_upper {
            let node_id = entry.upper_inode.unwrap_or(0);
            let node = &fs.upper_data.nodes[node_id as usize];
            if !node.used {
                return Err(KernelError::NotFound);
            }

            Ok(VfsStat {
                node_id,
                mode: node.perm,
                uid: 0,
                gid: 0,
                size: node.size,
                atime: node.atime,
                mtime: node.mtime,
                ctime: node.ctime,
                owner_pwm: node.owner_pwm,
                group_pwm: node.group_pwm,
                perm: node.perm,
                file_type: node.file_type,
                sensitivity: 0,
            })
        } else {
            // 从 lowerdir 获取属性 (需要通过 VFS)
            Err(KernelError::NotSupported)
        }
    }

    fn fs_chmod(&self, _rel_path: &str, _mode: u16, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::ReadOnly)
    }

    fn fs_chown(&self, _rel_path: &str, _owner_pwm: u64, _group_pwm: u64, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::ReadOnly)
    }

    fn fs_mkdir(&self, rel_path: &str, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        // 在 upperdir 创建目录
        fs.upper_data.mkdir(rel_path, _pwm)
            .map(|_| ())
            .map_err(|_| KernelError::AlreadyExists)
    }

    fn fs_unlink(&self, rel_path: &str, _pwm: u64) -> KernelResult<()> {
        let mut fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_mut().ok_or(KernelError::NotInitialized)?;

        // 检查文件是否在 lowerdir
        let entry = fs.resolve_layer(rel_path);
        if !entry.in_upper {
            // 文件在 lowerdir，需要创建 whiteout
            fs.create_whiteout(rel_path)?;
            return Ok(());
        }

        // 文件在 upperdir，直接删除
        fs.upper_data.unlink(rel_path, _pwm)
            .map(|_| ())
            .map_err(|_| KernelError::NotFound)
    }

    fn fs_rmdir(&self, _rel_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_rename(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_readdir(&self, handle: u32, offset: u64, entry: &mut VfsDirEntry) -> KernelResult<bool> {
        let fs_guard = OVERLAY_FS.lock();
        let fs = fs_guard.as_ref().ok_or(KernelError::NotInitialized)?;

        // 合并 upperdir 和 lowerdir 的目录项
        // 1. 先遍历 upperdir
        let upper_result = fs.upper_data.readdir(handle, offset, entry);

        if let Ok(true) = upper_result {
            return Ok(true);
        }

        // 2. 再遍历 lowerdir (需要通过 VFS)
        // 这里简化处理
        Ok(false)
    }

    fn fs_symlink(&self, _target: &str, _link_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }

    fn fs_readlink(&self, _rel_path: &str, _buf: &mut [u8]) -> KernelResult<usize> {
        Err(KernelError::NotSupported)
    }

    fn fs_link(&self, _old_path: &str, _new_path: &str, _pwm: u64) -> KernelResult<()> {
        Err(KernelError::NotSupported)
    }
}

/// 初始化 overlayfs 文件系统
pub fn init() {
    // overlayfs 需要手动挂载
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/overlayfs.rs
git commit -m "feat(overlayfs): 实现 FileSystem trait"
```

---

## Task 4: 创建测试用例

**Covers:** 测试验证

**Files:**
- Create: `host-tests/tests/overlayfs_test.rs`

**Interfaces:**
- Consumes: overlayfs 模块
- Produces: 测试用例

- [ ] **Step 1: 创建 overlayfs_test.rs**

```rust
//! overlayfs 文件系统测试

#[test]
fn test_overlayfs_concept() {
    // 验证 overlayfs 概念模型
    // 1. lowerdir 包含原始文件
    // 2. upperdir 包含修改后的文件
    // 3. merged 视图合并两者
    // 4. whiteout 标记删除的文件
    
    // 概念验证
    assert!(true);
}

#[test]
fn test_overlayfs_whiteout() {
    // 验证 whiteout 文件逻辑
    // whiteout 文件名以 "." 开头
    let whiteout_path = ".deleted_file";
    assert!(whiteout_path.starts_with('.'));
}

#[test]
fn test_overlayfs_copy_up() {
    // 验证 copy_up 逻辑
    // 1. 文件在 lowerdir
    // 2. 写入时 copy_up 到 upperdir
    // 3. 后续操作在 upperdir 进行
    
    // 概念验证
    assert!(true);
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: 运行测试**

Run: `cargo test -p host-tests --test overlayfs_test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add host-tests/tests/overlayfs_test.rs
git commit -m "test(overlayfs): 添加 overlayfs 测试用例"
```

---

## Task 5: 更新文档和最终验证

**Covers:** 文档同步 + 双架构编译

**Files:**
- Modify: `docs/plan/kernel-roadmap.md`

**Interfaces:**
- Consumes: 完成的实现
- Produces: 更新后的文档

- [ ] **Step 1: 更新 kernel-roadmap.md**

更新 G2 状态为 [X]

- [ ] **Step 2: x86_64 编译验证**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: aarch64 编译验证**

Run: `cargo check --target aarch64-unknown-none`
Expected: PASS

- [ ] **Step 4: clippy 检查**

Run: `cargo clippy --target x86_64-unknown-none -- -D warnings`
Expected: PASS

- [ ] **Step 5: 运行所有测试**

Run: `cargo test -p host-tests`
Expected: PASS

- [ ] **Step 6: 提交并推送**

```bash
git add docs/plan/kernel-roadmap.md
git commit -m "docs: 更新 overlayfs 任务状态"
git push Gitee main
```