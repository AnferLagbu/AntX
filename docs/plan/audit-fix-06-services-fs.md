# 审计修复分册 06：services 文件系统

> 修复 services/fs（access/file_handle/file_ops/inode/vfs）与 services/proc/namespace 的审计缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.3 节 + 附录 B（1/4 大文件）+ 附录 H（H.5.8）+ 附录 C（subsystem-services-fs 报告）。

## 工程计划 A: 文件系统权限与句柄

### 背景

- **权限检查失效集中**
  - 描述：open_by_handle_at 无 CAP 校验、access 忽略 mode、chown UID 回退 root——三处权限路径均失效。
  - 方案：按提权风险顺序修复（chown → open_by_handle → access）。
  - 状态：[]

### 待办

- **chown_syscall UID 查找失败回退 root（P0-13）**
  - 描述：[file_ops.rs:169-170](file:///home/anfer/Code/QueenX/src/kernel/services/fs/file_ops.rs#L169-L170) `tbl.find_by_uid(uid).map_or(0, ...)`，UID/GID 未注册时 owner_pwm=0（root），攻击者可获得目标文件归属权。
  - 方案：UID/GID 未找到返回 ENOENT/EINVAL，不得默认 root；补 chown host-tests。
  - 状态：[]

- **open_by_handle_at 无 CAP_DAC_READ_SEARCH（P0-08）**
  - 描述：[file_handle.rs:147-148](file:///home/anfer/Code/QueenX/src/kernel/services/fs/file_handle.rs#L147-L148) 注释后仅"简化: 允许所有已认证进程"，无 CAP 检查，任意进程可打开任意 inode 句柄。
  - 方案：拿 handle 前调 `credo::api::pwm_has_capability(pwm, CAP_DAC_READ_SEARCH)`，否则 EPERM。
  - 状态：[]

- **access_syscall 忽略 mode（P0-09）**
  - 描述：[access.rs:46-61](file:///home/anfer/Code/QueenX/src/kernel/services/fs/access.rs#L46-L61) mode（R_OK=4/W_OK=2/X_OK=1）仅范围校验后忽略，`access(path, W_OK)` 对只读文件返回 0。
  - 方案：接 `vfs_check_access(path, pwm, mode)` 按 rwx 位判断；doc 注释同步修正。
  - 状态：[]

## 工程计划 B: VFS 与 inode 修复

### 背景

- **inode trait 与 VFS 上限缺陷**
  - 描述：mount_idx 硬编码 u32::MAX、VFS_MAX_FDS 与 poll 不一致、trait 默认实现掩盖错误。
  - 方案：按错误可见性顺序修复。
  - 状态：[]

### 待办

- **Inode::mount_idx 硬编码 u32::MAX（附录 B 4.1）**
  - 描述：`AnonymousInode::mount_idx` 硬编码 `u32::MAX`，mmap 路径可能 panic/越界。
  - 方案：mount_idx 返回 Option/Result，或匿名 inode 专用哨兵并在使用点显式处理。
  - 状态：[]

- **VFS_MAX_FDS 与 poll 不一致（H.5.8 P1-K）**
  - 描述：framework/fs/vfs/api.rs `VFS_MAX_FDS=32` 与 poll fd 数=256 不一致。
  - 方案：统一上限常量（集中到 constants/limits.rs），poll 越界显式错误。
  - 状态：[]

- **Inode trait 默认实现掩盖错误（附录 B 4.4/4.6）**
  - 描述：`Inode::seek` 默认实现缺失（部分实现者返回错误 End 计算）；chmod/chown/readlink 等全部默认 `Ok(())`/`NotSupported` 无统一错误。
  - 方案：trait 默认实现改为 `Err(NotSupported)` 显式；seek 实现统一。
  - 状态：[]

- **RamFsInode::is_dir 硬编码 256（附录 B 4.7）**
  - 描述：`if (self.inode_id as usize) < 256` 硬编码 inode 数量上限。
  - 方案：改为显式目录标志位。
  - 状态：[]

- **vfs/api.rs 单文件 1700 行（TOP 20 #18）**
  - 描述：framework/fs/vfs/api.rs 1700 行单文件，违反简单优先（决策点 D4）。
  - 方案：拆分为 4 子模块（路径/权限/句柄/挂载），行为零变更。
  - 状态：[]

- **vfs/api.rs 直调 services 层（H.5.1 P0-31）**
  - 描述：framework/fs/vfs/api.rs 严重违反 F2（直调 services 层类型，DECISION-H13/H19）。
  - 方案：services 类型迁回 framework，或 api.rs 改经顶层 re-export 访问。
  - 状态：[]

## 工程计划 C: namespace 修复

### 背景

- **namespace 9 项发现**
  - 描述：附录 B 1 对 namespace.rs 787 行审计出 9 项（1 项 P0 级注册表缺陷 + 权限/截断/位运算问题）。
  - 方案：按权限 → 内存安全 → 语义顺序修复。
  - 状态：[]

### 待办

- **NS_REGISTRY 并发保护（附录 B 1.1）**
  - 描述：`NS_REGISTRY` 用 `IrqSpinLock` 但内部 `entries` 无并发保护，死锁/嵌套锁风险。
  - 方案：统一锁内访问或改用内部锁原语。
  - 状态：[]

- **sys_setns 位运算公式错误（附录 B 1.2）**
  - 描述：`NsType::from_clone_flag(1 << (ns_type + 8))` 位运算公式错误。
  - 方案：按 Linux CLONE_NEW* 位定义修正映射。
  - 状态：[]

- **set_nodename 截断越界（附录 B 1.3）**
  - 描述：`UtsNamespace::set_nodename` 复制超长输入截断后越界写。
  - 方案：长度校验用 `min(len, cap)` 且最后补 NUL。
  - 状态：[]

- **setns_by_type 无权限校验（附录 B 1.4）**
  - 描述：任意进程可切换到任何 namespace。
  - 方案：加 CAP_SYS_ADMIN / ns 归属校验。
  - 状态：[]

- **sys_unshare/sys_setns 未注册 dispatch（附录 B 1.9）**
  - 描述：未注册到 dispatch，调度入口完全断裂。
  - 方案：接线 dispatch（联动分册 05）。
  - 状态：[]

### 验证门槛

- **fs 回归**
  - 描述：权限修复后跑 fs host-tests（access/chown/file_handle 用例）。
  - 方案：`make test-host`。
  - 状态：[]

- **vfs 拆分回归**
  - 描述：vfs/api.rs 拆分后全量 host-tests + 双架构编译，确认行为零变更。
  - 方案：`make test-host` + `./ci/build.sh all`。
  - 状态：[]
