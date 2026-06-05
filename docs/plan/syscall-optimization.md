# 系统调用接口优化方案

> **版本**: 1.0  
> **关联文档**: [credo-did-design.md](credo-did-design.md), [posix-interface-plan.md](../engineering/posix-interface-plan.md)

---

## 1. 审计结果

### 1.1 全量统计

```
POSIX 标准 syscall (0-399):    76 个已定义
  ├── 已实现且有处理函数:          51 个
  ├── 显式 ENOSYS 桩:             3 个 (mprotect, alarm, net_nosys)
  ├── 显式 EPERM (有意设计):      2 个 (setuid, setgid)
  ├── 定义但未注册到 dispatch:    13 个 → 全部落入 catch-all ENOSYS  ← 问题1
  └── 额外非标准 (fchown):        1 个 → 同样未注册

QueenX 私有 syscall (400-439):  30 个  ← 问题2
  ├── 身份/认证 (400-416):       17 个
  ├── 磁盘管理 (420-425):         6 个
  ├── 进程管理 (430-432):         3 个
  ├── 系统管理 (433-437):         5 个

Framebuffer 设备 (450-452):      3 个  ← 问题3

总计: 109 个 syscall 常量
```

### 1.2 问题一：13 个 POSIX 孤儿

这些常量在 `types.rs` 中定义了，但在 `mod.rs` dispatch 表中没有对应的 `match` 分支，始终落入 `_ => ENOSYS`：

| 常量 | 编号 | 类别 | 建议 |
|------|------|------|------|
| `SYS_readahead` | 18 | 预读提示 | 无害 ENOSYS — 删除常量，让 `_` 处理 |
| `SYS_mremap` | 25 | 重映射 | 未来实现 — 保留常量但加 TODO 注释 |
| `SYS_semget` | 64 | SysV 信号量 | 非必须 — 删除（QX 用 POSIX 信号量） |
| `SYS_semop` | 65 | SysV 信号量 | 非必须 — 删除 |
| `SYS_clone` | 56 | 线程创建 | 未来实现 — 保留常量 |
| `SYS_vfork` | 58 | 已废弃 | 删除（POSIX 已废弃 vfork） |
| `SYS_getitimer` | 36 | ITIMER | 低优先级 — 保留常量 |
| `SYS_setitimer` | 38 | ITIMER | 低优先级 — 保留常量 |
| `SYS_link` | 86 | 硬链接 | 中期实现 — 保留常量 |
| `SYS_symlink` | 88 | 软链接 | 中期实现 — 保留常量 |
| `SYS_socketpair` | 53 | socket pair | 低优先级（仅 AF_UNIX） |
| `SYS_times` | 100 | 进程时间 | 低优先级 — 保留常量 |
| `SYS_fchown` | 93 | fd chown | 与 chown 重复 — 实现别名 |

**处理原则**：
- 已废弃的 POSIX ≥ 直接删除常量
- QX 不需要的 ≥ 直接删除常量
- 未来需要的 ≥ 保留常量 + 显式 `ENOSYS` 桩 + TODO 注释

### 1.3 问题二：30 个 QX 私有 syscall 的混杂现状

当前 400+ 编号已经做了正确的隔离（不与 POSIX 冲突），但存在以下结构性问题：

```
类别                 数量    问题
────────────────     ──     ──────────────────────────────
身份/认证/能力         17     重叠度高，token/grant/check_cap 功能碎片化
磁盘管理               6      合理，install wizard 专用
进程管理               3      可与 POSIX 合并（setpriority/sleep）
系统管理               5      gethostname/sethostname 已有 POSIX 等价
```

**具体冲突**：

