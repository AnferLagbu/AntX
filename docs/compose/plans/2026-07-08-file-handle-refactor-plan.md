# VFS 文件句柄重构实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use compose:subagent (recommended) or compose:execute to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 重构 QueenX VFS，引入 OpenFile + Inode trait + Per-process FD 表，实现 POSIX 合规的文件描述符共享和匿名文件系统。

**Architecture:** 三层分离: FdTable (per-process) → OpenFile (共享) → Inode (文件系统)。替换现有 VfsFile 扁平结构，实现 dup() 共享 offset 语义。

**Tech Stack:** Rust (no_std), QueenX VFS, RamFsData (匿名文件基础)

## Global Constraints

- services 层 0 unsafe，所有 unsafe 操作委托至 framework API
- 中文注释强制
- 双架构编译 0 error 0 warning
- 每个 Task 完成后必须通过 host-tests
- 不照搬 Linux 实现，用 QueenX 自己的命名和结构

---

## 文件结构

| 文件 | 职责 | 状态 |
|------|------|------|
| `services/fs/types.rs` | 新增 OpenFile, Inode trait, FileType, SeekWhence | 新建 |
| `services/fs/fd_table.rs` | 重写为 Per-process FD 表 | 重写 |
| `services/fs/anonymous.rs` | AnonymousFs 实现 (memfd 基础) | 新建 |
| `services/fs/vfs_manager.rs` | 修改为使用 OpenFile | 修改 |
| `services/fs/open.rs` | 修改 open/close/dup 实现 | 修改 |
| `services/fs/io.rs` | 修改 read/write/lseek 实现 | 修改 |
| `services/fs/vfs_types.rs` | 适配新 Inode trait | 修改 |
| `services/fs/ramfs_core.rs` | 实现 Inode trait | 修改 |
| `services/fs/tmpfs.rs` | 适配新接口 | 修改 |
| `services/fs/devfs.rs` | 适配新接口 | 修改 |
| `services/fs/procfs.rs` | 适配新接口 | 修改 |
| `services/fs/ext2/` | 适配新接口 | 修改 |
| `services/fs/exfat/` | 适配新接口 | 修改 |
| `services/proc/memfd.rs` | 使用 AnonymousFs | 修改 |
| `services/proc/pidfd.rs` | 适配新 FD 表 | 修改 |
| `services/syscall/dispatch.rs` | 添加新 syscall 分支 | 修改 |
| `host-tests/tests/` | 新增文件句柄测试 | 新建 |

---

## Task 1: 定义核心数据结构

**Covers:** [S2, S3]

**Files:**
- Create: `src/kernel/services/fs/types.rs` (扩展)
- Create: `src/kernel/services/fs/open_file.rs`

**Interfaces:**
- Consumes: 现有 VfsStat, Errno
- Produces: OpenFile, Inode trait, FileType, SeekWhence

- [ ] **Step 1: 在 types.rs 中添加新类型**

```rust
// OpenFile — 打开文件描述 (POSIX open file description)
pub struct OpenFile {
    /// 文件系统 inode 引用
    pub inode_id: u32,
    /// 挂载点索引
    pub mount_idx: u32,
    /// 共享文件偏移 (原子操作, dup 共享)
    pub offset: core::sync::atomic::AtomicU64,
    /// 共享状态标志 (O_RDONLY, O_APPEND 等)
    pub flags: u32,
    /// 权限凭证
    pub pwm: u64,
    /// 引用计数
    pub refcount: core::sync::atomic::AtomicU32,
    /// 文件类型
    pub file_type: u8,
    /// 是否匿名文件
    pub is_anonymous: bool,
}

impl OpenFile {
    pub fn new(inode_id: u32, mount_idx: u32, flags: u32, pwm: u64, file_type: u8) -> Self {
        Self {
            inode_id,
            mount_idx,
            offset: core::sync::atomic::AtomicU64::new(0),
            flags,
            pwm,
            refcount: core::sync::atomic::AtomicU32::new(1),
            file_type,
            is_anonymous: false,
        }
    }

    pub fn inc_ref(&self) {
        self.refcount.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    pub fn dec_ref(&self) -> u32 {
        self.refcount.fetch_sub(1, core::sync::atomic::Ordering::Release)
    }
}

/// Inode trait — 文件系统必须实现此 trait
pub trait Inode: Send + Sync {
    fn read(&self, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize>;
    fn write(&self, offset: u64, buf: &[u8], pwm: u64) -> KernelResult<usize>;
    fn stat(&self, pwm: u64) -> KernelResult<VfsStat>;
    fn truncate(&self, size: u64, pwm: u64) -> KernelResult<()>;
}
```

