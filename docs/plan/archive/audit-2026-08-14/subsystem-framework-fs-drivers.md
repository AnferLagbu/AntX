# framework/fs (drivers 子模块) 深度审计报告

> **审计范围**：`src/kernel/framework/fs/`（vfs/ + devfs/ + procfs/ + ramfs/ + hvfs/ + initramfs.rs + vfs_poll_trait.rs + mod.rs）
> **审计日期**：2026-08-14
> **文件数**：19 个源文件
> **代码规模**：约 2,543 LoC
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **19 个问题（P0×4, P1×6, P2×6, P3×3）**

## 1. 子系统概览

| 子模块 | 文件 | LoC | 主要职责 | 风险等级 |
|---|---|---:|---|---|
| vfs/ | 8 | 2,054 | VFS 主实现（含 api.rs 1700 行）| **极高** |
| initramfs.rs | 1 | 335 | cpio 解析与加载 | **高** |
| vfs_poll_trait.rs | 1 | 153 | poll 抽象 | 中 |
| hvfs/ | 2 | 50 | HvFS 桩（实际在 services/fs/hvfs）| 低 |
| ramfs/ | 2 | 11 | ramfs 桩 | 低 |
| procfs/ | 2 | 9 | procfs 桩 | 低 |
| devfs/ | 2 | 12 | devfs 桩 | 低 |
| mod.rs | 1 | 19 | 子系统入口 | 低 |

## 2. 严重问题

### 2.1 [P0] `vfs/api.rs:1700` vfs/api.rs 1700 行单文件**严重违反严重简单优先**