| QX 私有 | POSIX 等价 | 重复？ |
|---------|-----------|--------|
| `SYS_QX_GETHOSTNAME (433)` | `uname()` 已包含 nodename | ⚠️ 冗余 |
| `SYS_QX_SETHOSTNAME (434)` | 无标准 POSIX（需 root） | 可保留 |
| `SYS_QX_PROC_SETPRI (431)` | `setpriority()` / `sched_setscheduler()` | 可合并 |
| `SYS_QX_PROC_SLEEP (432)` | `nanosleep()` 已实现 | **完全重复** |
| `SYS_QX_REBOOT (436)` | `reboot()` (Linux 169) | 可改用 POSIX 编号 |
| `SYS_QX_TOKEN_CREATE (408)` | 返回 `0` 桩 | **死代码** |
| `SYS_QX_TOKEN_USE (409)` | 返回 `0` 桩 | **死代码** |
| `SYS_QX_TOKEN_REVOKE (410)` | 返回 `0` 桩 | **死代码** |

### 1.4 问题三：Framebuffer — 非标准设备接口

```
SYS_FB_OPEN (450)
SYS_FB_MMAP (451)
SYS_FB_RELEASE (452)
```

Linux 的做法：fb 设备通过 `/dev/fb0` + `ioctl` 访问，不是独立 syscall。

---

## 2. 优化方案

### 2.1 第一层：POSIX 孤儿清理

| 操作 | 常量 | 理由 |
|------|------|------|
| **删除** | `SYS_readahead`, `SYS_vfork`, `SYS_semget`, `SYS_semop`, `SYS_socketpair` | 废弃或非必须 |
| **保留 + 显式桩** | `SYS_mremap`, `SYS_clone`, `SYS_getitimer`, `SYS_setitimer`, `SYS_link`, `SYS_symlink`, `SYS_times` | 未来需要，加 `// TODO: Phase N` |
| **实现别名** | `SYS_fchown` → 复用 `sys_chown` 逻辑 | fd→path 转换 |

### 2.2 第二层：QX 私有 syscall 收编

目标：400+ 区间只保留**确实没有 POSIX 等价物**的接口。

#### 2.2.1 删除（9 个）

| 删除 | 原因 |
|------|------|
| `SYS_QX_TOKEN_CREATE`, `SYS_QX_TOKEN_USE`, `SYS_QX_TOKEN_REVOKE` | 返回 `0` 的死桩 |
| `SYS_QX_PROC_SETPRI` | 未来改走 POSIX `setpriority(which=PRIO_PROCESS, who, prio)` |
| `SYS_QX_PROC_SLEEP` | 完全被 `SYS_nanosleep` 覆盖 |
| `SYS_QX_REBOOT` | 改为 POSIX `SYS_reboot=169` |
| `SYS_QX_GETHOSTNAME` | 被 `uname().nodename` 覆盖 |
| `SYS_QX_BOOT_CHECK` | install wizard 内部逻辑，非 syscall |
| `SYS_QX_HOTPLUG_STATUS` | PCI hotplug 消息应走 `/dev/hotplug` + `read()` |

#### 2.2.2 保留并重分类（21 个）

```
区间 400-409: 身份认证 (Identity)
─────────────────────────────────
  SYS_QX_LOGIN           400  登录
  SYS_QX_LOGOUT          401  登出
  SYS_QX_CREATE_IDENTITY 402  创建身份
  SYS_QX_DELETE_IDENTITY 403  删除身份
  SYS_QX_IDENTITY_INFO   404  查询身份
  SYS_QX_CHANGE_PASSWORD 405  修改密码
  SYS_QX_VERIFY_PASSWORD 406  验证密码
  SYS_QX_CREATE_FIRST    407  首次创建 (genesis)

区间 410-419: 能力管理 (Capability)
─────────────────────────────────
  SYS_QX_GRANT           411  授予能力
  SYS_QX_REVOKE          412  撤销能力
  SYS_QX_CHECK_CAP       413  检查能力
  SYS_QX_GET_CAPS        414  获取能力清单
  SYS_QX_GET_PWM         415 → get_current_pwm
  SYS_QX_SET_PWM         416 → set_current_pwm

区间 420-429: 磁盘/安装 (Disk / Install)
─────────────────────────────────
  SYS_QX_DISK_LIST       420
  SYS_QX_DISK_INFO       421
  SYS_QX_DISK_FORMAT     422
  SYS_QX_DISK_PARTITION  423
  SYS_QX_DISK_INSTALL    424
  SYS_QX_FAT_FORMAT      425

区间 430-434: 进程/系统
───────────────────
  SYS_QX_PROC_LIST       430  进程列表
  SYS_QX_SETHOSTNAME     434  设置主机名 (保留：无 POSIX 等价)
```

