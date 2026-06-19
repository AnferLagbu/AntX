# E6: VFS 策略提取与文件系统解耦计划

> 本文档记录 VFS/RamFS/HvFS 策略提取与解耦的完整方案、分析结论与执行路径.
> 目标: 将文件系统策略逻辑从 framework 迁移到 services, 缩减 TCB, 并为新增文件系统建立可扩展架构.

---

## 一、背景与动机

### 1.1 当前问题

AntX 的 VFS 层 (`framework/fs/vfs/api.rs`) 存在两个职责混合:

1. **系统调用边界** (机制): unsafe 用户指针操作 (`UserReadPtr`/`UserWritePtr`/`UserRefMut`)
2. **文件系统分发** (策略): 14 个 `match fs_type { RamFs => ..., HvFs => ..., Unknown => -1 }` 分发点

每新增一个文件系统, 需要在 api.rs 的 14+ 个分发点各加一个分支, 并修改 `FsType` 枚举. 这违反了 framekernel 架构的"机制在 framework, 策略在 services"原则, 也阻碍了文件系统的可扩展性.

### 1.2 目标

- **TCB 缩减**: 将 RamFS (1,639 行) + HvFS (6,154 行) + dcache (876 行) = 8,669 行策略代码从 framework 移到 services
- **可扩展性**: 新增文件系统只需在 services 层实现 `FileSystem` trait, 无需修改 framework
- **安全契约**: services 层保持 `#![deny(unsafe_code)]`, 所有 unsafe 操作留在 framework

---

## 二、当前耦合分析

### 2.1 模块规模与安全属性

| 模块 | 行数 | unsafe 块 | framework 外部依赖 |
|------|------|-----------|-------------------|
| `vfs/api.rs` | 1,415 | 15 (用户指针) | → RamFS 24 处, → HvFS 30 处 |
| `ramfs/ramfs.rs` | 1,639 | 0 | → dcache 14 处 |
| `hvfs/` (18 文件) | 6,154 | 10 (磁盘序列化) | → block driver 24 处, → credo 4 处, → sync 多处 |
| `vfs/dcache.rs` | 876 | 0 | 无外部依赖 |

### 2.2 核心耦合点

1. **api.rs 的 14 个 `match fs_type` 分发点**: 硬编码 RamFS/HvFS 调用路径
2. **api.rs 直接访问 RamFS 内部字段**: `vfs_fchmod`/`vfs_fchown`/`vfs_link`/`vfs_symlink`/`vfs_readlink` 直接操作 `ramfs.nodes[node_id]`
3. **api.rs 暴露 RamFS 类型**: `RamFsDirEntry` 直接在 api.rs 中使用 (2 处)
4. **RamFS 方法签名与 HvFS 不统一**: RamFS 用 `node_id`, HvFS 用 `path/fd`
5. **HvFS 子模块间强耦合**: 18 个文件互相引用, 需整体迁移

### 2.3 非 VFS 分发文件系统现状

DevFS、ProcFS、InitRamFS 三个文件系统**不经过 VFS api.rs 的 `match fs_type` 分发**，各自独立运作。

| 文件系统 | 行数 | unsafe 块 | framework 依赖 | services 代理层 | 接入 VFS 分发 |
|---------|------|-----------|---------------|----------------|-------------|
| DevFS | 295 | 0 | `IrqSpinLock` | `services/fs/devfs.rs` (229 行, SafeDevFs 代理) | 否 |
| ProcFS | 245 | 0 | `IrqSpinLock`, `pmm_api` (2 处读内存统计) | `services/fs/procfs.rs` (191 行, SafeProcFs 代理) | 否 |
| InitRamFS | 333 | 1 (`unpack` 函数, `from_raw_parts`) | `vfs::api` (6 处: mkdir/open/write/close/symlink) | 无 | 否 |

#### 2.3.1 DevFS 分析

- **0 unsafe**, 295 行, 纯策略 (设备注册/IO 路由)
- services 层已有 `SafeDevFs` 代理 (229 行), 但仍引用 `framework::fs::devfs::devfs::DEVFS_DATA` 静态变量
- **迁移可行性**: 高. 将 `DevfsData` 及其全局实例迁移到 services, framework 仅 re-export 即可
- **VFS 分发接入**: DevFS 不走 VFS open/read/write 路径, 有独立的 `devfs_open`/`devfs_read`/`devfs_write`. 若需统一, 需在 `FileSystem` trait 中增加设备文件语义支持 (目前无必要)

#### 2.3.2 ProcFS 分析

- **0 unsafe**, 245 行, 纯策略 (虚拟文件内容生成)
- services 层已有 `SafeProcFs` 代理 (191 行), 同样引用 framework 静态变量
- **迁移可行性**: 高. `pmm_api::pmm_get_total_pages`/`pmm_get_free_pages` 是 safe 函数, services 可直接调用
- **VFS 分发接入**: 同 DevFS, ProcFS 有独立路径. 若需统一, 可在 `FileSystem` trait 中增加虚拟文件语义

