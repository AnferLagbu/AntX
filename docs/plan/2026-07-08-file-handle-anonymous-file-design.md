# 文件句柄系统与匿名文件系统设计

> 基于 POSIX 标准要求和业界调研，为 QueenX 设计文件句柄共享机制和匿名文件系统。

## [S1] 问题定义

### 1.1 当前缺陷

**缺陷 1: dup() 语义不正确**

当前 `VfsFile` 将 fd 编号、文件偏移、权限绑定在一起：
```rust
pub struct VfsFile {
    pub fd: u32,
    pub node_id: u32,
    pub offset: u64,      // ← 问题: dup 会复制这个
    pub flags: u32,       // ← 问题: dup 会复制这个
    pub pwm: u64,
    pub used: bool,
    pub file_type: u8,
    pub path: [u8; VFS_MAX_PATH],
}
```

`vfs_dup` 执行 `fd_table[i] = fd_table[old].clone()`，导致两个 fd 有独立的 offset。这违反 POSIX：
> "多个指向同一打开文件描述的文件描述符应共享同一组文件状态标志和文件偏移。"

**缺陷 2: 匿名文件不完整**

当前 `memfd_create` 委托 tmpfs：
- 需要 `/dev/shm` 已挂载
- 创建 `/dev/shm/memfd_<pid>` 路径文件
- 无真正的匿名文件 (不依赖路径)

**缺陷 3: name_to_handle_at / open_by_handle_at 未实现**

### 1.2 POSIX 标准要求

| 功能 | POSIX 要求 | 当前状态 |
|------|-----------|---------|
| dup() 共享 offset | ✅ POSIX.1-2001 要求 | ❌ 不满足 |
| open() 创建新描述 | ✅ POSIX 要求 | ✅ 满足 |
| O_CLOEXEC per-FD | ✅ POSIX 要求 | ⚠️ 部分实现 |
| O_APPEND 原子 | ✅ POSIX 要求 | ❌ 未保证 |
| memfd_create | ❌ Linux-specific | ⚠️ tmpfs 委托 |
| name_to_handle_at | ❌ Linux-specific | ❌ 未实现 |

---

## [S2] 方案 A: 增量添加 FileDescription 层

### 2.1 核心思路

在现有 VFS 基础上添加一层抽象，引入 `FileDescription` (打开文件描述)：

```
当前:
  fd → VfsFile { inode_id, offset, flags, pwm, path }
  (dup 深拷贝, offset 独立)

改为:
  fd → FdEntry { handle_id, cloexec }
  handle_id → FileDescription { inode_id, offset, flags, pwm, refcount }
  (dup 共享 FileDescription, offset 共享)
```

### 2.2 数据结构

```rust
/// 打开文件描述 (类似 Linux struct file)
///
/// 多个 fd 可以指向同一个 FileDescription (dup 语义)
/// offset 和 flags 在所有共享者之间共享
pub struct FileDescription {
    /// VFS inode ID (文件系统内部句柄)
    pub inode_id: u32,
    /// 挂载点索引 (用于 FileSystem trait 分发)
    pub mount_idx: u32,
    /// 共享文件偏移 (原子操作, dup 共享)
    pub offset: AtomicU64,
    /// 共享状态标志 (O_RDONLY, O_APPEND 等, dup 共享)
    pub flags: u32,
    /// 权限凭证
    pub pwm: u64,
    /// 引用计数 (dup 增加, close 减少)
    pub refcount: AtomicU32,
    /// 文件类型
    pub file_type: u8,
    /// 是否匿名文件 (memfd)
    pub is_anonymous: bool,
}

/// FD 表条目 (per-process)
///
/// 仅包含指向 FileDescription 的引用
pub struct FdEntry {
    /// 指向 FileDescription 的索引
    pub handle_id: u32,
    /// FD 级标志 (CLOEXEC, 不随 dup 共享)
    pub cloexec: bool,
    /// 是否使用中
    pub used: bool,
}
```

### 2.3 操作语义