- [ ] **Step 2: 验证编译**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/types.rs src/kernel/services/fs/open_file.rs
git commit -m "feat(vfs): 定义 OpenFile + Inode trait 核心数据结构"
```

---

## Task 2: 重写 FD 表为 Per-process

**Covers:** [S2, S3]

**Files:**
- Modify: `src/kernel/services/fs/fd_table.rs` (重写)
- Modify: `src/kernel/services/fs/vfs_manager.rs` (适配)

**Interfaces:**
- Consumes: OpenFile (Task 1)
- Produces: FdTable, alloc_fd, get_file, close_fd

- [ ] **Step 1: 重写 fd_table.rs**

```rust
/// Per-process FD 表
pub struct FdTable {
    /// FD 条目: handle_id → OpenFile 引用
    entries: [Option<u32>; 64],  // handle_id 索引
    /// CLOEXEC 标志 (per-FD, 不随 dup 共享)
    cloexec: [bool; 64],
    /// 使用状态
    used: [bool; 64],
}

impl FdTable {
    pub fn new() -> Self { /* ... */ }
    pub fn alloc_fd(&mut self, handle_id: u32, cloexec: bool) -> Option<usize> { /* ... */ }
    pub fn get_handle_id(&self, fd: usize) -> Option<u32> { /* ... */ }
    pub fn close_fd(&mut self, fd: usize) -> Option<u32> { /* ... */ }
    pub fn is_cloexec(&self, fd: usize) -> bool { /* ... */ }
    pub fn set_cloexec(&mut self, fd: usize, cloexec: bool) { /* ... */ }
}
```

- [ ] **Step 2: 验证编译**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/fd_table.rs
git commit -m "refactor(vfs): 重写 FD 表为 Per-process 结构"
```

---

## Task 3: 全局 OpenFile 表

**Covers:** [S2]

**Files:**
- Create: `src/kernel/services/fs/open_file_table.rs`

**Interfaces:**
- Consumes: OpenFile (Task 1)
- Produces: OpenFileTable, alloc, get, dec_ref

- [ ] **Step 1: 实现全局 OpenFile 表**

```rust
/// 全局 OpenFile 表 (内核管理)
pub struct OpenFileTable {
    handles: spin::Mutex<[Option<OpenFile>; 256]>,
    next_id: core::sync::atomic::AtomicU32,
}

impl OpenFileTable {
    pub fn alloc(&self, file: OpenFile) -> Option<u32> { /* ... */ }
    pub fn get(&self, id: u32) -> Option<&OpenFile> { /* ... */ }
    pub fn dec_ref(&self, id: u32) { /* ... */ }
}

pub static OPEN_FILE_TABLE: OpenFileTable = OpenFileTable::new();
```

- [ ] **Step 2: 验证编译**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/open_file_table.rs
git commit -m "feat(vfs): 实现全局 OpenFile 表"
```

---

## Task 4: 修改 VFS API 使用 OpenFile

**Covers:** [S2]

**Files:**
- Modify: `src/kernel/framework/fs/vfs/api.rs` (修改 open/close/read/write/dup)

**Interfaces:**
- Consumes: OpenFile, FdTable, OpenFileTable (Task 1-3)
- Produces: 修改后的 vfs_open, vfs_close, vfs_read, vfs_write, vfs_dup

- [ ] **Step 1: 修改 vfs_open 创建 OpenFile**

在 `vfs_open_internal` 中:
1. 创建 `OpenFile` 而非 `VfsFile`
2. 插入全局 `OPEN_FILE_TABLE`
3. 在进程 fd 表中分配 fd, 存储 handle_id

- [ ] **Step 2: 修改 vfs_close 释放 OpenFile**

在 `vfs_close_internal` 中:
1. 从 fd 表获取 handle_id
2. 调用 `OPEN_FILE_TABLE.dec_ref(handle_id)`
3. 如果 refcount 为 0, 释放 OpenFile

- [ ] **Step 3: 修改 vfs_read/vfs_write 使用共享 offset**

在 `vfs_read_internal` / `vfs_write_internal` 中:
1. 从 fd 表获取 handle_id
2. 从 OPEN_FILE_TABLE 获取 OpenFile
3. 使用 `OpenFile.offset` (共享)

- [ ] **Step 4: 修改 vfs_dup 共享 OpenFile**

在 `vfs_dup_internal` 中:
1. 获取旧 fd 的 handle_id
2. 调用 `OPEN_FILE_TABLE.get(handle_id).inc_ref()`
3. 在新 fd 槽存储相同 handle_id

- [ ] **Step 5: 验证编译**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 6: 运行测试**

Run: `make test-host`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/kernel/framework/fs/vfs/api.rs
git commit -m "refactor(vfs): VFS API 使用 OpenFile 实现 dup 共享语义"
```

---

## Task 5: 实现 AnonymousFs

**Covers:** [S2, S3]

**Files:**
- Create: `src/kernel/services/fs/anonymous.rs`