#### 2.3.3 InitRamFS 分析

- **1 处 unsafe**: `unpack` 函数中 `core::slice::from_raw_parts(data, len)` — 将 bootloader 传入的原始指针转为切片
- **与 VFS 强耦合**: `unpack` 调用 `vfs_mkdir`/`vfs_open`/`vfs_write`/`vfs_close`/`vfs_symlink` (6 处), 是 VFS 的消费者而非实现
- **迁移可行性**: 低, 且无必要. 原因:
  1. `unpack` 在内核启动时一次性调用, 不属于运行时策略
  2. 其 unsafe 是对 bootloader 传入指针的安全封装, 属于机制而非策略
  3. 它是 VFS 的**调用者** (通过 vfs_api 写入文件), 不是 VFS 的**被分发对象**
  4. 迁移到 services 后仍需调用 framework 的 VFS API, 无法消除耦合
- **VFS 分发接入**: 不适用. InitRamFS 是 cpio 解析器, 不是文件系统实现

### 2.4 HvFS 对 framework 的外部依赖详情

| 依赖 | 调用点数 | 是否 safe API | services 可否调用 |
|------|---------|-------------|------------------|
| `driver::block` (hdd_read/write_sector 等) | 24 | safe (`&mut [u8]`/`&[u8]`) | 可以 |
| `credo::api` (pwm 权限检查) | 4 | safe | 可以 |
| `sync::{Mutex, IrqSpinLock, OnceLock}` | 多处 | safe | 可以 (已有先例) |
| `vfs::types::KernelError` | 1 | 纯数据类型 | 可以 re-export |

**关键发现**: HvFS 的 10 个 unsafe 全部是磁盘数据结构序列化 (`as_bytes()`/`from_bytes_unaligned()`), 不是硬件操作. 硬件操作走 `block::hdd_*`, 全部是 safe 函数.

---

## 三、已完成工作

### 3.1 E6-1: flock.rs 策略提取 ✅

- **迁移内容**: `FlockTable` + `PosixLockTable` + 全部公共 API (730+ 行)
- **framework 层**: 转为 re-export 层, 零逻辑代码
- **services 层**: `services/fs/flock.rs`, `#![deny(unsafe_code)]`, 0 unsafe
- **TCB 收益**: -730 行

### 3.2 E6-2: inotify.rs 策略提取 ✅

- **迁移内容**: watch 管理 + 事件队列 + 通知分发 (590+ 行)
- **framework 层**: re-export + `sys_inotify_read` (保留 unsafe 用户缓冲区写入)
- **services 层**: `services/fs/inotify.rs`, `#![deny(unsafe_code)]`, 0 unsafe
- **新增 safe API**: `inotify_read_events(fd, max_count) -> Option<(Vec<InotifyEvent>, usize)>` 供 framework 调用
- **TCB 收益**: -590 行

### 3.3 累计 TCB 缩减

| 模块 | 迁移前行数 | 迁移后 framework 行数 | 缩减 |
|------|-----------|---------------------|------|
| flock | 730 | ~30 (re-export) | -700 |
| inotify | 590 | ~50 (re-export + sys_inotify_read) | -540 |
| dcache | 876 | ~30 (re-export) | -846 |
| DevFS | 295 | ~13 (re-export) | -282 |
| RamFS | 1,639 | ~10 (re-export) | -1,629 |
| ProcFS | 245 | ~7 (re-export) | -238 |
| **合计** | **4,375** | **~140** | **-4,235** |

### 3.4 E6-3: dcache 策略提取 ✅

- **迁移内容**: dcache + icache 全部实现 (876 行)
- **framework 层**: re-export 所有公共 API
- **services 层**: `services/fs/dcache.rs`, `#![deny(unsafe_code)]`, 0 unsafe
- **TCB 收益**: -846 行

### 3.5 E6-7: DevFS 迁移到 services ✅

- **迁移内容**: DevfsData 完整实现 + DEVFS_DATA 全局实例 + SafeDevFs 代理 (295 行)
- **framework 层**: re-export 公共 API
- **services 层**: `services/fs/devfs.rs`, `#![deny(unsafe_code)]`, 0 unsafe
- **关键改动**: `klog_info!` (含 unsafe) 替换为 `framework::klog::serial_write_bytes` (safe)
- **TCB 收益**: -282 行

### 3.6 E6-4: VFS 分发策略提取 (FileSystem trait) ✅