| 操作 | 实现 |
|------|------|
| `open()` | 创建新 `FileDescription`, 插入新 `FdEntry` |
| `dup(fd)` | 复制 `FdEntry.handle_id`, 增加 `FileDescription.refcount` |
| `close(fd)` | 移除 `FdEntry`, 减少 `refcount`, 为 0 时释放 |
| `read(fd)` | 通过 `handle_id` 找到 `FileDescription`, 使用共享 offset |
| `write(fd)` | 同上, 写后更新共享 offset |
| `lseek(fd)` | 更新共享 `FileDescription.offset` |
| `fcntl(F_SETFL)` | 更新共享 `FileDescription.flags` |

### 2.4 匿名文件支持

```rust
/// 匿名文件系统 (memfd 基础)
///
/// 复用 RamFsData 作为数据存储, 但不依赖路径
pub struct AnonymousFs {
    inner: RamFsData,
    // 无路径解析, 直接通过 inode_id 访问
}

impl FileSystem for AnonymousFs {
    fn fs_open(&self, _path: &str, flags: u32, pwm: u64) -> KernelResult<FsOpenResult> {
        // 分配新 inode (无需路径)
        let node_id = self.inner.alloc_node()?;
        Ok(FsOpenResult { handle: node_id, offset: 0, file_type: 0 })
    }

    fn fs_pread_inode(&self, node_id: u32, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize> {
        // 直接读取 inode 数据 (支持 mmap)
        self.inner.read_at(node_id, offset, buf)
    }
}
```

### 2.5 memfd_create 实现

```rust
pub fn memfd_create_syscall(name_ptr: u64, flags: u32) -> Result<usize, Errno> {
    // 1. 分配 FileDescription (匿名, 无路径)
    let handle_id = FILE_HANDLE_TABLE.alloc(FileDescription {
        inode_id: ANONYMOUS_INODE,  // 特殊 inode ID
        mount_idx: ANONYMOUS_MOUNT, // 特殊挂载索引
        offset: AtomicU64::new(0),
        flags: O_RDWR,
        pwm: current_pwm(),
        refcount: AtomicU32::new(1),
        file_type: 0,
        is_anonymous: true,
    })?;

    // 2. 在当前进程 fd 表中分配 fd
    let fd = current_fd_table.alloc_fd(FdEntry {
        handle_id,
        cloexec: (flags & MFD_CLOEXEC) != 0,
        used: true,
    })?;

    Ok(fd)
}
```

### 2.6 name_to_handle_at / open_by_handle_at

```rust
/// name_to_handle_at — 导出文件句柄
pub fn name_to_handle_at_syscall(
    dirfd: i32,
    path_ptr: u64,
    handle_type: i32,
    handle_ptr: u64,
    mnt_id_ptr: u64,
    flags: u32,
) -> Result<usize, Errno> {
    // 1. 解析路径, 获取 inode_id 和 mount_idx
    // 2. 序列化为 handle 结构
    // 3. 写入用户空间
    // 需要: 文件句柄序列化格式
    Err(Errno::ENOSYS)  // 暂存根
}

/// open_by_handle_at — 通过句柄打开文件
pub fn open_by_handle_at_syscall(
    mount_fd: i32,
    handle_ptr: u64,
    handle_type: i32,
    flags: u32,
) -> Result<usize, Errno> {
    // 1. 从 mount_fd 获取挂载点
    // 2. 反序列化 handle, 获取 inode_id
    // 3. 打开文件
    Err(Errno::ENOSYS)  // 暂存根
}
```

### 2.7 优点

- **最小改动**: 在现有 VFS 上增量添加, 不重写现有代码
- **POSIX 兼容**: dup() 共享 offset 满足标准要求
- **低风险**: 逐步迁移, 可以先实现核心部分
- **独立性**: 不照搬 Linux 实现, 用 QueenX 自己的命名和结构

### 2.8 缺点

- **兼容性有限**: 不支持 name_to_handle_at / open_by_handle_at (Linux-specific)
- **架构限制**: 仍然基于固定大小数组, 不支持 per-process FD 表
- **未来重构**: 如果需要更完整的功能, 可能需要再次重构

### 2.9 工作量

