# AntX 系统审计报告 — 2026-05-09

> **审查范围**: PWID (13 文件) + 文件系统 (10 文件) + 系统调用 + 启动路径
> **发现问题**: 28 (PWID) + 16 (FS) + 13 (Syscall) + 12 (Boot) = **69 个**
> **严重程度**: 🔴28 / 🟠19 / 🟡14 / 🔵5 / 🟢11

---

## 一、PWID v4 安全与代码审计 (28 个)

---

## 深度排查：系统性可用障碍

以下是**追踪实际调用链**后发现的、会阻止系统正常运行的缺陷。编号接代码审计的 #22。

### 能力语义反置（最高优先级）

**#23. 权限模型的核心语义是反的 — "零能力 = 全通"**

这不是一个 bug——是**整个能力模型的基础语义错误**。追溯发现，三个关键路径都执行了这个错误语义：

| 调用路径 | 文件:行 | 逻辑 |
|---------|--------|------|
| VFS → `pwid_enhanced_check` | `ffi.rs:L749-L755` | `caps==0 → return 1 (Allowed)` |
| HvFS → `check_permission` | `hvfs.rs:L817-L820` | `caps==0 → return true` |
| RamFS → `check_permission` | `ramfs.rs` | 同上模式 |

这意味着：
- 新注册的身份默认无能力 → **但是无能力的身份拥有所有权限**
- 一旦给身份设置能力 → 反而**开始被限制**
- 攻击方式：直接传 `pwid=0` 或任何未注册的值 → 全通

**影响范围**：整个文件系统、用户管理、设备操作——所有经过这些检查的操作全部对未认证请求开放。

正确语义应该是：`caps==0 → Denied`，然后通过 First Token 创建的初始身份拥有全能力掩码。

### 调用链正确但结果为否的路径

**#24. syscall.c 的 12 个 `pwid_has_cap_raw` 检查全部会拒绝非全能力身份**

- `pwid_has_cap_raw` → `pwid_get_capability_raw` → FFI → Rust → 正常
- 但每个检查都以 `CAP_DOMAIN_SYS_ADMIN = 0xFFFFFFFFFFFFFFFF` 作为参数
- 这等价于旧的 `pwid_is_root()`——只有全能力掩码身份能通过
- 如果 First Identity 创建成功（`all_caps = [u64::MAX;16]`）→ 它可以通过
- 如果 First Identity 从未创建（无持久化）→ **全部 12 个管理员操作对所有人都不可用**

这是一个架构性问题——当前没有"部分 sys_admin"的概念，管理操作是二元的。

### 功能路径未实现的

**#25. First Token / Genesis 路径存在于代码中但触发路径模糊**

- `create_first_identity()` 确实创建全能力身份
- `pwid_any_identity_exists()` 存在
- 但 `main.c` 中只调了 `pwid_init()`，没有 `if !any_identity_exists() { request_genesis() }` 的逻辑
- 实际效果：内核启动后 PWID 表为空，必须用户态主动调用 `SYS_AUTH_CREATE_FIRST`——**但用户态程序自身就在 Ring 3，它如何引导第一个特权身份？**

这是一个自举问题（bootstrap problem）：系统没有身份 → 受限身份无法创建特权身份 → 永远无法启动。

**#26. 持久化不存在 → 每次重启都回到自举问题**

`storage.rs` 完全是 TODO。所有 PWID 在内存中，重启消失。当前测试能通过是因为测试框架在每次启动时重新注册 PWID——但那是测试代码，不是生产代码。

---

## 补充代码审计发现

### #27. `manager.create_internal` 仍然调用 `PwidLevel::Root`

- `manager.rs:L385`: `self.create_internal(password, "root", PwidLevel::Root.as_u8(), &all_caps)`
- v4 已经废弃了 Root 概念——CapabilityMatrix 是唯一权限来源
- 这里继续使用 `PwidLevel::Root` 更像是 v3 遗留代码

### #28. `genesis_init` 和 `GENESIS_REQUESTED` 从未被调用

- `ffi.rs:L44`: `static GENESIS_REQUESTED: AtomicBool` — 声明了
- `ffi.rs:L669`: 设置为 `true` — 在某次调用中
- 但整个代码库中没有一个函数读取或检查这个标志的实际值

---

## 完整问题清单