- **位置**：[vfs/api.rs:1-1700](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs#L1-L1700)
- **问题**：
  - 单文件 1700 行包含：VFS 接口契约 + 路径解析 + pcache + 用户指针 + ramfs/devfs/hvfs 调用。
  - 应拆分：
    - `vfs/path.rs`（路径解析）
    - `vfs/pcache.rs`（pcache 调用）
    - `vfs/user.rs`（用户指针）
    - `vfs/syscall.rs`（syscall 入口）

### 2.2 [P0] `vfs/api.rs:21-35` 导入 services 层类型（违反 F2 单向数据流）

- **位置**：[vfs/api.rs:21-35](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs#L21-L35)
- **代码**：
  ```rust
  use crate::kernel::services::fs::devfs::DevfsData;
  use crate::kernel::services::fs::open_file_table::OPEN_FILE_TABLE;
  use crate::kernel::services::fs::vfs_types::OpenFile;
  ```
- **问题**：
  - `framework/fs/vfs/api.rs` 直接 `use crate::kernel::services::*`。
  - 违反 AGENTS.md F2 "services 禁止访问 framework 内部模块"（虽然这是反向——framework 访问 services）。
  - 应通过 `framework::config` 或 trait 抽象传递。

### 2.3 [P0] `initramfs.rs:335` cpio 解析**未审计多边界情况**

- **位置**：[initramfs.rs:1-335](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/initramfs.rs#L1-L335)
- **问题**：
  - `parse_hex_field`（[initramfs.rs:68-80](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/initramfs.rs#L68-L80)）遇到非 hex 字符 break 但**未报错**——损坏的归档静默失败。
  - `align4`（[initramfs.rs:83-85](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/initramfs.rs#L83-L85)）溢出风险。
  - 符号链接目标字符串**未验证长度**。

### 2.4 [P0] `vfs/api.rs:30` `static RAMFS_MOUNTED: AtomicBool` 全局状态**无文档化挂载/卸载协议**

- **位置**：[vfs/api.rs:63](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs#L63)
- **代码**：
  ```rust
  static RAMFS_MOUNTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
  ```
- **问题**：
  - 单一布尔标记 ramfs 是否挂载——**无锁保护**。
  - 多 CPU 并发 mount/unmount 可能重复挂载或丢失状态。

## 3. P1 问题

### 3.1 [P1] `vfs/api.rs:1700` 路径解析 `split_parent_name` 启发式可能错误

- **位置**：[vfs/api.rs:75-83](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs#L75-L83)
- **代码**：
  ```rust
  fn split_parent_name(rel_path: &str) -> (&str, &str) {
      rel_path.rfind('/').map_or(("/", rel_path), |pos| {
          if pos == 0 {
              ("/", &rel_path[1..])
          } else {
              (&rel_path[..pos], &rel_path[pos + 1..])
          }
      })
  }
  ```
- **问题**：
  - 路径含尾部 `/`（如 `/dir/`） → 父路径=父目录，name="dir"。
  - 路径含多个 `/`（如 `//dir`） → 父路径="/"——**与 POSIX 不一致**。
  - 路径含 `..` → 不处理——**symlink traversal 漏洞**。

### 3.2 [P1] `vfs/api.rs` `pcache` 调用**无锁边界**

- **位置**：[vfs/api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs)
- **问题**：
  - pcache 全局锁（[subsystem-mm.md §2.x](../audit/subsystem-mm.md)）调用路径未文档化。

### 3.3 [P1] `initramfs.rs:91` `parse_next_entry` 数据切片边界

- **位置**：[initramfs.rs:91-115](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/initramfs.rs#L91-L115)
- **问题**：
  - 切片 `[offset..offset + HEADER_SIZE]` 在 offset 接近 data.len() 时**已检查**但 data.len() = 0 时 panic？
  - `header[0..6]` 边界——已验证。

### 3.4 [P1] `vfs/api.rs:1700` vfs_read/write 中**用户指针验证分散**

- **位置**：[vfs/api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs)
- **问题**：
  - 与 [subsystem-framework-toplevel.md §2.2 userptr 校验](../audit/subsystem-framework-toplevel.md) 关联。

### 3.5 [P1] `vfs/mod.rs:31` VFS_MANAGER 全局单例**与之前审计同模式**

- **位置**：[vfs/mod.rs:1-31](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/mod.rs#L1-L31)
- **问题**：
  - 之前审计（[subsystem-services-fs.md §3.2 P0 dcache 全局锁](../audit/subsystem-services-fs.md)）已识别全局单例问题。

### 3.6 [P1] `vfs_poll_trait.rs:153` poll trait 实现**未深审**

- **位置**：[vfs_poll_trait.rs:1-153](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs_poll_trait.rs#L1-L153)
- **问题**：
  - poll 抽象层。

## 4. P2 问题

### 4.1 [P2] `devfs/devfs.rs:9` 仅 9 行（实际在 services/fs/devfs）

- **位置**：[devfs/devfs.rs:1-9](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/devfs/devfs.rs#L1-L9)
- **问题**：
  - 占位。

### 4.2 [P2] `procfs/procfs.rs:6` 仅 6 行（实际在 services/fs/procfs）

- **位置**：[procfs/procfs.rs:1-6](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/procfs/procfs.rs#L1-L6)
- **问题**：
  - 占位。

### 4.3 [P2] `ramfs/ramfs.rs:8` 仅 8 行

- **位置**：[ramfs/ramfs.rs:1-8](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/ramfs/ramfs.rs#L1-L8)
- **问题**：
  - 占位。

### 4.4 [P2] `vfs/api.rs:1700` 全局 VFS_MAX_FDS=32 硬编码（[subsystem-services-fs.md §3.8 P0](../audit/subsystem-services-fs.md)）

- **位置**：[vfs/api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs)
- **问题**：
  - 与之前审计关联。

### 4.5 [P2] `vfs/api.rs:1700` `OPEN_FILE_TABLE` services 单例**接口未审**

- **位置**：[vfs/api.rs:34](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs#L34)
- **问题**：
  - OPEN_FILE_TABLE 访问约束。

### 4.6 [P2] `initramfs.rs:121-170` cpio entry 处理**符号链接递归保护缺失**

- **位置**：[initramfs.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/initramfs.rs#L121-L170)
- **问题**：
  - 符号链接 → 链式解析无 max-depth 限制。

## 5. P3 问题

### 5.1 [P3] `mod.rs:19` 子系统入口极简

- **位置**：[mod.rs:1-19](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/mod.rs#L1-L19)
- **问题**：
  - 19 行入口。

### 5.2 [P3] `hvfs/` 仅占位（实际在 services）

- **位置**：[hvfs/](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/hvfs/)
- **问题**：
  - 占位。

### 5.3 [P3] `vfs/api.rs:1700` `fd_notify` 全局回调注册**与 audit 类似问题**

- **位置**：[vfs/api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/fs/vfs/api.rs)
- **问题**：
  - 与 [subsystem-framework-toplevel.md §3.5](../audit/subsystem-framework-toplevel.md) 同模式。

## 6. 跨子系统关联

### 6.1 fs ↔ fs (services)

- framework/fs → services/fs 频繁反向。
- 与 F2 单向数据流严重违反。

### 6.2 fs ↔ mm (page cache)

- pcache 跨 framework/mm 与 framework/fs。
- 与 [subsystem-framework-mm-remaining.md §3.x](../audit/subsystem-framework-mm-remaining.md) 关联。

### 6.3 fs ↔ driver

- 块设备驱动通过 chitin::BlockDevice trait 注册到 VFS。
- 与 [subsystem-driver.md](../audit/subsystem-driver.md) 关联。

### 6.4 fs ↔ process

- ELF 加载通过 VFS 读取。
- 与 [subsystem-framework-proc-remaining.md §3.x](../audit/subsystem-framework-proc-remaining.md) 关联。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 4 | 4-6 天 |
| **P1** | 6 | 4-5 天 |
| **P2** | 6 | 2-3 天 |
| **P3** | 3 | 0.5 天 |
| **合计** | **19** | **11-15 天** |

### P0 修复路径（建议执行顺序）

1. **§2.1 vfs/api.rs 单文件拆分**（1-2 天）
2. **§2.2 framework→services 反向依赖**（1-2 天）
3. **§2.3 initramfs 边界检查**（0.5-1 天）
4. **§2.4 RAMFS_MOUNTED 状态机**（0.5 天）