| 任务 | 预计时间 |
|------|---------|
| FileDescription 结构 + 全局表 | 2 天 |
| 修改 VfsFile 为 FdEntry | 2 天 |
| 修改 open/close/dup | 2 天 |
| AnonymousFs 实现 | 2 天 |
| memfd_create 完善 | 1 天 |
| 测试 | 2 天 |
| **总计** | **~2 周** |

---

## [S3] 方案 B: 完全重构 VFS

### 3.1 核心思路

替换现有 VfsFile, 引入 Linux 风格的三层结构, 但用 QueenX 自己的实现：

```
当前:
  VfsFile (扁平结构, 32 个全局条目)

改为:
  FdTable (per-process) → FileHandle (共享) → Inode (文件系统)
```

### 3.2 数据结构

```rust
/// 文件句柄 (QueenX 风格, 不叫 struct file)
///
/// 对应 POSIX "打开文件描述", 多个 fd 可共享
pub struct OpenFile {
    /// 文件系统 inode 引用
    pub inode: Arc<dyn Inode>,
    /// 共享文件偏移
    pub offset: AtomicU64,
    /// 共享状态标志
    pub flags: u32,
    /// 权限凭证
    pub pwm: u64,
    /// 引用计数
    pub refcount: AtomicU32,
    /// 文件类型
    pub file_type: FileType,
}

/// Inode trait (QueenX 风格, 不叫 struct inode)
///
/// 文件系统必须实现此 trait
pub trait Inode: Send + Sync {
    fn read(&self, offset: u64, buf: &mut [u8], pwm: u64) -> KernelResult<usize>;
    fn write(&self, offset: u64, buf: &[u8], pwm: u64) -> KernelResult<usize>;
    fn stat(&self, pwm: u64) -> KernelResult<VfsStat>;
    fn truncate(&self, size: u64, pwm: u64) -> KernelResult<()>;
    fn seek(&self, offset: i64, whence: SeekWhence, current: u64) -> KernelResult<u64>;
}

/// Per-process FD 表
pub struct ProcessFdTable {
    entries: Vec<Option<Arc<OpenFile>>>,
    cloexec: Vec<bool>,
}
```

### 3.3 与方案 A 的区别

| 方面 | 方案 A | 方案 B |
|------|--------|--------|
| FD 表 | 全局固定数组 (现有) | Per-process (新) |
| 文件描述 | 新增 FileDescription 层 | 替换 VfsFile 为 OpenFile |
| Inode | 使用现有 node_id | 新增 Inode trait |
| dup() | 共享 handle_id | 共享 Arc<OpenFile> |
| 迁移成本 | 低 (增量添加) | 高 (重写现有代码) |
| POSIX 合规 | 部分 (无 per-process FD) | 完全 |
| 未来扩展 | 有限 | 高 |

### 3.4 优点

- **完全 POSIX 合规**: per-process FD 表 + 共享 OpenFile
- **架构清晰**: 三层分离, 每层职责明确
- **未来扩展**: 支持更复杂的 FD 操作 (pipe 集成, socket 集成)
- **QueenX 风格**: 用 Inode trait 而非 Linux 的 f_op 表

### 3.5 缺点

- **改动巨大**: 几乎所有 FS 相关代码都要改
- **风险高**: 重构可能引入回归
- **耗时长**: 需要 4-6 周
- **测试不足**: 当前 host-tests 覆盖不够

### 3.6 工作量

| 任务 | 预计时间 |
|------|---------|
| OpenFile + Inode trait 定义 | 3 天 |
| Per-process FD 表 | 3 天 |
| 重写 VFS API (open/close/read/write) | 5 天 |
| 适配 ramfs/tmpfs/devfs/procfs | 5 天 |
| 适配 ext2/exfat | 3 天 |
| AnonymousFs + memfd_create | 3 天 |
| 测试 + 回归修复 | 5 天 |
| **总计** | **~4-5 周** |

---

## [S4] 方案对比

