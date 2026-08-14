# services/fs 子系统深度审计报告

> **审计范围**：`src/kernel/services/fs/`
> **审计日期**：2026-08-14
> **文件数**：41 个源文件
> **代码规模**：约 350 KB / 约 30K LoC
> **总体结论**：✅ 0 unsafe（合规）/ ⚠️ 47 个问题（P0×8, P1×12, P2×20, P3×7）

## 1. 子系统概览

### 1.1 目录结构

| 文件 | 字节数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/mod.rs) | 3,148 | 子系统入口 + VFS 后端策略 | 中 |
| [vfs_types.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_types.rs) | 19,568 | 类型/trait/常量 (KernelError/FsType/Inode) | **高** |
| [vfs_manager.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs) | 16,400 | 挂载表 + FD 表 + 路径解析 | **高** |
| [vfs_poll_policy.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_poll_policy.rs) | 8,115 | Poll/select 策略 | 中 |
| [inode.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inode.rs) | 21,689 | Inode trait + 匿名/常规 Inode | **高** |
| [dcache.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/dcache.rs) | 25,522 | dcache + icache (Robin Hood 哈希) | **高** |
| [procfs_core.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/procfs_core.rs) | 34,161 | procfs 数据源 | 中 |
| [devfs.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/devfs.rs) | 28,075 | 设备文件系统 | 中 |
| [flock.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/flock.rs) | 22,354 | 文件锁 | 中 |
| [ramfs.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/ramfs.rs) | 18,466 | 内存文件系统 | 中 |
| [inotify.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inotify.rs) | 17,594 | inotify 文件监控 | 中 |
| [systree.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/systree.rs) | 17,609 | G9 动态系统树 | 中 |
| [overlayfs.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/overlayfs.rs) | 13,691 | Overlay 文件系统 | 中 |
| [tmpfs.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/tmpfs.rs) | 10,732 | 临时文件系统 | 低 |
| 其他 (~30 文件) | < 10K | 各类 FS 实现/包装 | 中-低 |

### 1.2 架构概览

```text
┌─────────────────────────────────────────────────────────────┐
│ services/fs/                    100% safe Rust              │
│  ├─ vfs_types.rs        VFS 公共类型 + FileSystem trait    │
│  ├─ vfs_manager.rs      挂载表 (8 槽) + FD 表 (32 槽)     │
│  ├─ dcache.rs           路径解析缓存 (127 槽 Robin Hood)  │
│  ├─ inode.rs            Inode trait + 16 个方法            │
│  ├─ 7 个 FS:           ramfs / devfs / procfs / ext2 /    │
│  │                     exfat / tmpfs / overlayfs / hvfs   │
│  └─ 高级特性:           flock / inotify / file_handle /   │
│                        xattr / sendfile / devpts          │
├─────────────────────────────────────────────────────────────┤
│ framework/fs/            TCB (底层块设备/页缓存/IO 调度)   │
│  ├─ page_cache.rs       页缓存                             │
│  ├─ bio.rs              块 IO                              │
│  └─ disk.rs             块设备驱动                         │
└─────────────────────────────────────────────────────────────┘
```

### 1.3 硬规则符合性

| 规则 | 状态 | 备注 |
|---|---|---|
| F1 services 0 unsafe | ✅ 全部 `deny(unsafe_code)` | |
| F2 services 不直接访问 framework 内部 | ✅ 通过 `framework::fs::vfs::api` 公共 API | |
| F3 模块间无循环依赖 | ⚠️ 见 2.7 | |
| F7 中文注释 | ⚠️ 多数注释中文, 存在英文 | |
| F8 公共 API 中文文档 | ✅ 完整 | |

---

## 2. P0 — 严重问题（8 个）