- **已完成**:
  1. `FileSystem` trait 定义 (`framework/fs/vfs/types.rs`): 20 个方法, 覆盖 open/close/read/write/stat/chmod/chown/mkdir/unlink/rmdir/rename/readdir/symlink/readlink/link/truncate/seek/resolve_path/create
  2. `FsOpenResult` 类型: 统一 RamFS (node_id) / HvFS (fd) 的不透明 handle
  3. `VfsMount` 增加 `fs: Option<&'static dyn FileSystem>` 字段
  4. `VfsManager` 增加 `resolve_mount_fs()` 和 `mount_with_fs()` 方法
  5. RamFS `FileSystem` trait 实现 (ramfs.rs 末尾)
  6. HvFS `FileSystem` trait 实现 (hvfs.rs 末尾)
  7. `VfsDirEntry` 增加 `Default` 实现
  8. api.rs 全部 14 处 `match fs_type` 替换为 trait object 分发 (优先) + fallback:
     - `vfs_mount_internal`: 带 trait object 挂载
     - `vfs_open_internal`: trait 分发 + CREAT fallback
     - `vfs_read_internal`: trait 分发 + pcache 快路径保留
     - `vfs_write_internal`: trait 分发 + inotify/epoll 通知保留
     - `vfs_unlink_internal`: trait 分发 + POSIX 锁/inotify 通知保留
     - `vfs_truncate_internal`: trait 分发 + inotify 通知保留
     - `vfs_mkdir_internal`: trait 分发 + inotify 通知保留
     - `vfs_rmdir_internal`: trait 分发
     - `vfs_stat_internal`: trait 分发 + uid/gid 映射保留
     - `vfs_readdir_internal`: trait 分发
     - `vfs_chmod` / `vfs_chown_ext`: trait 分发
     - `vfs_rename`: trait 分发
     - `vfs_seek`: trait 分发
  9. 双架构编译 + 审计通过

### 3.7 E6-5: RamFS 迁移到 services ✅

- **迁移内容**: `RamFsData` 完整实现 + `RAMFS_DATA` 全局实例 (1,639 行)
- **framework 层**: `framework/fs/ramfs/ramfs.rs` 转为 re-export 层 (~10 行)
- **services 层**: `services/fs/ramfs_core.rs`, 实现 `FileSystem` trait
- **api.rs fallback 移除**: 全部 `match fs_type { RamFs => RAMFS_DATA.lock()... }` fallback 路径已删除, 改为 trait object 分发:
  - `vfs_symlink`: 直接 `fs.fs_symlink()`
  - `vfs_readlink`: 直接 `fs.fs_readlink()`
  - `vfs_rename`: 删除 RamFS/HvFS fallback, 仅保留 trait 分发
  - `vfs_seek`: 删除 RamFS/HvFS fallback, 仅保留 trait 分发
  - `vfs_fstat`: 改为 `fs.fs_stat()` trait 分发
  - `vfs_link`: 改为 `fs.fs_link()` trait 分发
- **TCB 收益**: -1,629 行

### 3.8 E6-8: ProcFS 迁移到 services ✅

- **迁移内容**: `ProcfsData` 完整实现 + `PROCFS_DATA` 全局实例 (245 行)
- **framework 层**: `framework/fs/procfs/procfs.rs` 转为 re-export 层 (~7 行)
- **services 层**: `services/fs/procfs_core.rs`, `#![deny(unsafe_code)]`, 0 unsafe
- **依赖**: `framework::cpu::get_cpu_info()`, `framework::mm::api`, `framework::config::procfs` — 全部 safe API, services 可调用
- **services/fs/procfs.rs**: `SafeProcFs` 代理 import 路径更新为 `services::fs::procfs_core`
- **TCB 收益**: -238 行

### 3.9 E6-9a: DevFS 硬编码消除 ✅

- **改动内容**:
  1. `DevfsData::mount()` 不再硬编码 5 个虚拟设备, 改为空操作 (仅重置计数)
  2. `devfs::init()` 调用 `register_standard()` 显式注册标准虚拟设备
  3. `DevKind` 枚举扩展: 新增 `Block`/`Char`/`Net`/`Input` 物理设备类型
  4. `DevKind` 新增 `is_virtual()`/`is_physical()` 方法
  5. `read()`/`write()` 新增物理设备类型分支 (暂返回 -1, 待 E6-9b 桥接)
  6. 测试更新: `test_devfs_mount` 改为 mount + `register_standard()`
- **TCB 收益**: 消除硬编码, 为 Chitin 桥接铺路

### 3.10 E6-9b: Chitin→DevFS 桥接 ✅

- **改动内容**:
  1. Chitin 注册表新增 `DEVICE_REGISTER_CALLBACK` 回调钩子
  2. 新增 `chitin_set_register_callback()` 公共 API (由 DevFS 订阅)
  3. 新增 `notify_last_registered()` 内部函数, 所有注册函数 (5 个) push 后自动通知
  4. DevFS 新增 `init_with_chitin_bridge()` — 注册标准设备 + 订阅回调
  5. DevFS 新增 `on_chitin_device_registered()` 回调 — 将 `ChitinProto` 映射为 `DevKind` 并自动创建设备节点
  6. `ChitinProto::Bus/Other` 不创建 DevFS 节点 (仅 Block/Char/Net/Input)