| 维度 | 方案 A (增量) | 方案 B (重构) |
|------|-------------|-------------|
| **POSIX 合规** | ⚠️ 部分 (无 per-process FD) | ✅ 完全 |
| **改动范围** | 小 (5 个文件) | 大 (20+ 文件) |
| **风险** | 低 | 高 |
| **耗时** | 2 周 | 4-5 周 |
| **未来扩展** | 有限 | 高 |
| **与 Linux 差异** | 保留现有架构 | 用 QueenX 风格重构 |
| **测试需求** | 中 | 高 |

### 推荐

**如果目标是快速实现 G4 (syscall 补全)：** 方案 A

**如果目标是长期架构质量：** 方案 B

**如果两者都要：** 先 A 后 B (先实现功能, 后续重构)

---

## [S5] 实施进度

### 5.1 已完成项 (折中实现)

| 任务 | 状态 | 文件 |
|------|------|------|
| OpenFile 结构 | ✅ | `services/fs/vfs_types.rs:397` |
| OpenFileTable (全局表) | ✅ | `services/fs/open_file_table.rs` |
| AnonymousFs | ✅ | `services/fs/anonymous.rs` |
| memfd_create | ✅ | `services/proc/memfd.rs` |
| name_to_handle_at | ✅ | `services/fs/file_handle.rs` |
| open_by_handle_at | ✅ | `services/fs/file_handle.rs` |
| Per-process FD 表 (折中) | ✅ | `services/fs/process_fd_table.rs` |
| POSIX dup 语义 | ✅ | `framework/fs/vfs/api.rs:1318-1362` |

### 5.2 折中实现 vs 完整方案 B

| 维度 | 折中实现 (当前) | 完整方案 B |
|------|----------------|------------|
| **Per-process FD 表** | ✅ 固定数组 `[FdEntry; 256]` | ✅ `Vec<Option<Arc<OpenFile>>>` |
| **POSIX dup 语义** | ✅ 共享 OpenFile (handle_id) | ✅ 共享 `Arc<OpenFile>` |
| **dup/dup2** | ✅ 已实现 | ✅ 已实现 |
| **CLOEXEC** | ✅ 已实现 | ✅ 已实现 |
| **exec 清理** | ✅ 已实现 | ✅ 已实现 |
| **Inode trait** | ❌ 未实现 | ✅ 完整 trait 定义 |
| **动态 FD 分配** | ❌ 固定 256 | ✅ Vec 动态扩展 |
| **文件系统抽象** | 复用现有 VFS | 新增 Inode trait |
| **改动范围** | 小 (2 文件) | 大 (20+ 文件) |
| **风险** | 低 | 高 |
| **耗时** | 已完成 | 4-5 周 |

### 5.3 完整方案 B 实施计划

**目标**: 从折中实现升级到完整方案 B，引入 Inode trait + Vec + Arc<dyn Inode>

**实施步骤**:

| 阶段 | 任务 | 文件 | 工作量 |
|------|------|------|--------|
| Phase 1 | Inode trait 定义 | 新增 `services/fs/inode.rs` | 2 天 |
| Phase 2 | OpenFile 改用 `Arc<dyn Inode>` | `services/fs/vfs_types.rs` | 1 天 |
| Phase 3 | ProcessFdTable 改用 Vec | `services/fs/process_fd_table.rs` | 1 天 |
| Phase 4 | 重写 VFS API | `framework/fs/vfs/api.rs` | 5 天 |
| Phase 5 | 适配 ramfs/tmpfs/devfs/procfs | `services/fs/` | 5 天 |
| Phase 6 | 适配 ext2/exfat | `services/fs/ext2/`, `services/fs/exfat/` | 3 天 |
| Phase 7 | AnonymousFs + memfd | `services/fs/anonymous.rs` | 2 天 |
| Phase 8 | 测试 + 回归修复 | 多个文件 | 5 天 |
| **总计** | | | **~4-5 周** |

**风险评估**:
- 改动范围大 (20+ 文件)，可能引入回归
- 需要全面测试覆盖
- 建议分阶段实施，每阶段验证编译和测试

**前置条件**:
- 当前折中实现已验证 POSIX dup 语义正确
- host-tests 全部通过
- 双架构编译 0w0e
