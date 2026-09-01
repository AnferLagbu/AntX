# Credo 能力位常量治理计划（复用先例回溯）

> 独立计划文档：登记能力位常量的"新增 vs 复用"决策准则与既有复用先例的回溯治理方案。
> **本计划仅登记决策与规划，不修改任何代码**（2026-08-31 用户明确指示）。
> 准则来源：分册 7 DECISION-078（用户 2026-08-31 确立）。

## 背景

分册 7 委托前，用户为 B07-05（pwm_set_syscall 任意提权）确立了常量决策准则：

> **重要或特殊用途常量采用新增，其他的采用复用既有。**

该准则确立后，需回溯检查此前所有"复用能力位"的先例，评估是否也存在"语义不精确 / 魔法数 / 缺命名常量"问题。本计划对此前 10 处 `pwm_has_capability` 复用点逐一定性，并登记治理决策。

## 既有复用先例盘点（10 处）

### A 类：语义精确、与 Linux 对齐 → 保持复用（3 项）

| # | 先例 | 位置 | 使用位 | Linux 对应 | 处置 |
|---|---|---|---|---|---|
| A1 | mount / umount2 | `services/fs/mount.rs:52,86` | SYSTEM 域 `0x01` | CAP_SYS_ADMIN（同义） | 保持复用 ✅ |
| A2 | setns | `services/proc/namespace.rs:812` | SYSTEM 域 `0x01` | CAP_SYS_ADMIN（同义） | 保持复用 ✅ |
| A3 | ramfs/hvfs 文件权限 | `services/fs/ramfs_core/ramfs_data.rs:345`、`services/fs/hvfs/hvfs_data.rs:530` | `FS_CAP_*`/`PROC_CAP_*` 命名常量 | 能力制 | 保持复用 ✅（命名规范） |

### B 类：语义不精确 / 魔法数 / 缺命名 → 需治理（5 项）

| # | 先例 | 位置 | 现状问题 | 治理决策 | 状态 |
|---|---|---|---|---|---|
| B1 | reboot（重启系统） | `services/proc/sysinfo.rs:155` | 复用 SYSTEM `0x01`，但重启是独立系统操作，Linux 语义应 CAP_SYS_BOOT | **新增专用能力位**（CAP_SYS_BOOT 语义） | [] 待实施 |
| B2 | open_by_handle_at | `services/fs/file_handle.rs:150` | 复用 SYSTEM `0x01`（B06-03 曾裁决），但绕过路径直接开 inode 句柄属高敏操作，Linux 语义应 CAP_DAC_READ_SEARCH | **新增专用能力位**（CAP_DAC_READ_SEARCH 语义，回溯 B06-03 裁决） | [] 待实施 |
| B3 | sethostname | `services/proc/sysinfo.rs:136` | **0x09 未命名魔法数**（=bit0\|bit3，非 0x01 也非命名常量），疑似 bug | **修复为命名常量**（新增 SET_HOSTNAME 位，或归入命名后的系统管理位） | [] 待实施 |
| B4 | mmap | `services/mm/mmap.rs:304` | MEM 域 `0x01` **无命名常量**（capability.rs 无 MEM_CAP_* 定义） | **新增 MEM_CAP_* 命名位**（补 capability.rs MEM 域常量） | [] 待实施 |
| B5 | sys_boot_install | `framework/syscall/dispatch.rs:1188` | `pwm_has_capability(pwm, 4, 0)` **required=0 可疑**（无实际位要求） | **留待澄清**——先确认意图（缺位 bug 还是有意为之）再定 | [] 待澄清 |

## 实施规划（供后续委托人执行，本计划不落地代码）

### 新增能力位布局建议（待实施时按 capability.rs 实际布局设计）

- **B1 reboot**：SYSTEM 域新增 `CAP_SYS_BOOT` 位，替换 sysinfo.rs:155 的 `0x01` 复用。
- **B2 open_by_handle_at**：新增 `CAP_DAC_READ_SEARCH` 语义位（归属域待定，可 SYSTEM 或 FS 域），替换 file_handle.rs:150 的 `0x01`。
- **B3 sethostname**：新增命名常量（如 SYSTEM 域 `CAP_SET_HOSTNAME`），替换 sysinfo.rs:136 的魔法数 `0x09`。
- **B4 mmap**：capability.rs 新增 `MEM_CAP_*` 命名常量族（如 MEM_CAP_ALLOC 等），替换 mmap.rs:304 的 `0x01`。
- **B5 sys_boot_install**：实施前先由委托人澄清 `required=0` 的真实意图（若为缺位 bug，补齐所需位）。

### 验证门槛（实施时）

1. `capability.rs` 新增常量后，`framework/credo/mod.rs` re-export 同步（若属公共 API）。
2. 各调用点替换后，`audit_services_boundary.py` 0 违规（services 层引用合规）。
3. 双架构 `cargo check` + clippy（-D pedantic）0 error。
4. host-tests 全量通过（cred 相关回归）。
5. 涉及 B2 时，补充 open_by_handle_at 权限拒绝 host-tests（B06-03 已有先例）。

## 变更历史

- **2026-08-31**：创建本计划。用户确认决策倾向 1① 2① 3① 4① 5②（B1/B2 新增专用位、B3 修魔法数、B4 补命名、B5 待澄清）；A 类 3 项保持复用。**仅登记计划，不修改代码**。