| # | 级别 | 文件 | 行号 | 简述 |
|---|:--:|------|------|------|
| 1 | 🔴 | ffi.rs | L749-L755 | 未注册 PWID 全通 |
| 2 | 🔴 | session.rs | L128 | 密码比较非常数时间 |
| 3 | 🔴 | manager.rs | generate() | 密码哈希不加盐 |
| 4 | 🔴 | manager.rs | generate() | PWID 只有 60 位熵 |
| 5 | 🟠 | manager.rs | L345 | can_modify() 永真 |
| 6 | 🟠 | permission.rs | L100-L101 | 信任等级检查空体 |
| 7 | 🟠 | token.rs | L290 | 紧缩保留过期 token |
| 8 | 🟠 | session.rs | elevate() | 提权栈无锁 |
| 9 | 🟡 | ffi.rs | L19-L41 | 单例 TOCTOU |
| 10 | 🟡 | manager.rs | 多处 | 锁获不一致 |
| 11 | 🔵 | storage.rs | 全文 | 持久化缺失 |
| 12 | 🔵 | session.rs | — | 无超时清理 |
| 13 | 🔵 | token.rs | L196-L199 | 满表静默失败 |
| 14 | 🔵 | ffi.rs | L681-L767 | 信任关系 Stub |
| 15 | 🔵 | audit.rs + manager | 多处 | 审计不完整 |
| 16 | 🟢 | audit.rs | L44-L48 | Race condition |
| 17 | 🟢 | token.rs | — | ID 回卷 |
| 18 | 🟢 | trust_chain.rs | L194 | 递归无栈保护 |
| 19 | 🟢 | context.rs | L221 | 风险评分反向 |
| 20 | 🟢 | capability.rs | — | 常量散落两处 |
| 21 | 🟢 | ffi.rs | L651 | 空密码 elevate |
| 22 | 🟢 | ffi.rs | L44 | GENESIS 未消费 |
| **23** | **🔴** | **ffi/hvfs/ramfs** | **多处** | **"零能力=全通"语义反置** |
| 24 | 🟠 | syscall.c | 12处 | 管理操作全或无 |
| 25 | 🔴 | main.c + ffi.rs | — | 自举问题 |
| 26 | 🔴 | storage.rs | 全文 | 重启丢失=自举循环 |
| 27 | 🟠 | manager.rs | L385 | 遗留 Root 调用 |
| 28 | 🟢 | ffi.rs | L44+L669 | GENESIS 未消费 |

---

# 补充审计：文件系统 UX 缺陷

---

## 🔴 系统可用性 (会阻止正常运行)

### FS-1. `vfs_unlink` 对 RamFS 只 truncate，不释放 inode

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/vfs/ffi.rs` L227-L230 |
| **现象** | `vfs_unlink_internal` 对 ramfs 路径调用 `truncate(inode_num, 0, pwid)` — 仅将文件大小清零，不标记 inode 为 free，不移除目录条目 |
| **UX 影响** | 用户执行 `rm file` → 成功 → `ls` 仍然看到该文件 → inode 泄漏 → 256 个文件后 RamFS 满 |

### FS-2. `diskfs_read` 在 I/O 错误时填充零并声称成功

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/diskfs/diskfs.rs` L205-L240 |
| **现象** | `hvfs.read_file_data` 返回 None（I/O 错误）→ 不对调用者报错，用零填充缓冲区，返回 `bytes_to_read as i32`（成功） |
| **UX 影响** | 用户读文件 → 得到全零内容 → 以为文件就是空的 → 实际是磁盘坏了。**静默数据损坏** |

### FS-3. `diskfs_write` 忽略写入失败

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/diskfs/diskfs.rs` L242-L269 |
| **现象** | `hvfs.write_file_data` 返回值被丢弃，始终递增 offset 并声称写入成功 |
| **UX 影响** | 用户写文件 → "写入成功" → 磁盘满但数据没落盘 → 数据丢失。**静默数据丢失** |

### FS-4. `diskfs_mount` 未格式化磁盘静默退化为内存模式

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/diskfs/diskfs.rs` L88-L124 |
| **现象** | `HVFS_DISK_UNFORMATTED` → 记录日志 `"not formatted"`，然后**在内存模式挂载而不格式化**。没有通知调用者 |
| **UX 影响** | 用户以为数据写入磁盘，重启后全部丢失 |