#### 2.2.3 credo 重构后的重命名

Credo 落地后，`SYS_QX_*` 的身份/能力类 syscall 命名同步更新：

```
SYS_QX_LOGIN            → SYS_CREDO_LOGIN
SYS_QX_LOGOUT           → SYS_CREDO_LOGOUT
SYS_QX_CREATE_IDENTITY  → SYS_CREDO_IDENTITY_CREATE
SYS_QX_DELETE_IDENTITY  → SYS_CREDO_IDENTITY_DELETE
SYS_QX_IDENTITY_INFO    → SYS_CREDO_IDENTITY_INFO
SYS_QX_CHANGE_PASSWORD  → SYS_CREDO_PASSWORD_CHANGE
SYS_QX_VERIFY_PASSWORD  → SYS_CREDO_PASSWORD_VERIFY
SYS_QX_CREATE_FIRST     → SYS_CREDO_GENESIS
SYS_QX_GRANT            → SYS_CREDO_GRANT
SYS_QX_REVOKE           → SYS_CREDO_REVOKE
SYS_QX_CHECK_CAP        → SYS_CREDO_CAP_CHECK
SYS_QX_GET_CAPS         → SYS_CREDO_CAP_LIST
SYS_QX_GET_PWM          → SYS_CREDO_CURRENT_ID
SYS_QX_SET_PWM          → SYS_CREDO_SWITCH_ID
```

### 2.3 第三层：Framebuffer 路由决策

**方案 A（推荐）**：改为设备节点模型

```
删除: SYS_FB_OPEN, SYS_FB_MMAP, SYS_FB_RELEASE
新增: /dev/fb0 设备节点（devfs）
实现: fb_open → fd → fb_mmap 通过 mmap() syscall 的正常路径
接口: ioctl(fb_fd, FBIOGET_VSCREENINFO, ...) ← 标准 Linux fb API
```

**方案 B**：保留 450+ 并标注为临时

```
保留但标记: // QX extension: 待 /dev/fb0 设备模型完成后移除
```

**采纳**：方案 A（中期），方案 B（短期，Phase 1 不做行为变更）。

### 2.4 最终 syscall 数目

```
优化前: 109 个 (76 POSIX + 30 QX + 3 FB)
优化后:  84 个 (69 POSIX + 21 credo/disk + 3 FB 临时)

删除明细:
  - POSIX 孤儿删除: 5 个 (readahead, vfork, semget, semop, socketpair)
  - QX 冗余/死桩删除: 9 个 (token ×3, proc_setpri, proc_sleep, reboot,
                            gethostname, boot_check, hotplug_status)
  - QX→POSIX 归化: 1 个 (fchown 实现别名)
  - 净减: 15 个
```

---

## 3. QX 私有 syscall 判定标准

凡遇下列情况，一律**不收编**为 QX 私有 syscall：

| 条件 | 替代方案 |
|------|---------|
| POSIX 已有等价的 syscall | 实现对应 POSIX syscall |
| 可以通过 `/dev/` + `read/write/ioctl` 完成 | 走设备模型 |
| 可以通过 `/proc/` + `read` 完成 | 走 procfs |
| 可以通过 `/sys/` + `read/write` 完成 | 走 sysfs |
| 是安装引导的一次性逻辑 | 移入 init 进程用户态 |
| 是调试/诊断用途 | 移入 `sysctl` 或 procfs |
| 返回值为常量桩 | 直接删除 |

**只有同时满足以下三条才允许 400+ 编号**：

1. **无 POSIX 等价物** — 不与任何标准 syscall 语义重叠
2. **非设备交互** — 不是通过 open/read/write/ioctl 能完成的设备操作
3. **不是用户态可实现** — 需要内核态特权操作

---

## 4. 实施计划