**Interfaces:**
- Consumes: RamFsData (现有), FileSystem trait
- Produces: AnonymousFs, ANONYMOUS_INODE

- [ ] **Step 1: 实现 AnonymousFs**

```rust
/// 匿名文件系统 — memfd 基础
///
/// 复用 RamFsData 作为数据存储, 但不依赖路径
pub struct AnonymousFs {
    inner: crate::kernel::services::fs::ramfs_core::RamFsData,
}

impl AnonymousFs {
    pub fn new() -> Self {
        Self { inner: RamFsData::new() }
    }

    pub fn alloc_inode(&self) -> Option<u32> {
        self.inner.alloc_node()
    }

    pub fn read_at(&self, node_id: u32, offset: u64, buf: &mut [u8]) -> Option<usize> {
        self.inner.read_at(node_id, offset, buf)
    }

    pub fn write_at(&self, node_id: u32, offset: u64, buf: &[u8]) -> Option<usize> {
        self.inner.write_at(node_id, offset, buf)
    }
}

pub static ANONYMOUS_FS: AnonymousFs = AnonymousFs::new();
```

- [ ] **Step 2: 验证编译**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/fs/anonymous.rs
git commit -m "feat(vfs): 实现 AnonymousFs 匿名文件系统"
```

---

## Task 6: 完善 memfd_create

**Covers:** [S2, S3]

**Files:**
- Modify: `src/kernel/services/proc/memfd.rs`

**Interfaces:**
- Consumes: AnonymousFs, OpenFile, OpenFileTable (Task 1-5)
- Produces: 完善的 memfd_create_syscall

- [ ] **Step 1: 重写 memfd_create 使用 AnonymousFs**

```rust
pub fn memfd_create_syscall(_name_ptr: u64, flags: u32) -> Result<usize, Errno> {
    // 1. 在 AnonymousFs 中分配 inode
    let inode_id = crate::kernel::services::fs::anonymous::ANONYMOUS_FS
        .alloc_inode()
        .ok_or(Errno::ENOMEM)?;

    // 2. 创建 OpenFile
    let handle_id = crate::kernel::services::fs::open_file_table::OPEN_FILE_TABLE
        .alloc(OpenFile {
            inode_id,
            mount_idx: 0,  // AnonymousFs 不需要 mount_idx
            offset: AtomicU64::new(0),
            flags: O_RDWR,
            pwm: current_pwm(),
            refcount: AtomicU32::new(1),
            file_type: 0,
            is_anonymous: true,
        })
        .ok_or(Errno::ENOMEM)?;

    // 3. 在当前进程 fd 表中分配 fd
    let fd = current_fd_table.alloc_fd(handle_id, (flags & MFD_CLOEXEC) != 0)
        .ok_or(Errno::EMFILE)?;

    Ok(fd)
}
```

- [ ] **Step 2: 验证编译**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/kernel/services/proc/memfd.rs
git commit -m "feat(memfd): 完善 memfd_create 使用 AnonymousFs"
```

---

## Task 7: 实现 name_to_handle_at / open_by_handle_at

**Covers:** [S2]

**Files:**
- Modify: `src/kernel/services/syscall/types.rs` (添加常量)
- Modify: `src/kernel/services/syscall/dispatch.rs` (添加分发)
- Create: `src/kernel/services/fs/file_handle.rs`

**Interfaces:**
- Consumes: OpenFile, OpenFileTable (Task 1-3)
- Produces: name_to_handle_at_syscall, open_by_handle_at_syscall

- [ ] **Step 1: 在 types.rs 添加常量**

```rust
pub const SYS_name_to_handle_at: u64 = 303;
pub const SYS_open_by_handle_at: u64 = 304;
```

- [ ] **Step 2: 实现 file_handle.rs**

```rust
/// 文件句柄序列化格式
#[repr(C)]
pub struct FileHandle {
    pub handle_type: u32,
    pub inode_id: u32,
    pub mount_idx: u32,
}

/// name_to_handle_at — 导出文件句柄
pub fn name_to_handle_at_syscall(
    dirfd: i32,
    path_ptr: u64,
    _handle_type: i32,
    handle_ptr: u64,
    mnt_id_ptr: u64,
    _flags: u32,
) -> Result<usize, Errno> {
    // 1. 解析路径, 获取 inode_id 和 mount_idx
    // 2. 序列化为 FileHandle
    // 3. 写入用户空间
    Err(Errno::ENOSYS)  // TODO: 完整实现
}

/// open_by_handle_at — 通过句柄打开文件
pub fn open_by_handle_at_syscall(
    _mount_fd: i32,
    handle_ptr: u64,
    _handle_type: i32,
    _flags: u32,
) -> Result<usize, Errno> {
    // 1. 从 mount_fd 获取挂载点
    // 2. 反序列化 handle
    // 3. 打开文件
    Err(Errno::ENOSYS)  // TODO: 完整实现
}
```