### FS-5. `vfs_format_internal` 永远返回 -1

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/vfs/ffi.rs` L940-L943 |
| **现象** | 函数返回 -1，内部不做任何事 |
| **UX 影响** | 用户执行 `mkfs` → "失败" → 无法格式化磁盘 → 无磁盘可用 |

---

## 🟠 功能残缺

### FS-6. `vfs_chmod` / `vfs_chown` 是空操作 stubs

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/vfs/ffi.rs` L978-L985 |
| **现象** | 两个函数返回 0（成功）但实际不做任何事 |
| **UX 影响** | `chmod 755 file` → "成功" → 权限未改变 → 以为设置了权限 |

### FS-7. `vfs_rename` / `vfs_readdir` 仅支持 RamFS

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/vfs/ffi.rs` L743-L795, L477-L555 |
| **现象** | diskfs 路径返回 -1 |
| **UX 影响** | `mv`、`ls` 在磁盘文件系统上不可用 |

### FS-8. `vfs_seek` 的 SEEK_END (whence=2) 不可用

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/vfs/ffi.rs` L840-L860 |
| **现象** | `whence == 2` → 返回 -1 |
| **UX 影响** | `fseek(fd, 0, SEEK_END)` 永远失败 → 无法追加写、无法获取文件大小 |

### FS-9. `hvfs_unmount` 返回 0 但不做任何事

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/vfs/ffi.rs` L936-L938 |
| **现象** | 返回 0，不调用任何实际卸载逻辑 |
| **UX 影响** | 用户 `umount /mnt` → "成功" → 文件系统仍然挂载 |

### FS-10. `TEST_PWID` 魔数在生产代码中

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/vfs/ffi.rs` L9 — 6 处使用 |
| **现象** | `const TEST_PWID: u64 = 0x0020F45A8B978417` — 当 pwid=0 时作为回退值 |
| **UX 影响** | 未认证的操作被归于这个硬编码 PWID → 审计日志无法区分正常操作和 fallback |

### FS-11. `ramfs_mount` 清除所有数据

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/ramfs/ramfs.rs` L639-L698 |
| **现象** | 每次 mount 调用 `RamFsInode::new()` 重新初始化所有 inodes |
| **UX 影响** | `mount /mnt ramfs` → 之前 `/mnt` 中的所有文件消失 |

### FS-12. RamFS 最大 256 个 inode + 8MB 总容量

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/ramfs/ramfs.rs` L13 |
| **现象** | `RAMFS_MAX_INODES = 256`，块限制 2048 × 4096 = 8MB |
| **UX 影响** | 256 个文件后 `touch file` → 失败 → 无明确错误信息 |

---

## 🟡 边缘情况

### FS-13. RamFS `write` 权限用 `FS_CAP_CREATE` 而非 `FS_CAP_WRITE`

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/ramfs/ramfs.rs` L802 |
| **现象** | `ramfs_write` 检查 `FS_CAP_CREATE` 而非 `FS_CAP_WRITE` |
| **UX 影响** | 有一个"创建能力"的身份 → 可以写入任何已打开的文件。语义错位 |

### FS-14. `vfs_open` 的 O_CREAT 不支持 diskfs

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/vfs/ffi.rs` L117-L136 |
| **现象** | ramfs 有 O_CREAT 回退路径；diskfs 返回 -1 |
| **UX 影响** | `touch newfile` 在磁盘上不可用 |

### FS-15. `diskfs_open` O_CREAT 有 race condition

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/diskfs/diskfs.rs` L154-L161 |
| **现象** | 打开→立即关闭→解析路径 — 中间无锁 |
| **UX 影响** | 并发 open(O_CREAT) 可能创建重复文件 |

### FS-16. `diskfs.truncate` 忽略 pwid 参数

| 字段 | 值 |
|------|-----|
| **文件** | `src/fs/diskfs/diskfs.rs` L315-L318 |
| **现象** | 截断操作不做权限检查 |
| **UX 影响** | 任何身份可以截断任何文件 |

---

# 补充审计：系统调用 UX 缺陷

---

## 🔴 系统可用性

### SC-1. `sys_proc_create` 在页表分配失败时杀死父进程

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/syscall.c` L116-L121 |
| **现象** | `vmm_create_user_page_table() == 0` → `process_exit(1)` → 终止当前进程（父） |
| **UX 影响** | 父进程调用 fork → 内存不足 → **父进程被杀死**。应只清理子进程并返回 E_NOMEM |