- **锁顺序**: callback → chitin_devices → devfs_devices (无死锁风险)
- **TCB 收益**: Chitin 驱动注册即自动出现在 /dev, 无需手动维护设备节点

### 3.11 E6-9c: DevFS 接入 VFS 分发 ✅

- **改动内容**:
  1. `DevfsData` 实现 `FileSystem` trait (open/close/read/write/stat/readdir 等)
  2. `FsType` 枚举新增 `DevFs` 变体, `from_name("devfs")` / `as_str()` 支持
  3. `vfs_mount_internal` 新增 `FsType::DevFs` 初始化分支和 trait object 分发
  4. DevFS 不支持的操作 (chmod/chown/mkdir/unlink/symlink 等) 返回 `NotSupported`/`PermissionDenied`
  5. `fs_stat` 返回字符设备模式 `0o20666`
- **TCB 收益**: DevFS 可通过 `vfs_mount("/dev", "devfs")` 挂载, 完全走 VFS trait 分发路径

---

## 四、待执行计划

### E6-3: dcache 策略提取 ✅

**复杂度**: 低 | **必要性**: 高 | **风险**: 极低

**已完成**: `framework/fs/vfs/dcache.rs` 已迁移到 `services/fs/dcache.rs`, framework 层仅 re-export 公共 API (15 行).

**TCB 收益**: -876 行

---

### E6-4: VFS 分发策略提取 (FileSystem trait) ✅

**已完成**: 详见 3.6 节.

#### 4.1 FileSystem trait 设计

```rust
/// 文件系统策略接口 — services 层实现, framework 层调用
pub trait FileSystem: Send + Sync {
    fn name(&self) -> &'static str;

    // 生命周期
    fn fs_init(&self) -> KernelResult<()>;
    fn fs_mount(&self, path: &str) -> KernelResult<()>;
    fn fs_unmount(&self, path: &str) -> KernelResult<()>;

    // 文件操作 — 统一用 path + fd 双模式
    fn fs_open(&self, rel_path: &str, flags: u32, pwm: u64) -> KernelResult<FsOpenResult>;
    fn fs_close(&self, fd: u32) -> KernelResult<()>;
    fn fs_read(&self, fd: u32, buf: &mut [u8]) -> KernelResult<usize>;
    fn fs_write(&self, fd: u32, buf: &[u8]) -> KernelResult<usize>;

    // 元数据
    fn fs_stat(&self, rel_path: &str, pwm: u64) -> KernelResult<VfsStat>;
    fn fs_chmod(&self, rel_path: &str, mode: u16, pwm: u64) -> KernelResult<()>;
    fn fs_chown(&self, rel_path: &str, owner: u64, group: u64, pwm: u64) -> KernelResult<()>;

    // 目录操作
    fn fs_mkdir(&self, rel_path: &str, pwm: u64) -> KernelResult<()>;
    fn fs_unlink(&self, rel_path: &str, pwm: u64) -> KernelResult<()>;
    fn fs_rmdir(&self, rel_path: &str, pwm: u64) -> KernelResult<()>;
    fn fs_rename(&self, old_path: &str, new_path: &str, pwm: u64) -> KernelResult<()>;
    fn fs_readdir(&self, fd: u32, entry: &mut VfsDirEntry) -> KernelResult<bool>;

    // 符号链接
    fn fs_symlink(&self, target: &str, link_path: &str, pwm: u64) -> KernelResult<()>;
    fn fs_readlink(&self, rel_path: &str, buf: &mut [u8]) -> KernelResult<usize>;
    fn fs_link(&self, old_path: &str, new_path: &str, pwm: u64) -> KernelResult<()>;
}

pub struct FsOpenResult {
    pub node_id: u32,   // RamFS 用; HvFS 可填 fd
    pub offset: u64,
    pub file_type: u8,
}
```

#### 4.2 前置工作

| 序号 | 内容 | 原因 |
|------|------|------|
| 1 | 统一 RamFS/HvFS 方法签名 | RamFS 用 `node_id`, HvFS 用 `path/fd`, 需统一为 trait 接口 |
| 2 | 封装 RamFS 内部字段访问 | `vfs_fchmod` 等直接访问 `ramfs.nodes[node_id]`, 需改为方法调用 |
| 3 | 抽象 `RamFsDirEntry` | api.rs 中 2 处直接使用 RamFS 类型, 需用 VFS 层类型替代 |
| 4 | 剥离 unsafe 用户指针操作 | api.rs 的 `UserReadPtr/UserWritePtr` 必须留在 framework, 策略部分移到 trait 实现 |

