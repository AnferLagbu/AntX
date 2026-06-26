# E6: VFS 策略提取与文件系统解耦计划

> 本文档记录 VFS/RamFS/HvFS 策略提取与解耦的完整方案、分析结论与执行路径. 目标: 将文件系统策略逻辑从 framework 迁移到 services, 缩减 TCB, 并为新增文件系统建立可扩展架构. 创建于 2026-06-22, 2026-06-26 归档重写.

## 工程计划 A: VFS 策略提取与解耦

### 背景
- **背景条目**
  - 描述: VFS api.rs 存在职责混合: 系统调用边界 (机制, unsafe 用户指针) + 文件系统分发 (策略, 14 个 match 分发点). 每新增 FS 需在 14+ 分发点各加分支
  - 方案: 目标: TCB 缩减 (RamFS 1639 + HvFS 6154 + dcache 876 = 8669 行策略代码从 framework 移到 services) + 可扩展性 (新增 FS 只需在 services 实现 FileSystem trait) + 安全契约 (services 保持 #![deny(unsafe_code)], unsafe 留 framework)
  - 状态: [X]

### 现状 (已完成)
- **E6-1 flock.rs 策略提取**
  - 描述: 730+ 行 flock 策略提取
  - 方案: framework/fs/vfs/flock.rs 转 re-export 层; services/fs/flock.rs 0 unsafe; TCB 收益 -730 行
  - 状态: [X]
- **E6-2 inotify.rs 策略提取**
  - 描述: 590+ 行 inotify 策略提取
  - 方案: framework 保留 re-export + sys_inotify_read (unsafe 用户缓冲区); services/fs/inotify.rs 0 unsafe; 新增 safe API inotify_read_events; TCB 收益 -590 行
  - 状态: [X]
- **E6-3 dcache 策略提取**
  - 描述: 876 行 dcache 策略提取
  - 方案: framework/fs/vfs/dcache.rs → services/fs/dcache.rs 0 unsafe; framework 仅 re-export 公共 API; TCB 收益 -846 行
  - 状态: [X]
- **E6-4 VFS 分发策略提取 (FileSystem trait)**
  - 描述: api.rs 14 处 match 替换为 trait object 分发
  - 方案: FileSystem trait 20 个方法 (open/close/read/write/stat/chmod/chown/mkdir/unlink/rmdir/rename/readdir/symlink/readlink/link/truncate/seek/resolve_path/create) + FsOpenResult 类型 + VfsMount fs 字段 + RamFS/HvFS trait 实现 + api.rs 14 处替换 + 双架构编译通过
  - 状态: [X]
- **E6-5 RamFS 迁移到 services**
  - 描述: 1639 行 RamFS 迁移
  - 方案: services/fs/ramfs_core.rs 实现 FileSystem trait; framework/fs/ramfs/ramfs.rs 转为 re-export (~10 行); api.rs fallback 全部移除, 仅保留 trait 分发; TCB 收益 -1629 行
  - 状态: [X]
- **E6-7 DevFS 迁移到 services**
  - 描述: 295 行 DevFS 迁移
  - 方案: services/fs/devfs.rs 0 unsafe; framework re-export 公共 API; klog_info 替换为 framework::klog::serial_write_bytes; TCB 收益 -282 行
  - 状态: [X]
- **E6-8 ProcFS 迁移到 services**
  - 描述: 245 行 ProcFS 迁移
  - 方案: services/fs/procfs_core.rs 0 unsafe; framework re-export (~7 行); SafeProcFs import 路径更新; TCB 收益 -238 行
  - 状态: [X]
- **E6-9a DevFS 硬编码消除**
  - 描述: 5 个虚拟设备硬编码消除
  - 方案: DevfsData::mount() 改为空操作; devfs::init() 调用 register_standard() 显式注册; DevKind 扩展 Block/Char/Net/Input; is_virtual/is_physical 方法; read/write 新增物理设备分支
  - 状态: [X]
- **E6-9b Chitin→DevFS 桥接**
  - 描述: 设备注册自动通知 DevFS
  - 方案: Chitin 注册表新增 DEVICE_REGISTER_CALLBACK 钩子 + chitin_set_register_callback() + notify_last_registered() 5 个注册函数 push 后自动通知 + DevFS init_with_chitin_bridge() + on_chitin_device_registered() 回调 ChitinProto 映射 DevKind; 锁顺序 callback → chitin_devices → devfs_devices 无死锁
  - 状态: [X]
- **E6-9c DevFS 接入 VFS 分发**
  - 描述: DevFS 走 VFS trait 分发路径
  - 方案: DevfsData 实现 FileSystem trait + FsType 新增 DevFs 变体 + vfs_mount_internal 新增初始化分支 + DevFS 不支持操作返回 NotSupported/PermissionDenied + fs_stat 返回字符设备模式 0o20666
  - 状态: [X]

### 累计 TCB 收益
- **TCB 收益汇总**
  - 描述: 7 个文件系统的 TCB 收益
  - 方案: flock 迁移前 730 → 迁移后 30 (re-export) = -700; inotify 590 → 50 = -540; dcache 876 → 30 = -846; DevFS 295 → 13 = -282; RamFS 1639 → 10 = -1629; ProcFS 245 → 7 = -238; 合计 4375 → ~140 = **-4235 行**
  - 状态: [X]

## 工程计划 B: 现状耦合分析

### 背景
- **背景条目**
  - 描述: 4 个核心耦合点 + 3 个非 VFS 分发 FS 现状 + HvFS framework 依赖详情
  - 方案: 理解当前状态, 为后续扩展提供基础
  - 状态: [X]

### 现状
- **模块规模与安全属性**
  - 描述: 4 个核心模块
  - 方案: vfs/api.rs 1415 行 15 unsafe (用户指针) → RamFS 24 处 → HvFS 30 处; ramfs/ramfs.rs 1639 行 0 unsafe → dcache 14 处; hvfs/ (18 文件) 6154 行 10 unsafe (磁盘序列化) → block driver 24 处 → credo 4 处 → sync 多处; vfs/dcache.rs 876 行 0 unsafe 无外部依赖
  - 状态: [X]
- **核心耦合点**
  - 描述: 5 类耦合
  - 方案: (1) api.rs 14 个 match fs_type 硬编码分发 / (2) api.rs 直接访问 RamFS 内部字段 vfs_fchmod/vfs_fchown/vfs_link/vfs_symlink/vfs_readlink / (3) api.rs 暴露 RamFS 类型 RamFsDirEntry (2 处) / (4) RamFS/HvFS 方法签名不统一 RamFS 用 node_id HvFS 用 path/fd / (5) HvFS 子模块间强耦合 18 文件互相引用
  - 状态: [X]
- **非 VFS 分发 FS 现状**
  - 描述: 3 个非 VFS 分发 FS
  - 方案: DevFS 295 行 0 unsafe IrqSpinLock SafeDevFs 229 行代理 接入 VFS 否; ProcFS 245 行 0 unsafe IrqSpinLock + pmm_api (2 处读内存统计) SafeProcFs 191 行代理 接入 VFS 否; InitRamFS 333 行 1 unsafe (unpack 函数 from_raw_parts) vfs::api 6 处: mkdir/open/write/close/symlink 无 接入 VFS 否
  - 状态: [X]
- **HvFS framework 依赖详情**
  - 描述: 4 类依赖
  - 方案: driver::block (hdd_read/write_sector 等) 24 处 safe (&mut [u8]/&[u8]) services 可调用; credo::api (pwm 权限检查) 4 处 safe 可调用; sync::{Mutex, IrqSpinLock, OnceLock} 多处 safe 可调用; vfs::types::KernelError 1 处 纯数据类型 可 re-export; 关键发现: HvFS 10 unsafe 全部是磁盘数据结构序列化 (as_bytes/from_bytes_unaligned), 不是硬件操作, 硬件操作走 block::hdd_* 全部 safe
  - 状态: [X]

## 工程计划 C: FileSystem trait 设计

### 方案
- **trait 接口定义**
  - 描述: 20 个方法覆盖全部 VFS 操作
  - 方案: pub trait FileSystem: Send + Sync; 生命周期: name() + fs_init() + fs_mount() + fs_unmount(); 文件操作: fs_open(rel_path, flags, pwm) + fs_close(fd) + fs_read(fd, buf) + fs_write(fd, buf); 元数据: fs_stat(rel_path, pwm) + fs_chmod(rel_path, mode, pwm) + fs_chown(rel_path, owner, group, pwm); 目录操作: fs_mkdir/fs_unlink/fs_rmdir/fs_rename + fs_readdir; 符号链接: fs_symlink/fs_readlink/fs_link; pub struct FsOpenResult { node_id, offset, file_type }
  - 状态: [X]
- **前置工作**
  - 描述: 4 项前置
  - 方案: (1) 统一 RamFS/HvFS 方法签名 (RamFS 用 node_id, HvFS 用 path/fd) / (2) 封装 RamFS 内部字段访问 (vfs_fchmod 等直接访问 ramfs.nodes[node_id] 改为方法调用) / (3) 抽象 RamFsDirEntry (api.rs 2 处直接使用 RamFS 类型, 用 VFS 层类型替代) / (4) 剥离 unsafe 用户指针操作 (api.rs UserReadPtr/UserWritePtr 必须留 framework, 策略部分移到 trait 实现)
  - 状态: [X]
- **VfsMount 改造**
  - 描述: 引入 fs: Option<&'static dyn FileSystem> 字段
  - 方案: 改造前 pub struct VfsMount { path, fs_type: FsType, used } 枚举硬编码; 改造后 pub struct VfsMount { path, fs: Option<&'static dyn FileSystem>, used } trait object + VfsManager 新增 resolve_mount_fs() 和 mount_with_fs()
  - 状态: [X]

## 变更历史
- **2026-06-26**
  - 描述: 按新文档规则重写 (标题+条目(描述+方案+状态)+详情)
  - 方案: 结构重组, 保留原意
  - 状态: [X]
- **2026-06-22**
  - 描述: E6 VFS 策略提取与文件系统解耦完成 (E6-1~E6-9c 全部 [X])
  - 方案: 累计 TCB 收益 -4235 行
  - 状态: [X]