### SC-2. `sys_fs_unmount` 返回 0 但不执行卸载

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/syscall.c` L769-L782 |
| **现象** | 权限检查通过 → `return 0` — 从未调用任何卸载函数 |
| **UX 影响** | `umount` → "成功" → 文件系统仍然挂载。**欺骗性成功** |

### SC-3. `sys_proc_exec` 丢弃 argv 和 envp

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/syscall.c` L131-L151 |
| **现象** | `(void)argv; (void)envp;` — 新进程得不到命令行参数 |
| **UX 影响** | 所有用户程序以为自己在无参数运行。`cat file.txt` → cat 不知道要读 file.txt |

### SC-4. 14 个 syscall stubs 全部返回 `E_PERM`

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/syscall.c` L230-L748 |
| **现象** | `brk/mmap/pipe/gettimeofday/ioctl` 等 14 个 syscall 全部返回 `E_PERM` |
| **UX 影响** | 用户被误导为"权限不足"——实际上是功能未实现 |

---

## 🟠 错误码语义错误

### SC-5. 5 处 `return -1` 与 `E_PERM` 冲突

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/syscall.c` L248, L264, L274, L352, L725 |
| **现象** | `-1 == E_PERM` — 传 NULL path → 返回 -1 → 用户看到"权限不足" |
| **UX 影响** | 用户以为无权访问，实际是传了无效参数。不可诊断 |

### SC-6. 7 处返回 `E_AUTH_NOROOT` (v4 已废弃 Root 概念)

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/syscall.c` L223-L687 |
| **现象** | 能力检查失败返回 `E_AUTH_NOROOT (-105)` — v4 没有 Root |
| **UX 影响** | 用户收到"无 Root 权限"错误，但系统不识别"Root"概念 → 困惑 |

### SC-7. `sys_auth_login` 成功返回 1 而非 0

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/syscall.c` L378-L381 |
| **现象** | 所有其他 syscall 成功返回 0 或正值(fd/pid)；login 返回 1 |
| **UX 影响** | 破坏"0=成功"约定 |

### SC-8. `sys_fs_fstat` 是 stub 但返回 `E_PERM`

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/syscall.c` L323-L327 |
| **现象** | `fstat` 是空桩，返回 `E_PERM` |
| **UX 影响** | 用户对"已打开"的文件调用 fstat → 告诉用户"无权" |

### SC-9. 无效 fd 的错误码不一致

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/syscall.c` L248, L264 |
| **现象** | close(-1) → E_PERM；close(99999) → 无上限检查直接传 VFS |
| **UX 影响** | 错误码不清晰；极大 fd 可能导致数组越界 |

---

## 🟡 用户接口不完备

### SC-10. 用户头文件缺少 10+ 个 syscall 的 wrapper

| 字段 | 值 |
|------|-----|
| **文件** | `src/include/user/syscall.h` |
| **现象** | chmod/chown/unlink/rename/token/trust 等宏存在但无法通过内联 wrapper 调用 |
| **UX 影响** | 用户必须手写 `syscallN()` — 类型不安全，易出错 |

### SC-11. 用户头文件缺失 token/trust/check 的宏

| 字段 | 值 |
|------|-----|
| **文件** | `src/include/user/syscall.h` |
| **现象** | SYS_AUTH_TOKEN_CREATE(51) 等在内部定义但用户头文件无 |
| **UX 影响** | 令牌系统已实现但用户无法访问 |

### SC-12. 内部/用户头文件符号名不一致

| 字段 | 值 |
|------|-----|
| **文件** | `src/include/syscall.h` vs `src/include/user/syscall.h` |
| **现象** | `SYS_PROC_EXEC` ≠ `SYS_PROC_EXECUTE`；`SYS_FS_SEEK` ≠ `SYS_FS_SEEK_OFFSET` 等 10 处 |
| **UX 影响** | 文档/日志中的名称与代码中的名称不同 |

### SC-13. 内部 syscall 表有编号冲突

| 字段 | 值 |
|------|-----|
| **文件** | `src/include/syscall.h` L74-L82 |
| **现象** | `SYS_NET_SOCKET(81)` 与 `SYS_IPC_SIGNAL(81)` 冲突 |
| **UX 影响** | 实现网络栈后 IPC 会坏；反之亦然 |

---

# 补充审计：启动/初始化 UX 缺陷

---

## 🔴 系统可用性