#### 4.3 VfsMount 改造

```rust
// 改造前
pub struct VfsMount {
    pub path: [u8; VFS_MAX_PATH],
    fs_type: FsType,           // 枚举, 硬编码
    pub used: bool,
}

// 改造后
pub struct VfsMount {
    pub path: [u8; VFS_MAX_PATH],
    fs: Option<&'static dyn FileSystem>,  // trait object
    pub used: bool,
}
```

#### 4.4 api.rs 分发改造示例

```rust
// 改造前
match fs_type {
    FsType::RamFs => {
        let mut ramfs = RAMFS_DATA.lock();
        ramfs.open(rel_path, flags, pwm)
    }
    FsType::HvFs => {
        let hvfs = get_hvfs();
        hvfs.open(rel_path, flags, pwm)
    }
    FsType::Unknown => -1,
}

// 改造后
let fs = VFS_MANAGER.get_filesystem(mount_idx);
match fs.fs_open(rel_path, flags, pwm) {
    Ok(result) => { /* 设置 fd */ }
    Err(e) => e.as_i32(),
}
```

**前置依赖**: E6-3 (dcache 迁移)
**TCB 收益**: 分发逻辑约 200-300 行可移除, 更重要的是为 E6-5 铺路

---

### E6-5: RamFS 迁移到 services ✅

**已完成**: 详见 3.7 节.

---

### E6-6: HvFS unsafe 消除 + 迁移到 services ✅

**复杂度**: 极高 | **必要性**: 中高 | **风险**: 中

#### 6.1 HvFS unsafe 分析

HvFS 共 10 处 unsafe, 分布在 5 个文件:

| 文件 | unsafe 数 | 用途 |
|------|----------|------|
| `hvfs.rs` | 1 | SAFETY 注释 (非实际 unsafe 块) |
| `arc.rs` | 2 | `from_raw_parts` (缓存切片) |
| `spa.rs` | 2 | `as_bytes()` 序列化 + `read_unaligned` 反序列化 |
| `zil_persist.rs` | 3 | `as_bytes()` 序列化 + 指针转换 |
| `bp.rs` | 2 | `as_bytes()` 序列化 + `copy_nonoverlapping` 反序列化 |

**共同模式**: 全部是 `repr(C)` 磁盘数据结构的序列化/反序列化.

#### 6.2 unsafe 消除方案与执行结果

**采用方案: zerocopy IntoBytes 编译期验证 + safe 反序列化**

1. **引入 `zerocopy` 依赖** (Cargo.toml, features = ["derive"])
2. **消除隐式 padding**: 重排结构体字段顺序, 确保所有 padding 为显式 `_pad` 字段:
   - `HvDva`: `vdev_id:u16, offset:u64` → `offset:u64, asize:u32, vdev_id:u16, gang:u8, _pad:[u8;1]` (消除 6 bytes 隐式 padding)
   - `HvBpProp`: `bool` → `u8`, 字段重排为大端对齐优先 (消除 3 bytes 隐式 padding)
   - `HvUberblock`: `magic:u32` 移至末尾 (消除 4 bytes 隐式 padding)
3. **derive `IntoBytes`**: 编译期验证结构体无隐式 padding
4. **`as_bytes()`**: 保留 `from_raw_parts` slice cast (由 `IntoBytes` derive 编译期保证安全), 添加 SAFETY 注释
5. **反序列化全部改为 safe**: 逐字段 `from_le_bytes` 读取, 消除所有 `copy_nonoverlapping`/`read_unaligned`/指针转换:
   - `HvDva::from_bytes()` — 5 个字段逐个读取
   - `HvBpProp::from_bytes()` — 8 个字段逐个读取
   - `HvBlockPointer::from_bytes()` — 递归调用子结构 from_bytes + 逐字段读取
   - `HvUberblock::from_bytes_unaligned()` — 递归调用子结构 from_bytes + 逐字段读取
   - `ZilBlockHeader::from_block()` — 12 个字段逐个读取, 返回值类型改为 `Option<Self>`

**消除结果**:
- 反序列化: 5 处 unsafe → 0 unsafe ✅
- 序列化: 4 处 `as_bytes` → 0 unsafe ✅ (derive `Immutable` + `zerocopy::IntoBytes::as_bytes()` safe 方法)
- ARC `lookup_slice`: 1 处 → 封装到 `arc_safe::ptr_to_slice()` ✅ (框架层 safe API, 内部 unsafe 对外不可见)
- **总计**: unsafe 从 10 处降至 1 处 (仅 `arc_safe.rs` 的 `from_raw_parts`, 框架层必要封装)

**新增文件**:
- `framework/fs/hvfs/arc_safe.rs` — ARC 缓存裸指针→切片的 safe 封装