- [ ] **Step 3: 添加 dispatch 分支**

- [ ] **Step 4: 验证编译**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/kernel/services/fs/file_handle.rs src/kernel/services/syscall/dispatch.rs
git commit -m "feat(vfs): 实现 name_to_handle_at / open_by_handle_at 框架"
```

---

## Task 8: 适配 ramfs/tmpfs/devfs/procfs

**Covers:** [S2]

**Files:**
- Modify: `src/kernel/services/fs/ramfs_core.rs`
- Modify: `src/kernel/services/fs/tmpfs.rs`
- Modify: `src/kernel/services/fs/devfs.rs`
- Modify: `src/kernel/services/fs/procfs.rs`

**Interfaces:**
- Consumes: OpenFile, Inode trait (Task 1)
- Produces: 各 FS 实现 Inode trait

- [ ] **Step 1: ramfs_core.rs 实现 Inode trait**

```rust
impl Inode for RamFsNode {
    fn read(&self, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize> {
        // 委托给现有 read_at 方法
    }
    fn write(&self, offset: u64, buf: &[u8], pwm: u64) -> KernelResult<usize> {
        // 委托给现有 write_at 方法
    }
    fn stat(&self, pwm: u64) -> KernelResult<VfsStat> { /* ... */ }
    fn truncate(&self, size: u64, pwm: u64) -> KernelResult<()> { /* ... */ }
}
```

- [ ] **Step 2: tmpfs.rs 适配**

- [ ] **Step 3: devfs.rs 适配**

- [ ] **Step 4: procfs.rs 适配**

- [ ] **Step 5: 验证编译**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/kernel/services/fs/ramfs_core.rs src/kernel/services/fs/tmpfs.rs
git commit -m "refactor(vfs): ramfs/tmpfs/devfs/procfs 适配 Inode trait"
```

---

## Task 9: 适配 ext2/exfat

**Covers:** [S2]

**Files:**
- Modify: `src/kernel/services/fs/ext2/` (适配)
- Modify: `src/kernel/services/fs/exfat/` (适配)

**Interfaces:**
- Consumes: OpenFile, Inode trait (Task 1)
- Produces: ext2/exfat 实现 Inode trait

- [ ] **Step 1: ext2 适配 Inode trait**

- [ ] **Step 2: exfat 适配 Inode trait**

- [ ] **Step 3: 验证编译**

Run: `cargo check --target x86_64-unknown-none`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/kernel/services/fs/ext2/ src/kernel/services/fs/exfat/
git commit -m "refactor(vfs): ext2/exfat 适配 Inode trait"
```

---

## Task 10: 全面测试 + 回归修复

**Covers:** [S2, S3]

**Files:**
- Modify: `host-tests/` (更新现有测试)
- Create: `host-tests/tests/file_handle_test.rs`

**Interfaces:**
- Consumes: 所有 Task 1-9
- Produces: 测试通过

- [ ] **Step 1: 运行现有 host-tests**

Run: `make test-host`
Expected: 识别回归

- [ ] **Step 2: 修复回归问题**

- [ ] **Step 3: 添加文件句柄测试**

```rust
#[test]
fn test_dup_shares_offset() {
    // 打开文件, 写入数据
    // dup fd
    // 通过旧 fd 读取, 验证偏移已更新
    // 通过新 fd 读取, 验证共享偏移
}

#[test]
fn test_memfd_create() {
    // 创建 memfd
    // 写入数据
    // 验证数据可读
}

#[test]
fn test_anonymous_file_mmap() {
    // 创建 memfd
    // mmap
    // 写入数据
    // 验证 mmap 区域可读
}
```

- [ ] **Step 4: 运行所有测试**

Run: `make test-host`
Expected: PASS

- [ ] **Step 5: 验证双架构编译**

Run: `./ci/build.sh all`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add host-tests/
git commit -m "test(vfs): 文件句柄重构全面测试 + 回归修复"
```

---

## 工作量汇总

| Task | 描述 | 预计时间 |
|------|------|---------|
| 1 | 核心数据结构 | 1 天 |
| 2 | Per-process FD 表 | 1 天 |
| 3 | 全局 OpenFile 表 | 1 天 |
| 4 | VFS API 修改 | 2 天 |
| 5 | AnonymousFs | 1 天 |
| 6 | memfd_create | 1 天 |
| 7 | name_to_handle_at | 1 天 |
| 8 | ramfs/tmpfs/devfs/procfs 适配 | 2 天 |
| 9 | ext2/exfat 适配 | 1 天 |
| 10 | 测试 + 回归修复 | 2 天 |
| **总计** | | **~12 天 (2.5 周)** |