### Phase 1: 清理（零行为变更，仅清垃圾）

| 步骤 | 操作 |
|------|------|
| 1.1 | 删除 `types.rs` 中 5 个废弃常量 |
| 1.2 | 删除 `mod.rs` dispatch 中 3 个死桩（token 系列） |
| 1.3 | 7 个保留常量加 `// TODO: Phase N — implement me` 注释 |
| 1.4 | `SYS_fchown` 实现为 `sys_chown(fd→path, ...)` 的包装 |
| 1.5 | `SYS_QX_PROC_SLEEP` 改为 `sys_nanosleep` 的别名 |

**验证**: `make test-unit` → ALL 255 PASS

### Phase 2: 重新编号（Credo 就位后）

| 步骤 | 操作 |
|------|------|
| 2.1 | 400+ 区间重新分配（身份 400-409, 能力 410-419, 磁盘 420-429） |
| 2.2 | `SYS_QX_*` → `SYS_CREDO_*` 命名批量替换 |
| 2.3 | `userlib` 侧常量同步更新 |
| 2.4 | 删除 9 个冗余 QX syscall |

### Phase 3: Framebuffer 设备化

| 步骤 | 操作 |
|------|------|
| 3.1 | 实现 `/dev/fb0` devfs 节点 |
| 3.2 | fb mmap 通过标准 `mmap()` syscall 实现 |
| 3.3 | 删除 `SYS_FB_*` 三个 syscall |

---

## 5. 与 Credo/DID 的接口衔接

Credo 重构完成后，完整 syscall 分类如下：

```
POSIX 区间 (0-399):
  ├── 文件 I/O           (0-8)      read/write/open/close/stat/fstat/lstat/poll/lseek
  ├── 内存管理           (9-12)     mmap/mprotect/munmap/brk
  ├── 信号               (13-15)    rt_sigaction/rt_sigprocmask/rt_sigreturn
  ├── 设备 I/O           (16)       ioctl
  ├── 文件操作           (21-33)    access/pipe/select/sched_yield/dup/dup2
  ├── 定时器             (35)       nanosleep
  ├── 进程               (39,57-63) getpid/fork/execve/exit/wait4/kill/uname
  ├── 网络               (41-55)    socket/connect/accept/...  (feature: net)
  ├── 文件/目录          (72-93)    fcntl/truncate/getdents/chdir/mkdir/...
  ├── 用户/组            (102-112)  getuid/getgid/geteuid/getegid/getppid/setsid
  ├── 文件系统           (162-170)  sync/fsync/mount/umount2
  └── 杂项               (186-234)  gettid/time/clock_gettime/exit_group/tgkill

Credo 私有区间 (400-429):
  ├── 身份认证  400-409: login/logout/create/delete/info/changepw/verify/genesis
  ├── 能力管理  410-419: grant/revoke/check/list/current_id/switch_id
  └── 磁盘管理  420-429: disk_list/info/format/partition/install/fat_format

进程列表 (430):
  └── SYS_CREDO_PROC_LIST  430  (无 POSIX 等价)

系统管理 (431-434):
  └── SYS_CREDO_SETHOSTNAME  434  (无 POSIX 等价)

Framebuffer (临时, 450-452):
  └── FB_OPEN/FB_MMAP/FB_RELEASE  → 待 /dev/fb0 设备模型迁移
```

---

## 6. 决策记录

| 决策 | 采纳 | 理由 |
|------|------|------|
| 废弃 POSIX 常量的去留 | 删除 readahead/vfork/semget/semop/socketpair | 永远不会在 QX 上实现 |
| token 系列 syscall | 删除 | 三行返回 0 的桩代码，无调用者 |
| QX_PROC_SLEEP vs nanosleep | 删除 QX 版 | 完全重复 |
| reboot 的编号 | 从 436 → 169 (POSIX) | POSIX 已有标准编号 |
| Framebuffer 架构 | 中期迁往 /dev/fb0，短期保留 450+ | 不阻塞 credo 重构 |
| QX → CREDO 前缀重命名 | Phase 2 执行 | 与 credo 模块重构同步 |