**新增 derive**:
- `HvDva`/`HvBpProp`/`HvBlockPointer`/`HvUberblock`/`ZilBlockHeader`/`ZilBlockTrailer`: 增加 `Immutable` derive

#### 6.3 HvFS 迁移到 services (阶段 2: 已完成 ✅)

1. ~~将 `framework/fs/hvfs/` 整体 (18 文件, 6,154 行) 迁移到 `services/fs/hvfs/`~~ ✅
2. ~~实现 `FileSystem` trait~~ ✅ (已在 hvfs.rs 中实现)
3. ~~framework 层 re-export 公共类型~~ ✅
4. ~~双架构编译 + 审计验证~~ ✅ (x86_64 + aarch64 0 error, 审计全通过)

**迁移详情**:
- 17 个业务文件从 `framework/fs/hvfs/` 迁移到 `services/fs/hvfs/`
- framework 层仅保留 `arc_safe.rs` (unsafe 封装) 和 `mod.rs` (re-export)
- use 路径修改: `framework::fs::hvfs::*` → `services::fs::hvfs::*`
- 同步原语修改: `framework::sync::mutex::Mutex` → `services::sync::irq_lock::IrqSpinLock as Mutex`
- 同步原语修改: `framework::sync::once_lock::OnceLock` → `services::sync::once::OnceCell`
- 日志修改: `klog_info!` → `framework::klog::serial_write_bytes` (services 层 deny unsafe_code)
- `arc_safe::ptr_to_slice` 保持 framework 层引用 (唯一 unsafe 封装)
- 删除旧 `services/fs/hvfs.rs` 安全代理 (330 行, 迁移后不再需要)

**前置依赖**: E6-4 (FileSystem trait) ✅
**TCB 收益**: 阶段 1 消除 9 处 unsafe (10→1); 阶段 2 迁移后 -6,154 行 TCB

---

### E6-7: DevFS 迁移到 services ✅

**已完成**: 详见 3.5 节 + 3.9~3.11 节 (含硬编码消除、Chitin 桥接、VFS 分发接入).

---

### E6-8: ProcFS 迁移到 services ✅

**已完成**: 详见 3.8 节.

---

### E6-9: Chitin↔DevFS 联合与硬编码消除 ✅

**已完成**: 详见 3.9~3.11 节.

---

### InitRamFS: 不迁移

**结论**: InitRamFS 保留在 framework, 不纳入迁移计划.

理由:
1. `unpack` 是启动时一次性 cpio 解析器, 不是运行时文件系统
2. 其唯一 unsafe (`from_raw_parts`) 是对 bootloader 原始指针的安全封装, 属于机制
3. 它是 VFS 的**调用者** (通过 `vfs_mkdir`/`vfs_open`/`vfs_write` 写入 RamFS), 不是 VFS 的被分发对象
4. 迁移到 services 后仍需调用 framework 的 VFS API, 无法消除耦合, 反而增加跨层调用复杂度

---

### E6-9: Chitin↔DevFS 联合与硬编码消除

**复杂度**: 中 | **必要性**: 高 | **风险**: 低

#### 9.1 现状问题

**DevFS 硬编码**: `DevfsData::mount()` 中 5 个虚拟设备 (null/zero/console/tty/credo) 被硬编码写入设备表，而非通过注册机制动态添加。`services/fs/devfs.rs` 的 `register_standard()` 也硬编码了同样的 4 个设备。

**Chitin↔DevFS 断裂**: Chitin 的设计文档明确指出 DevFS 应通过 `chitin_register_driver` 暴露设备节点，但实际代码中两者零关联：
- Chitin 注册了真实硬件设备 (ATA 块设备、串口字符设备、e1000/virtio 网卡、键盘)，但不在 DevFS 创建节点
- DevFS 只有 5 个虚拟设备，用户态无法通过 `/dev/sda` 等路径访问真实硬件
- HvFS 直接调用 `chitin_blk_read/write` 绕过 DevFS

**后果**:
1. 用户态无法 `open("/dev/sda")` 访问块设备 — 设备发现路径断裂
2. 每新增一个虚拟设备都需修改 `mount()` 硬编码 — 违反开放封闭原则
3. Chitin 设备注册表与 DevFS 设备表是两套独立体系 — 信息冗余、不一致风险

#### 9.2 目标架构

```
Chitin 注册表 (CHITIN_DEVICES)          DevFS 设备表
┌─────────────────────┐               ┌──────────────────┐
│ ata0  (Block, Ready) │─── 桥接 ───→ │ /dev/sda         │
│ serial0 (Char, Ready)│─── 桥接 ───→ │ /dev/serial0     │
│ e1000  (Net, Ready)  │─── 桥接 ───→ │ /dev/eth0        │
│ kbd    (Input, Ready)│─── 桥接 ───→ │ /dev/input/kbd   │
└─────────────────────┘               ├──────────────────┤
                                      │ null   (虚拟)     │
                                      │ zero   (虚拟)     │
                                      │ console (虚拟)    │
                                      │ tty    (虚拟)     │
                                      │ credo  (虚拟)     │
                                      └──────────────────┘
```