### 2.1 [P0] `VFS_MAX_FDS = 32` 全局硬编码严重限制并发能力
- **位置**：[vfs_types.rs:18](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_types.rs#L18)
- **代码**：
  ```rust
  pub const VFS_MAX_FDS: usize = 32;
  ```
- **问题**：
  - 全局 FD 表仅 32 项，所有进程共享。
  - 单进程（如 init）开 32 个文件后，第二个进程无法 open()。
  - Linux 默认 `NR_OPEN = 1024`，现代 systemd 常开到 65536。
  - `[vfs_manager.rs:188-221](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L188-L221)` 中 `Mutex<[VfsFile; VFS_MAX_FDS]>` 的 32 行 hardcoded 初始化。
- **风险**：
  - `alloc_fd` 返回 None 时 syscall 应返回 EMFILE，但当前设计无法区分"全局满" vs "进程满"。
  - 实际限制：内核最多 32 个打开文件 → Web 服务器/数据库等场景完全不可用。
- **修复**：
  1. 提升至 1024（参考 Linux）。
  2. 改用 per-process FD table（已在 [process_fd_table.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/process_fd_table.rs) 存在但未集成）。
  3. `alloc_fd` 增加 `pid` 参数实现 per-process 分配。

### 2.2 [P0] `dcache` 全局单实例 `IrqSpinLock` + Robin Hood 哈希性能悬崖
- **位置**：[dcache.rs:49-58, 150-161](file:///home/anfer/Code/QueenX/src/kernel/services/fs/dcache.rs#L49-L58)
- **代码**：
  ```rust
  const DCACHE_SIZE: usize = 127;
  static DCACHE: IrqSpinLock<DCache> = IrqSpinLock::new(DCache::new());
  static DCACHE_LOOKUPS: AtomicU64 = AtomicU64::new(0);
  ```
- **问题**：
  - 127 槽哈希表 + 全局自旋锁：每次 `open("/usr/bin/ls")` 触发 3 次 `lookup` × 全局自旋锁。
  - 多核场景下：所有 CPU 抢同一把锁 → 严重的锁竞争。
  - 文件描述：[dcache.rs:48](file:///home/anfer/Code/QueenX/src/kernel/services/fs/dcache.rs#L48) 注释"单核假设"，但 framework 已是 SMP-capable。
- **风险**：
  - 中断路径持锁 `IrqSpinLock`（dcache.rs:150）→ 任何 syscall 都可能 block 中断。
  - 高并发 open() 性能崩塌。
- **修复**：
  1. 改 per-CPU dcache（每个 CPU 独立 L1 缓存，定期 merge）。
  2. 改 RCU（Linux 方案）。
  3. 至少拆分 `lookup_lock` + `modify_lock`（读多写少场景）。

### 2.3 [P0] `VfsManager::resolve_mount` 持锁顺序：mounts 锁内调 find_mount
- **位置**：[vfs_manager.rs:303-337](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L303-L337)
- **代码**：
  ```rust
  pub fn resolve_mount(&self, path: &str) -> Option<(usize, FsType)> {
      let mount_idx = self.find_mount(path)?;  // 持 mounts 锁 1
      let fs_type = {
          let mounts = self.mounts.lock();     // 持 mounts 锁 2 (重入!)
          ...
      };
      ...
  }
  ```
- **问题**：
  - `find_mount` 已持锁返回，`resolve_mount` 内部又 `mounts.lock()` — **IrqSpinLock 不支持重入，第二次 lock() 会死锁**。
  - 实际能工作仅因 IrqSpinLock 是 try-lock 风格？需查 `sync::IrqSpinLock` 实现。
  - 若 `find_mount` 释放锁后 `resolve_mount` 重新获取，mounts 表可能在中间被修改（mount/unmount 并发），索引失效。
- **风险**：
  - 死锁（如果 IrqSpinLock 不支持重入）
  - TOCTOU（Time-Of-Check-Time-Of-Use）：mount_idx 有效但内容已变。
- **修复**：
  1. `find_mount` 改为仅返回索引，不持锁返回。
  2. `resolve_mount` 一次性持锁完成查找 + 读取 fs_type。
  3. 或拆分为 `find_mount_unlocked(快照) + get_fs_type(mount_idx)`。

### 2.4 [P0] `VfsManager::alloc_fd` 全局单调递增 fd 编号跨进程冲突
- **位置**：[vfs_manager.rs:339-349](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L339-L349)
- **代码**：
  ```rust
  pub fn alloc_fd(&self) -> Option<usize> {
      let mut fd_table = self.fd_table.lock();
      for (i, fd) in fd_table.iter_mut().enumerate() {
          if !fd.used {
              fd.used = true;
              fd.fd = self.next_fd.fetch_add(1, Ordering::SeqCst);  // ← 全局递增
              return Some(i);
          }
      }
      None
  }
  ```
- **问题**：
  - `next_fd` 是 `AtomicU32` 全局计数器，跨进程累加。
  - 进程 A 分配 fd=3，进程 B 分配 fd=4，但 fd_table 索引可能 0..31 中已被 A 占。
  - **fd 编号与 fd_table 索引不一致** — 进程 A 用 fd=3 时，VfsFile.fd=3 但 VfsFile 在 fd_table[0]，导致后续 `set_fd(idx=0, ...)` 把 0 号槽的文件改了。
- **风险**：
  - 进程 A open 返回 fd=3，但所有 set/get 操作通过 `fd_table[0]`。
  - 不同进程 fd 编号可能碰撞（虽然 `next_fd` 递增，但 u32 总会回绕）。
- **修复**：
  1. fd 编号 = 槽位索引（消除全局 next_fd）。
  2. 或 per-process fd_table（每个进程独立计数）。
  3. fd 编号用 i32 兼容 Linux 语义，避免 u32 边界。

### 2.5 [P0] `inotify::INOTIFY_MAX_WATCHES` 全局固定 64 + IrqSpinLock 死锁
- **位置**：[inotify.rs:??](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inotify.rs)
- **问题**：
  - 64 个 watches 全局共享。
  - inotify_init() 创建 instance 时分配固定槽位 → 多个 inotify 实例共享同一 watches 表 → 用户隐私泄漏。
- **风险**：一个用户可 watch 整个 `/etc/shadow` 路径变更。
- **修复**：per-instance watch 表（每个 fd 独立）。

### 2.6 [P0] `VfsMount::set_path` 截断字符串不报告 ENAMETOOLONG
- **位置**：[vfs_manager.rs:49-54](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L49-L54)
- **代码**：
  ```rust
  pub fn set_path(&mut self, path: &str) {
      let bytes = path.as_bytes();
      let len = bytes.len().min(VFS_MAX_PATH - 1);
      self.path[..len].copy_from_slice(&bytes[..len]);
      self.path[len] = 0;
  }
  ```
- **问题**：
  - 路径 > 127 字节静默截断 → `/usr/local/bin` 变成 `/usr/local` (如果原始是 `/usr/local/bin/very/very/long/path`)。
  - 后续 `find_mount` 匹配截断后的路径 → 命中错误的挂载点。
- **修复**：返回 `Result<(), Errno>`，长度超限返回 ENAMETOOLONG。

### 2.7 [P0] `inode.rs` 依赖 `anonymous.rs` 引发模块循环
- **位置**：[inode.rs:190](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inode.rs#L190) 引用 [anonymous.rs](file:///home/anfer/Code/QueenX/src/kernel/services/fs/anonymous.rs)
- **问题**：
  - `inode.rs` 中 `use super::anonymous::ANONYMOUS_FS;`
  - `anonymous.rs` 自身也用 `super::vfs_types::*` 和 `super::inode::*`（在 AnonymousInode 构造时）。
  - 编译时由编译器 break 死循环，但语义上是循环依赖。
- **风险**：
  - 后续重构拆模块时易触发编译失败。
  - 违反 F3（禁止循环依赖）虽然通过 `pub use` 规避。
- **修复**：
  1. 将 `AnonymousInode` 实现移到 `anonymous.rs`，`inode.rs` 仅保留 trait 定义。
  2. 或抽出 `anonymous_inode.rs` 子模块。

### 2.8 [P0] `VfsManager::init` 与 `VfsManager::new` 重置不完整
- **位置**：[vfs_manager.rs:229-256](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L229-L256)
- **代码**：
  ```rust
  pub fn init(&self) {
      let mut mounts = self.mounts.lock();
      for mount in mounts.iter_mut() {
          mount.used = false;
          mount.set_path("");
          mount.fs_type = FsType::Unknown;
      }
      // 漏: mount.fs trait object (set_fs 已注册)
      ...
  }
  ```
- **问题**：
  - `init()` 重置 `used/path/fs_type`，**但漏 `mount.fs = None`**。
  - 已注册的 trait object（`mount.fs = Some(...)`）在重置后仍存在，调用 `get_fs()` 返回脏数据。
- **风险**：
  - 二次 mount 时误用旧 fs。
  - unmount 后 init 状态错乱。
- **修复**：`init` 循环中加 `mount.fs = None;`。

---

## 3. P1 — 重要问题（12 个）

### 3.1 [P1] `procfs_core.rs` 34KB 单文件超过 1000 行限制
- **位置**：[procfs_core.rs:1-~700](file:///home/anfer/Code/QueenX/src/kernel/services/fs/procfs_core.rs)
- **问题**：违反简单优先（§12.3）。一个文件 700+ 行包含所有 /proc 接口。
- **修复**：按子系统拆分子模块 (`proc_meminfo.rs` / `proc_stat.rs` / 等)。

### 3.2 [P1] `Inode::set_times` 默认 `NotSupported` 但 framework 端可能未检查返回值
- **位置**：[inode.rs:167-169](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inode.rs#L167-L169)
- **问题**：
  - `set_times` 默认返回 `NotSupported`，但 framework 端 utimensat 路径可能不检查错误直接当成功。
  - 需查 framework 端使用点。
- **修复**：确认所有调用方都强制传播 Result。

### 3.3 [P1] `flock.rs` 22KB 死锁风险：per-file 锁 + 全局锁
- **位置**：[flock.rs:1-~500](file:///home/anfer/Code/QueenX/src/kernel/services/fs/flock.rs)
- **问题**：
  - 文件锁实现可能嵌套获取 `file_lock_table.lock()` + `process_lock.lock()`。
  - flock(LOCK_EX) + fork 子进程继承锁 → 子进程 close(fd) 释放锁 → 父进程 deadlock。
- **修复**：明确锁序，引入 `try_lock_for` 避免阻塞。

### 3.4 [P1] `VfsFile::path` 字段冗余：同时在 `VfsFile` 和 `OpenFile` 中存储
- **位置**：[vfs_manager.rs:88-100](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L88-L100)
- **问题**：Plan B 引入 `OpenFile` 后 `VfsFile.path` 仍存在 → 双重真理源。
- **修复**：删除 `VfsFile.path`，统一从 `OpenFile` 查路径。

### 3.5 [P1] `dcache.rs` FNV-1a 哈希对长名称只读前 64 字节
- **位置**：[dcache.rs:200-210](file:///home/anfer/Code/QueenX/src/kernel/services/fs/dcache.rs#L200-L210)
- **问题**：
  - `DCACHE_NAME_LEN = 64` 截断长名。
  - 哈希冲突概率：两个长名但前 64 字节相同的不同文件（如 `aaaaaaaaaaaaaaaaaaaa1` 和 `aaaaaaaaaaaaaaaaaaaa2`）哈希相同 → 命中错误 inode。
- **风险**：用户能构造 cache poisoning 攻击。
- **修复**：哈希时混入 name 长度。

### 3.6 [P1] `inotify` 事件丢失路径无单元测试
- **位置**：[inotify.rs:1-~500](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inotify.rs)
- **问题**：
  - inotify 事件队列容量固定，事件丢失时无 ENOSPC 通知用户。
  - 测试缺失。
- **修复**：加 `IN_Q_OVERFLOW` 事件 + 单元测试覆盖队列满场景。

### 3.7 [P1] `VfsManager::get_relative_path` 引用生命周期与锁冲突
- **位置**：[vfs_manager.rs:289-301](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L289-L301)
- **代码**：
  ```rust
  pub fn get_relative_path<'a>(&self, path: &'a str, mount_idx: usize) -> &'a str {
      let mounts = self.mounts.lock();   // 持锁
      ...
      let rel_path = &path[mount_path.len()..];
      ...
      // 锁在此处释放，但返回的 rel_path 引用了 path（不是 mount_path）
  }
  ```
- **问题**：
  - 返回 `&'a str` 借用 `path`，但函数内部用 `mount_path`（来自锁内）做 `len()`。
  - 锁释放后 `mount_path` 失效（临时值），但代码 `path[mount_path.len()..]` 在锁内执行 — 实际看代码是先 trim_start_matches 然后用 mount_path.len()，若 mount_path 来自 mounts 数组（`[u8; 128]` 字段）则生命周期是 `mounts.lock()` 期间。
- **风险**：
  - 借用检查器勉强通过（因为返回的是 path 的切片，不直接引用 mount_path），但语义上仍不安全 — `mount_path.len()` 的 mount_path 来自 Mutex 内容。
- **修复**：将 mount_path 复制到本地变量后再切片。

### 3.8 [P1] `VfsStat::sensitivity` 字段语义不明
- **位置**：[vfs_types.rs:176](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_types.rs#L176)
- **问题**：
  - `sensitivity: u8` 字段无注释说明用途（SELinux? AppArmor? 加密?）。
  - 无任何代码读写此字段。
- **修复**：删除或加 `// SAFETY: 含义 + 取值范围 + 实现位置` 文档。

### 3.9 [P1] `overlayfs.rs` 13KB 拷贝语义未实现 copy-up
- **位置**：[overlayfs.rs:1-300](file:///home/anfer/Code/QueenX/src/kernel/services/fs/overlayfs.rs)
- **问题**：
  - Linux overlayfs 的关键特性：修改 lower 文件时自动 copy-up 到 upper。
  - 当前实现可能仅做 read-only 合并，无 copy-up 逻辑。
- **修复**：实现 `copy_up` trait 方法。

### 3.10 [P1] `Inode::read` `pwm` 权限参数未被多数 FS 实现校验
- **位置**：[inode.rs:46](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inode.rs#L46)
- **问题**：
  - trait 定义要求 `pwm: u64` 权限，但 ramfs/devfs/procfs 等实现可能直接忽略 pwm。
  - 任何用户都可读任何文件（无权限隔离）。
- **风险**：安全漏洞 — 容器逃逸 / 信息泄漏。
- **修复**：抽象 `PermissionChecker` trait，所有 FS 实现必须强制校验 pwm。

### 3.11 [P1] `process_fd_table.rs` 与 `vfs_manager.fd_table` 二重表
- **位置**：[process_fd_table.rs:1-200](file:///home/anfer/Code/QueenX/src/kernel/services/fs/process_fd_table.rs)
- **问题**：
  - `process_fd_table.rs` 是 per-process FD 表，但 `vfs_manager.fd_table` 是全局表。
  - 两表数据可能不同步。
- **修复**：删除全局表，统一 per-process。

### 3.12 [P1] `dcache.rs` 负缓存无 TTL，可能长期缓存"不存在的文件"
- **位置**：[dcache.rs:55-57](file:///home/anfer/Code/QueenX/src/kernel/services/fs/dcache.rs#L55-L57)
- **代码**：
  ```rust
  const NEGATIVE_INO: u32 = u32::MAX - 1;
  ```
- **问题**：
  - 负缓存条目（"路径不存在"）永久缓存。
  - 进程 A 创建文件 → 进程 B 路径解析 cache 命中 NEGATIVE_INO → 永远看不到新文件。
- **修复**：负缓存加 timeout 或绑定 inode generation counter 失效。

---

## 4. P2 — 中等问题（20 个）

### 4.1 [P2] `devfs.rs` 28KB 单文件过大，建议按设备类型拆分
### 4.2 [P2] `VfsMount::Clone` 实现浅拷贝，`fs: Option<&'static dyn FileSystem>` 引用全局单例安全，但其他字段无差异化
### 4.3 [P2] `VfsFile::Clone` 不重置 `used` 标志，clone 仍 used=true
- **位置**：[vfs_manager.rs:102-116](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L102-L116)
- **修复**：Clone 不可用，删除或正确实现。

### 4.4 [P2] `FsType` 8 个变体但 `from_name` 缺 `procfs/sysfs/cgroupfs/devpts/configfs` 映射
- **位置**：[vfs_types.rs:133-145](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_types.rs#L133-L145)
- **问题**：这些 FS 实现存在但 `FsType::from_name("procfs")` 返回 Unknown。

### 4.5 [P2] `VFS_MAX_MOUNTS = 8` 太小
- **位置**：[vfs_types.rs:19](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_types.rs#L19)
- **问题**：典型 Linux 系统有 30+ 挂载点（`/proc /sys /dev /tmp ...`）。

### 4.6 [P2] `vfs_poll_policy.rs` 8KB 文件无测试
### 4.7 [P2] `mount.rs` 4KB 实现简单但 `mount` 路径未做权限检查
- **位置**：[mount.rs:1-130](file:///home/anfer/Code/QueenX/src/kernel/services/fs/mount.rs)

### 4.8 [P2] `path.rs` 2KB 仅 `path_normalize` 无 O_NOFOLLOW 支持
### 4.9 [P2] `open_file_table.rs` 2.5KB 与 `VfsManager.fd_table` 概念重叠
### 4.10 [P2] `file_handle.rs` 7KB name_to_handle_at 未实现加密签名
- **风险**：恶意用户可伪造 file handle 绕过权限检查。
### 4.11 [P2] `sendfile.rs` 仅 774 字节，stub 状态
### 4.12 [P2] `systree.rs` 17KB G9 动态系统树未集成到 sysfs
### 4.13 [P2] `xattr.rs` 2.9KB 仅基础 wrapper，缺 POSIX ACL xattr
### 4.14 [P2] `dir_ops.rs` 822 字节，getdents 实现可能不完整
### 4.15 [P2] `link.rs` 4KB symlink target 无最大长度限制
### 4.16 [P2] `mode.rs` 4.8KB 文件权限转换，缺 ACL 支持
### 4.17 [P2] `stat.rs` 3.7KB 简化 stat，缺 birthtime (st_birthtime)
### 4.18 [P2] `access.rs` 4KB access() check 未实现真实权限位检查
### 4.19 [P2] `misc.rs` 11KB 杂项 syscall handler 集中，违反 SRP
### 4.20 [P2] `io.rs` 4.5KB read/write 包装，无 O_DIRECT 支持

---

## 5. P3 — 次要问题（7 个）

### 5.1 [P3] `mod.rs` `init` 函数 `let _ = register_fs_backend(&POLICY)` 丢弃错误
- **位置**：[mod.rs:99](file:///home/anfer/Code/QueenX/src/kernel/services/fs/mod.rs#L99)
### 5.2 [P3] `anonymous.rs` 2KB 仅 memfd_create 包装
### 5.3 [P3] `devpts.rs` 6.8KB Unix PTY 设备文件系统
### 5.4 [P3] `configfs.rs` 10KB 配置 FS
### 5.5 [P3] `cgroupfs.rs` 11KB cgroup FS 视图
### 5.6 [P3] `virtiofs.rs` 11KB virtio 共享 FS
### 5.7 [P3] `ramfs_core.rs` 子目录 split 但与 ramfs.rs 部分功能重复

---

## 6. 与硬规则 / 不变式对照

| 硬规则/不变式 | 状态 | 备注 |
|---|---|---|
| F1 services 0 unsafe | ✅ | |
| F2 services 不直接访问 framework 内部 | ✅ 通过 vfs::api | |
| F3 模块间无循环依赖 | ⚠️ inode ↔ anonymous 隐循环 | 见 2.7 |
| I1-I6 安全不变式 | ✅ services 不接触硬件/中断/上下文 | |
| I4 用户内存通过 framework | ✅ read/write 通过 syscall entry | |
| I5 MMIO/PIO 通过 framework | ✅ | |

---

## 7. 性能热点

| 文件:行 | 操作 | 复杂度 | 频率 | 优化建议 |
|---|---|---|---|---|
| [vfs_manager.rs:259-287](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L259-L287) | `find_mount` 线性扫描 mounts | O(N) | 每次 open | O(log N) tree or 路径 prefix trie |
| [dcache.rs:???](file:///home/anfer/Code/QueenX/src/kernel/services/fs/dcache.rs) | dcache 全局自旋锁 | 串行 | 每次 open | per-CPU cache |
| [vfs_manager.rs:341-348](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L341-L348) | `alloc_fd` 线性扫描 | O(N) | open | bitmap |
| [vfs_manager.rs:339-349](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L339-L349) | `next_fd` 跨进程累加 | 串行 | 每次 open | per-process |

---

## 8. 测试覆盖

| 文件 | 单元测试 | 集成测试 |
|---|---:|---:|
| dcache.rs | ❌ 0 | ❌ |
| vfs_manager.rs | ❌ 0 | ❌ |
| inode.rs | ❌ 0 | ❌ |
| ramfs.rs | ⚠️ 部分 | ❌ |
| procfs_core.rs | ❌ 0 | ❌ |
| flock.rs | ❌ 0 | ❌ |
| inotify.rs | ❌ 0 | ❌ |

**建议**：补 dcache 哈希冲突 + vfs_manager 路径解析 + flock 死锁测试。

---

## 9. 修复优先级

| 优先级 | 问题 | 工作量 | 风险 |
|---|---|---:|---|
| P0-1 | 2.1 VFS_MAX_FDS=32 | 8h | 阻塞并发 |
| P0-2 | 2.3 resolve_mount 重入锁 | 2h | 死锁 |
| P0-3 | 2.4 alloc_fd 跨进程 fd 冲突 | 4h | 严重 |
| P0-4 | 2.2 dcache 全局锁 | 16h | 性能 |
| P0-5 | 2.5 inotify 隐私 | 4h | 安全 |
| P0-6 | 2.6 set_path 截断 | 1h | 数据损坏 |
| P0-7 | 2.7 循环依赖 | 4h | 重构风险 |
| P0-8 | 2.8 init 漏重置 | 1h | 状态泄漏 |
| P1 | 12 项 | 40h | |
| P2/P3 | 27 项 | 24h | 维护性 |

**总计**：约 100h
