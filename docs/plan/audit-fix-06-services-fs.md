# 审计修复分册 06：services 文件系统

> 修复 services/fs（access/file_handle/file_ops/inode/vfs）与 services/proc/namespace 的审计缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.3 节 + 附录 B（1/4 大文件）+ 附录 H（H.5.8）+ 附录 C（subsystem-services-fs 报告）。

> **2026-08-30 基线核实**：委托前对全部 27 项逐一对照当前磁盘代码核实（见各条目标注）。结论：已修复/虚报 4 项（B06-17/19/21/25）、**已实装 1 项（B06-04，DECISION-077 方案 A，本会话完成）**、仍存在 14 项、部分修复 3 项（B06-06/08/11）、背景收口 3 项（B06-01/05/16）。**其余待办由分册 6 委托人统一负责**（B06-04 已完成项作为实施范式参照，见 DECISION-077）。计划 B 定位信息过时：Inode trait/AnonymousInode/RamFsInode/LegacyInode 已整体迁至 `src/kernel/services/fs/inode.rs`（计划正文所述 `framework/fs/` 位置不适用）。

## 工程计划 A: 文件系统权限与句柄

### 背景

- **B06-01. 权限检查失效集中**
  - 描述：open_by_handle_at 无 CAP 校验、access 忽略 mode、chown UID 回退 root——三处权限路径均失效。
  - 方案：按提权风险顺序修复（chown → open_by_handle → access）。
  - 状态：[X] (2026-08-30 核实：三处缺陷均仍存在，见 B06-02/03/04；本背景条目作"确认待修"收口)

### 待办