#### 9.3 执行步骤

**阶段一: 去除硬编码 (E6-9a)**

1. 重构 `DevfsData::mount()`: 不再硬编码 5 个设备，改为调用 `register_device()` 注册
2. 在内核启动流程中显式调用 `devfs::register_standard()` 注册虚拟设备
3. 虚拟设备类型扩展: 将 `dev_type: u8` 改为枚举 `DevKind` (services 层已定义), 增加虚拟/物理标记
4. 双架构编译 + 测试验证

**阶段二: Chitin→DevFS 桥接 (E6-9b)**

1. 在 Chitin 注册函数 (`chitin_register`/`chitin_register_with_ops`/`chitin_register_block`) 中增加回调钩子:
   ```rust
   /// 设备注册回调 — 可由 DevFS 订阅, 自动创建设备节点
   static DEVICE_REGISTER_CALLBACK: Mutex<Option<fn(&ChitinDevice)>> = Mutex::new(None);

   pub fn chitin_set_register_callback(cb: fn(&ChitinDevice)) {
       *DEVICE_REGISTER_CALLBACK.lock() = Some(cb);
   }
   ```
2. 在 DevFS 初始化时订阅回调:
   ```rust
   pub fn init_with_chitin_bridge() {
       init_global();
       chitin::chitin_set_register_callback(on_chitin_device_registered);
   }

   fn on_chitin_device_registered(dev: &ChitinDevice) {
       let name = format!("/dev/{}", dev.name);
       let kind = match dev.proto {
           ChitinProto::Block => DevKind::Block,
           ChitinProto::Char => DevKind::Char,
           ChitinProto::Net => DevKind::Net,
           ChitinProto::Input => DevKind::Input,
           _ => return,
       };
       let _ = global().register(&dev.name, kind);
   }
   ```
3. DevFS 的 `read`/`write` 路由: 对物理设备类型, 转发到 Chitin 对应协议的 I/O 函数
4. 设备命名策略: Chitin 设备名 (如 `ata0`) → DevFS 路径 (如 `/dev/sda`), 需要命名映射表 (可在 services 层配置)

**阶段三: DevFS 接入 VFS 分发 (E6-9c, 可选)**

1. 为 DevFS 实现 `FileSystem` trait (或 `DeviceFileSystem` 子 trait)
2. `vfs_open("/dev/sda")` → VFS 查找 mount 点 → DevFS trait dispatch → `devfs_open("sda")` → Chitin 块设备 I/O
3. 此阶段依赖 E6-4 (FileSystem trait), 优先级低于阶段一、二

#### 9.4 DevKind 扩展

```rust
// 改造前 (services/fs/devfs.rs)
pub enum DevKind {
    Null = 1, Zero = 2, Console = 3, Tty = 4, Credo = 5,
}

// 改造后
pub enum DevKind {
    // 虚拟设备
    Null = 1, Zero = 2, Console = 3, Tty = 4, Credo = 5,
    // 物理设备 (来自 Chitin)
    Block = 10, Char = 11, Net = 12, Input = 13,
}
```

#### 9.5 安全考量

- 回调注册 (`chitin_set_register_callback`) 必须在启动早期单线程上下文调用, 与 Chitin 现有 `chitin_register_*` 约束一致
- DevFS `read`/`write` 转发到 Chitin 时, 需确保 buffer 生命周期安全 — Chitin 的 `BlockOps::read_sector`/`write_sector` 已提供 safe 封装
- 命名映射表在 services 层, 不影响 framework 安全边界

#### 9.6 依赖关系

```
E6-7 DevFS 迁移到 services
     │
     ▼
E6-9a 去除硬编码
     │
     ▼
E6-9b Chitin→DevFS 桥接
     │
     ├──────────────┐
     ▼              ▼
E6-9c 接入 VFS   E6-4 FileSystem trait
(可选, 低优先级)
```

**前置依赖**: E6-7 (DevFS 迁移到 services)
**TCB 收益**: 间接 — 消除硬编码后 DevFS 更易维护, Chitin 桥接使设备发现路径完整

---

## 五、执行路径与依赖关系

```
E6-1 flock 迁移      ✅ 已完成
E6-2 inotify 迁移    ✅ 已完成
     │
     ├──────────────────────────────┐
     ▼                              ▼
E6-3 dcache 迁移              E6-7 DevFS 迁移 (可并行)
     │                              │
     ▼                              ▼
E6-4 FileSystem trait        E6-8 ProcFS 迁移 (可并行)
     │                              │
     ├──────────────────┐           ▼
     ▼                  ▼      E6-9a 去除硬编码
E6-5 RamFS 迁移     E6-6 HvFS unsafe 消除   │
                         │              ▼
                         ▼         E6-9b Chitin↔DevFS 桥接
                    E6-6b HvFS 迁移       │
                                          ▼
                                     E6-9c DevFS 接入 VFS (可选)

InitRamFS: 不迁移 (保留 framework, 是 VFS 调用者而非被分发对象)
```