### BOOT-1. init 加载失败 → 静默挂起，用户看到黑屏

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/main.c` L73-L88 |
| **现象** | `start_user_init()` 失败 → `return` → 内核进入 `while(1) { hlt/poll }` 无限循环 |
| **UX 影响** | 屏幕全黑。用户无法知道 init 加载失败了。串口有日志但用户不看串口 |

### BOOT-2. RELEASE 模式无磁盘 → panic

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/smart_mount.c` L30-L52 |
| **现象** | Release → 无盘/未格式化 → `panic("RELEASE mode requires persistent storage!")` |
| **UX 影响** | 新虚拟机首次启动 → 黑屏 → 用户不知道发生了什么 |

### BOOT-3. RamFS 模式每次重启重复安装向导

| 字段 | 值 |
|------|-----|
| **文件** | `src/user/install/user_install.c` L424-L431 |
| **现象** | RamFS → `/.antx_installed` 重启消失 → 每次启动提示安装 |
| **UX 影响** | 用户每次启动都要完成 5 步安装流程 |

### BOOT-4. HVFS_DISK_VERSION_ERROR 静默降级为 RamFS

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/smart_mount.c` L10-L26 |
| **现象** | 磁盘版本不兼容 → `default: return -1` → 被解释为"无磁盘" → RamFS |
| **UX 影响** | 用户插入旧版本磁盘 → 数据全部不可见 → 系统不告知原因 |

---

## 🟠 误导性消息

### BOOT-5. MODULE_CHECK_VOID 总是打印 "initialized"

| 字段 | 值 |
|------|-----|
| **文件** | `src/include/module_check.h` L30-L33 |
| **现象** | 9 个模块无论初始化成功或失败，都打印 "XXX initialized" |
| **UX 影响** | 日志: "ATA Driver initialized" — 实际上 ATA 控制器可能没工作 |

### BOOT-6. 无网卡用 WARN 级别日志

| 字段 | 值 |
|------|-----|
| **文件** | `src/net/qx_net_init.c` L26-L28 |
| **现象** | `klog_net_warn("No NIC found, running without network")` |
| **UX 影响** | 开发环境无网卡 → WARN → 用户以为网络有问题 |

### BOOT-7. MSR 初始化失败 WARN + CPU 初始化失败 WARN

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/cpu.c` L669-L670, `src/kernel/main.c` L106 |
| **现象** | QEMU 无 KVM → MSR 失败 → WARN |
| **UX 影响** | 启动日志出现"CPU driver initialization failed" — 让用户以为硬件坏了 |

### BOOT-8. "SMP init failed" 在单核机器上措辞不当

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/main.c` L236-L241 |
| **现象** | 单核机器 → `smp_init()` 返回 0 → 日志 `"SMP init failed"` |
| **UX 影响** | 用户以为多核功能故障 |

### BOOT-9. 安装向导仅接受小写 `"yes"`

| 字段 | 值 |
|------|-----|
| **文件** | `src/user/install/user_install.c` L182 |
| **现象** | `confirm[0] == 'y' && confirm[1] == 'e' && confirm[2] == 's'` — "YES" 被拒绝 |
| **UX 影响** | 用户输入 "YES" → 安装取消。需要重新开始 |

---

## 🟡 硬编码脆弱性

### BOOT-10. 硬编码内存大小

| 字段 | 值 |
|------|-----|
| **文件** | `src/include/kernel.h` L17 |
| **现象** | `#define MEMORY_SIZE (512 * 1024 * 1024)` — 固定 512MB |
| **UX 影响** | 256MB 机器 → 无法启动；1GB 机器 → 浪费 512MB |

### BOOT-11. `build_user_init_bin_len` 手动维护

| 字段 | 值 |
|------|-----|
| **文件** | `src/user/embedded/user_init_bin.c` |
| **现象** | `build_user_init_bin_len = 34768` — 硬编码，需与 ELF 文件同步 |
| **UX 影响** | 更新 init 但忘记更新长度 → init 损坏但无人察觉 → 回到 BOOT-1 |

### BOOT-12. PCI 初始化被注释掉 (已知 Rust FFI crash)

| 字段 | 值 |
|------|-----|
| **文件** | `src/kernel/main.c` L226-L228 |
| **现象** | `/* PCI init 在 Rust FFI 路径有已知崩溃，跳过 */` |
| **UX 影响** | 需要 PCI 枚举的驱动无法正常工作