- **B06-02. chown_syscall UID 查找失败回退 root（P0-13）**
  - 描述：[file_ops.rs:169-170](file:///home/anfer/Code/QueenX/src/kernel/services/fs/file_ops.rs#L169-L170) `tbl.find_by_uid(uid).map_or(0, ...)`，UID/GID 未注册时 owner_pwm=0（root），攻击者可获得目标文件归属权。
  - 方案：UID/GID 未找到返回 ENOENT/EINVAL，不得默认 root；补 chown host-tests。
  - 状态：[] (2026-08-30 核实：**仍存在**；行号已漂移——chown 实际位于 [file_ops.rs:125-137](file:///home/anfer/Code/QueenX/src/kernel/services/fs/file_ops.rs#L125-L137)，L131-132 `find_by_uid(uid).map_or(0,...)` / `find_by_uid(gid).map_or(0,...)` 原样保留，未改为返回错误)

- **B06-03. open_by_handle_at 无 CAP_DAC_READ_SEARCH（P0-08）**
  - 描述：[file_handle.rs:147-148](file:///home/anfer/Code/QueenX/src/kernel/services/fs/file_handle.rs#L147-L148) 注释后仅"简化: 允许所有已认证进程"，无 CAP 检查，任意进程可打开任意 inode 句柄。
  - 方案：拿 handle 前调 `credo::api::pwm_has_capability(pwm, CAP_DAC_READ_SEARCH)`，否则 EPERM。
  - 状态：[] (2026-08-30 核实：**仍存在**；[file_handle.rs:147-148](file:///home/anfer/Code/QueenX/src/kernel/services/fs/file_handle.rs#L147-L148) 仍仅注释无校验；修复 API 可用：`credo/api.rs` L200 `pwm_has_capability`，同层先例 `services/fs/mount.rs` L52/L86、`ramfs_core/ramfs_data.rs` L345)

- **B06-04. access_syscall 忽略 mode（P0-09）**
  - 描述：[access.rs:46-61](file:///home/anfer/Code/QueenX/src/kernel/services/fs/access.rs#L46-L61) mode（R_OK=4/W_OK=2/X_OK=1）仅范围校验后忽略，`access(path, W_OK)` 对只读文件返回 0。实测 framework 层**无任何 rwx 判断 API**（`vfs_check_access` 不存在），权限检查是能力制（ramfs/hvfs 用 `pwm_has_capability(FS_CAP_READ/WRITE)`）；`VfsStat.mode` 存在但可靠性分 FS（exfat 恒 777、多数 fs_chmod 是 stub）。
  - 方案：**与现有 open/read/write 路径对齐**——复用能力制检查：`R_OK→FS_CAP_READ`、`W_OK→FS_CAP_WRITE`、`X_OK→FS_CAP_EXEC`（按 credo 能力域定义），`F_OK` 保持存在性检查；与 ramfs/hvfs 既有语义一致、0 新 framework 代码。**决策点**：若要求严格 mode 位语义，需先修复各 FS 的 chmod/mode 真实性（exfat 恒 777 等），工作量大，建议作为后续独立任务。
  - 状态：[X] (2026-08-30 实装 DECISION-077 方案 A：access_syscall 复用能力制校验——`R_OK/W_OK/X_OK` 映射到 `FS_CAP_READ/WRITE/EXECUTE`，经 `pwm_has_capability(pwm, CAP_DOMAIN_FS, caps)` 判定，`F_OK` 仅做存在性检查；与 open/read/write 路径 (ramfs/hvfs) 语义一致、0 新 framework 代码。回归测试 td26_access_cap_test.rs 8 项 + 双架构编译/clippy/QEMU 全部通过。详见 DECISION-077)

## 工程计划 B: VFS 与 inode 修复

### 背景

- **B06-05. inode trait 与 VFS 上限缺陷**
  - 描述：mount_idx 硬编码 u32::MAX、VFS_MAX_FDS 与 poll 不一致、trait 默认实现掩盖错误。
  - 方案：按错误可见性顺序修复。
  - 状态：[X] (2026-08-30 核实：**定位信息过时**——Inode trait/AnonymousInode/RamFsInode/LegacyInode 已整体迁至 `src/kernel/services/fs/inode.rs`，`VFS_MAX_FDS` 定义于 `services/fs/vfs_types.rs:18`；本背景条目作"确认定位 + 待修"收口)

### 待办

- **B06-06. Inode::mount_idx 硬编码 u32::MAX（附录 B 4.1）**
  - 描述：`AnonymousInode::mount_idx` 硬编码 `u32::MAX`，mmap 路径可能 panic/越界。
  - 方案：mount_idx 返回 Option/Result，或匿名 inode 专用哨兵并在使用点显式处理。
  - 状态：[] (2026-08-30 核实：**部分**——硬编码仍在 [services/fs/inode.rs:203](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inode.rs#L203) `mount_idx: u32::MAX`，`mount_idx()` 返回类型仍为 u32 (L262-264)；但风险路径已规避：mmap 走 `fd_to_mount_idx→get_fd_mount_idx` 返回 `Option<usize>`（[mmap.rs:54-59](file:///home/anfer/Code/QueenX/src/kernel/services/mm/mmap.rs#L54-L59)），vma.mount_idx 亦为 `Option<usize>`（[vma.rs:173](file:///home/anfer/Code/QueenX/src/kernel/framework/mm/vma.rs#L173)）；`OpenFile::mount_idx()` 透传 u32::MAX 但全仓库无调用点。优先级可由 P0 降为"清理哨兵语义")

- **B06-07. VFS_MAX_FDS 与 poll 不一致（H.5.8 P1-K）**
  - 描述：framework/fs/vfs/api.rs `VFS_MAX_FDS=32` 与 poll fd 数=256 不一致。
  - 方案：统一上限常量（集中到 constants/limits.rs），poll 越界显式错误。
  - 状态：[] (2026-08-30 核实：**仍存在，且升级为确定性越界**——`VFS_MAX_FDS=32` 定义于 [services/fs/vfs_types.rs:18](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_types.rs#L18)，`fd_table` 长度 32（[vfs_manager.rs:160](file:///home/anfer/Code/QueenX/src/kernel/services/fs/vfs_manager.rs#L160)）；poll 实现 [file_ops.rs:108](file:///home/anfer/Code/QueenX/src/kernel/services/fs/file_ops.rs#L108) 用硬编码 256 做上限后**直接索引 32 长数组**，fd∈[32,255] 越界。**建议优先级 P0**)

- **B06-08. Inode trait 默认实现掩盖错误（附录 B 4.4/4.6）**
  - 描述：`Inode::seek` 默认实现缺失（部分实现者返回错误 End 计算）；chmod/chown/readlink 等全部默认 `Ok(())`/`NotSupported` 无统一错误。
  - 方案：trait 默认实现改为 `Err(NotSupported)` 显式；seek 实现统一。
  - 状态：[] (2026-08-30 核实：**部分**——chmod/chown 仍默认 `Ok(())`（[services/fs/inode.rs:148-158](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inode.rs#L148-L158)），9 个实现者仅 LegacyInode 显式覆盖；readlink 已改显式 `Err(InvalidArgument)` (L140-142)；seek 已无默认实现且 9 实现者均显式实现，不再静默)

- **B06-09. RamFsInode::is_dir 硬编码 256（附录 B 4.7）**
  - 描述：`if (self.inode_id as usize) < 256` 硬编码 inode 数量上限。
  - 方案：改为显式目录标志位。
  - 状态：[] (2026-08-30 核实：**仍存在**——[services/fs/inode.rs:376-384](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inode.rs#L376-L384) 硬编码 256 与 magic `== 1`（DIR）均未改)

- **B06-10. vfs/api.rs 单文件 1700 行（TOP 20 #18）**
  - 描述：framework/fs/vfs/api.rs 1700 行单文件，违反简单优先（决策点 D4）。
  - 方案：拆分为 4 子模块（路径/权限/句柄/挂载），行为零变更。
  - 状态：[] (2026-08-30 核实：**仍存在**——[framework/fs/vfs/api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs) 仍 1700 行未拆分)
  - 详情：⚠ **同文件冲突约束**——`vfs/api.rs` 同时被 B09-12（F2 反向依赖治理）涉及。**必须串行执行，顺序：B09-12 → B06-10**（先修依赖方向再拆分）；并发委派时本条目须等 B09-12 完成后执行。（2026-08-30 核实：B09-12 状态仍 `[]` 未启动，串行约束有效）

- **B06-11. vfs/api.rs 直调 services 层（H.5.1 P0-31）**
  - 描述：framework/fs/vfs/api.rs 严重违反 F2（直调 services 层类型，DECISION-H13/H19）。
  - 方案：services 类型迁回 framework，或 api.rs 改经顶层 re-export 访问。
  - 状态：[] (2026-08-30 核实：**部分**——VFS_MANAGER 已改经 re-export（[api.rs:25](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs#L25) → [vfs.rs:9-11](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/vfs.rs#L9-L11)）；但仍存 3 处直接 `use crate::kernel::services::fs::{devfs::DevfsData, open_file_table::OPEN_FILE_TABLE, vfs_types::OpenFile}`（[api.rs:33-35](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs#L33-L35)）)

- **B06-12. Inode::stat 路径级操作（附录 B 4.2）**
  - 描述：`LegacyInode::stat` 使用 `rel_path`，但 `fs_stat(&rel_path, pwm)` 是路径级操作，违反"Plan B Inode trait 不依赖路径"原则。
  - 方案：stat 改走 inode 句柄，不依赖路径。
  - 状态：[] (2026-08-30 核实：**仍存在**——[services/fs/inode.rs:470-483](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inode.rs#L470-L483) stat 仍走 `rel_path` + `fs_stat`；LegacyInode 仍在 file_handle.rs:191 作为 `fs_resolve_inode` 未实现时的回退路径使用)

- **B06-13. AnonymousInode 错误丢失（附录 B 4.3）**
  - 描述：`AnonymousInode::read/write` 中 `ANONYMOUS_FS.read_at(...)` 返回 `Option<usize>`，失败仅返回 `Io` 错误，丢失底层原因。
  - 方案：透传底层错误或映射到 Errno。
  - 状态：[] (2026-08-30 核实：**仍存在**——[services/fs/inode.rs:209-219](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inode.rs#L209-L219) read/write 仍 `.ok_or(KernelError::Io)` 吞底层原因)

- **B06-14. AnonymousInode::is_dir 硬编码 false（附录 B 4.5）**
  - 描述：`is_dir` 硬编码 `false`，但 AnonymousFS 可能有匿名目录 inode 类型。
  - 方案：inode 类型驱动 is_dir 判断。
  - 状态：[] (2026-08-30 核实：**仍存在**——[services/fs/inode.rs:249-251](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inode.rs#L249-L251) `fn is_dir(&self) -> bool { false }` 硬编码)

- **B06-15. LegacyInode::is_dir file_type 未更新（附录 B 4.8）**
  - 描述：`is_dir` 使用 `self.file_type == Dir.as_u8()`，但 `file_type` 字段未被 chmod/chown 路径更新。
  - 方案：file_type 由挂载/创建路径维护，chmod/chown 不改类型。
  - 状态：[] (2026-08-30 核实：**仍存在**——[services/fs/inode.rs:515-517](file:///home/anfer/Code/QueenX/src/kernel/services/fs/inode.rs#L515-L517) is_dir 用 `self.file_type == Dir.as_u8()`；`file_type` 仅在 from_fs_result (L429-436) 构造时设置一次，chmod/chown (L527-555) 不刷新)

## 工程计划 C: namespace 修复

### 背景

- **B06-16. namespace 9 项发现**
  - 描述：附录 B 1 对 namespace.rs 787 行审计出 9 项（1 项 P0 级注册表缺陷 + 权限/截断/位运算问题）。
  - 方案：按权限 → 内存安全 → 语义顺序修复。
  - 状态：[X] (2026-08-30 核实：9 项中 3 项已修复/虚报（B06-17/19/25）、6 项仍存在（B06-18/20/22/23/24 及 B06-21 已修）；见各条目标注。本背景条目作"确认状态分布"收口)

### 待办

- **B06-17. NS_REGISTRY 并发保护（附录 B 1.1）**
  - 描述：`NS_REGISTRY` 用 `IrqSpinLock` 但内部 `entries` 无并发保护，死锁/嵌套锁风险。
  - 方案：统一锁内访问或改用内部锁原语。
  - 状态：[X] (2026-08-30 核实：**已修复/虚报**——`NS_REGISTRY: IrqSpinLock<NsRegistry>`（[namespace.rs:735](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L735)），所有 `entries` 访问均持外层锁：`setns_by_type` L638 一次 lock() 后 find + 克隆 Arc（不重入锁）、`ns_register` L739 一次 lock() 后 register()，无嵌套锁/死锁路径)

- **B06-18. sys_setns 位运算公式错误（附录 B 1.2）**
  - 描述：`NsType::from_clone_flag(1 << (ns_type + 8))` 位运算公式错误。
  - 方案：按 Linux CLONE_NEW* 位定义修正映射。
  - 状态：[] (2026-08-30 核实：**仍存在**——[namespace.rs:762](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L762) `1 << (ns_type + 8)` 原样保留，对 ns_type 0..6 得 0x100..0x4000 均不等于 CLONE_NEW* 故 `from_clone_flag` 恒 None，靠 L764-773 fallback 兜底；功能可用但公式未修正)

- **B06-19. set_nodename 截断越界（附录 B 1.3）**
  - 描述：`UtsNamespace::set_nodename` 复制超长输入截断后越界写。
  - 方案：长度校验用 `min(len, cap)` 且最后补 NUL。
  - 状态：[X] (2026-08-30 核实：**已修复**——[namespace.rs:168-173](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L168-L173) `len = name.len().min(64)` + `buf[..len]` 复制 + `buf[len]=0`，缓冲 `[u8;65]` 无越界。附注：set_nodename 当前无调用者，属孤立 API 但代码已安全)

- **B06-20. setns_by_type 无权限校验（附录 B 1.4）**
  - 描述：任意进程可切换到任何 namespace。
  - 方案：加 CAP_SYS_ADMIN / ns 归属校验。
  - 状态：[] (2026-08-30 核实：**仍存在**——[namespace.rs:637-683](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L637-L683) 仅"查注册表 find + 克隆 Arc"，无 CAP_SYS_ADMIN、无 ns 归属校验)

- **B06-21. sys_unshare/sys_setns 未注册 dispatch（附录 B 1.9）**
  - 描述：未注册到 dispatch，调度入口完全断裂。
  - 方案：接线 dispatch（联动分册 05）。
  - 状态：[X] (2026-08-30 核实：**已修复**——[dispatch.rs:345-352](file:///home/anfer/Code/QueenX/src/kernel/framework/syscall/dispatch.rs#L345-L352) 已接线 `QX_UNSHARE`/`QX_SETNS`；framework re-export 链完整（framework/proc/namespace.rs L10-14 → proc/mod.rs L122）；services 层 [namespace.rs:747-758](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L747-L758)（unshare）与 L761-787（setns）完整实装，非 stub)

- **B06-22. sys_unshare/sys_setns clone_flags 互斥缺失（附录 B 1.5）**
  - 描述：`sys_unshare`/`sys_setns` 缺少 `clone_flags` 与 `CLONE_NEWUSER` 互斥校验。
  - 方案：按 Linux 语义补互斥校验。
  - 状态：[] (2026-08-30 核实：**仍存在**——`NamespaceSet::unshare` [namespace.rs:597-626](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L597-L626) 仅 `new_ns_flags == 0` 返回 EINVAL，无 CLONE_NEWUSER 组合/互斥校验；sys_setns L761-787 无标志校验)

- **B06-23. PidNamespace::alloc_pid 计数漂移（附录 B 1.6）**
  - 描述：PID 永不重用但 `nr_processes` 无 decrement，计数漂移。
  - 方案：进程退出时 decrement + PID 回收策略。
  - 状态：[] (2026-08-30 核实：**仍存在（潜在，未接线）**——[namespace.rs:271-274](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L271-L274) 仅 `nr_processes.fetch_add(1)` 与 `next_pid.fetch_add(1)`，无 decrement；但 `alloc_pid` 全仓库无调用者（真实 PID 走 framework `proc_alloc_pid` user_proc.rs:2026），漂移尚未实际触发)

- **B06-24. map_uid/map_gid 边界（附录 B 1.7）**
  - 描述：`UserNamespace::map_uid/map_gid` 未考虑 count=0 / 溢出。
  - 方案：补 count=0 与溢出校验。
  - 状态：[] (2026-08-30 核实：**仍存在（部分，未接线）**——[namespace.rs:381-408](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L381-L408) `inner_start + count` 为普通 u32 加法无 checked_add，count=0 无显式分支；map_uid/map_gid 亦无调用者，属潜在风险)

- **B06-25. next_ephemeral_port 永不自旋回卷（附录 B 1.8）**
  - 描述：`NetNamespace::next_ephemeral_port` AtomicU16 永不自旋回卷。
  - 方案：自旋回卷 + 冲突跳过策略。
  - 状态：[X] (2026-08-30 核实：**虚报**——`next_ephemeral_port` 字段仅声明与初始化（[namespace.rs:425/435/445](file:///home/anfer/Code/QueenX/src/kernel/services/proc/namespace.rs#L425)），全仓库无任何函数读取/分配该端口，"永不自旋回卷"的分配函数当前不存在，无缺陷可修)

### 验证门槛

- **B06-26. fs 回归**
  - 描述：权限修复后跑 fs host-tests（access/chown/file_handle 用例）。
  - 方案：`make test-host`。
  - 状态：[] (2026-08-30 核实：委托前基线已确认——`./ci/build.sh all` 5/5 通过、host-tests 10/10；修复后需复跑)

- **B06-27. vfs 拆分回归**
  - 描述：vfs/api.rs 拆分后全量 host-tests + 双架构编译，确认行为零变更。
  - 方案：`make test-host` + `./ci/build.sh all`。
  - 状态：[] (2026-08-30 核实：拆分未开始；串行前置 B09-12 亦未开始，见 B06-10)

## DECISION-077（2026-08-30）

- **B06-04 access_syscall 权限语义：复用能力制（方案 A）**
  - 背景：`access_syscall` 原实现只做 `0..=0o7` 范围校验 + 存在性检查，mode（R_OK/W_OK/X_OK）被丢弃，`access(path, W_OK)` 对只读文件也返回成功（P0-09 提权风险）。方案 A（能力制）vs 方案 B（严格 mode 位语义）经用户裁决选 A。
  - 决策：`R_OK→FS_CAP_READ`、`W_OK→FS_CAP_WRITE`、`X_OK→FS_CAP_EXECUTE`，经 `pwm_has_capability(pwm, CAP_DOMAIN_FS, caps)` 判定；`F_OK`（mode=0）不要求能力，仅做存在性检查。与 open/read/write 路径（ramfs/hvfs `check_permission`）语义一致，0 新 framework 代码。
  - 理由：能力制是 QueenX 既有安全模型（DECISION-H13/H19 产物），方案 B 需先修各 FS chmod/mode 真实性（exfat 恒 777、fs_chmod stub），工作量大且会造成"open 用能力制、access 用 mode 位"的自相矛盾。
  - 影响：仅改 `services/fs/access.rs`（约 +20 行）；`faccessat_syscall` 委托 access_syscall 自动生效。回归测试 td26_access_cap_test.rs 8 项覆盖 F_OK/R/W/X/组合/越界/能力先于存在性/viable floor。
  - 验证：双架构 `cargo check` + clippy（-D warnings -D pedantic）0 错误、`audit_services_boundary` 0 违规、host-tests 全量 RC=0、`./ci/build.sh all` 5/5、QEMU x86_64 启动到 Ring 3。