---

## 六、总体 TCB 收益预估

| 阶段 | 迁移内容 | 行数 | 累计 TCB 缩减 |
|------|---------|------|-------------|
| E6-1 | flock | 700 | 700 |
| E6-2 | inotify | 540 | 1,240 |
| E6-3 | dcache | 876 | 2,116 |
| E6-7 | DevFS | 295 | 2,411 |
| E6-8 | ProcFS | 245 | 2,656 |
| E6-5 | RamFS | 1,639 | 4,295 |
| E6-6 | HvFS | 6,154 | 10,449 |

**最终 framework/fs 仅保留**:
- `vfs/api.rs` — 系统调用边界 (unsafe 用户指针操作) + trait dispatch
- `vfs/vfs.rs` — VfsManager (mount/fd 管理, 机制)
- `vfs/types.rs` — 公共类型
- `initramfs.rs` — cpio 解析器 (VFS 调用者, 属于机制)
- `driver/block.rs` — 块设备 safe API

---

## 七、新增文件系统流程 (trait 化后)

1. **在 services 层实现 `FileSystem` trait** (如 `services::fs::tmpfs::TmpFs`)
2. **在 framework 层注册**: `VFS_MANAGER.mount(path, "tmpfs")` 时通过名称查找并存储 `&dyn FileSystem`
3. **api.rs 无需修改**: 所有分发自动走 trait object

### 对比

| 方面 | 当前 (match 分发) | trait 化后 |
|------|-------------------|-----------|
| 新增文件系统 | 改 api.rs 14+ 处 | 实现 trait, 0 处改 api.rs |
| 编译隔离 | 改 api.rs 全量重编 | 新 fs 独立编译 |
| TCB | RamFS/HvFS 逻辑在 framework | 策略在 services, framework 仅机制 |
| 测试 | 只能集成测试 | 可 mock trait 单元测试 |

---

## 八、HvFS 迁移可行性结论

**HvFS 可以迁移到 services**, 前提是完成 unsafe 消除 (E6-6).

关键论据:
1. HvFS 对 framework 的所有外部依赖 (block driver, credo, sync) 都是 safe API, services 可调用
2. HvFS 的 10 处 unsafe 全部是固定模式的 `repr(C)` 序列化, 可用 `zerocopy` 完全消除
3. 消除后 HvFS 整体 0 unsafe, 符合 services 层 `#![deny(unsafe_code)]` 要求
4. HvFS 内部磁盘 IO 通过 `block::hdd_*` safe API, 符合 framekernel "机制在 framework, 策略在 services" 原则

---

## 九、风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| FileSystem trait 接口设计不当, 后续需频繁修改 | 中 | 高 | 先用 RamFS/HvFS 两个实现验证 trait 完备性, 再固化 |
| zerocopy 派生宏与现有 `repr(C)` 结构体不兼容 | 低 | 中 | 逐个文件改造, 编译验证; 不兼容处手写 safe 封装 |
| RamFS/HvFS 迁移后性能退步 (trait object 虚调用) | 低 | 低 | VFS 路径本身有锁开销, 虚调用可忽略; 必要时用 enum dispatch 替代 |
| dcache 迁移后 RamFS 编译路径变化 | 低 | 低 | re-export 保持路径不变, RamFS 迁移后自然消除 |

---

## 十、变更历史

| 日期 | 内容 |
|------|------|
| 2026-06-10 | 初始版本: E6-1/E6-2 已完成, E6-3~E6-6 计划制定 |
| 2026-06-10 | 追加 DevFS/ProcFS/InitRamFS 分析: E6-7 DevFS 迁移, E6-8 ProcFS 迁移, InitRamFS 不迁移; 更新依赖图与 TCB 收益 (总计 10,449 行) |
| 2026-06-10 | 追加 E6-9: Chitin↔DevFS 联合与硬编码消除 (三阶段: 去硬编码→桥接→接入 VFS) |
| 2026-06-10 | E6-6 阶段 1 完成: HvFS 序列化/反序列化 unsafe 消除 (5 处→0), zerocopy IntoBytes 编译期验证, 字段重排消除隐式 padding; 更新所有已完成任务标记 |
| 2026-06-11 | E6-6 阶段 2 完成: HvFS 17 个文件从 framework 迁移到 services; framework 仅保留 arc_safe.rs + re-export; 同步原语迁移到 services 层; klog_info→serial_write_bytes; 双架构编译+审计通过 |
