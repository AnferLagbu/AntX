# QueenX POSIX 原生接口实现工程计划

> **核心原则**：内核能力层不动，syscall ABI 层全部替换为 POSIX 标准。QueenX 不做 "下一个 Linux"，而是像 FreeBSD/XNU 一样以自己的内核实现 POSIX 接口。

---

## 1. 当前状态审计

### 1.1 Syscall 链路全景

```
                    x86_64                           aarch64
                    ──────                           ───────
用户程序            shell/init/install               shell/init/install
                      │                                 │
用户态 lib           src/user/lib/src/sys.rs           (同一文件, cfg 分支)
                      │                                 │
syscall 门           asm!("int 0x80")                  asm!("svc #0")
  rax=num, rdi/rsi/   x0=num, x1-x4=args
  rdx/r10=args
                      │                                 │
内核入口             isr.asm:syscall_handler          exception.rs:handle_svc
                      │                                 │
分发器               syscall_dispatch_from_frame()     svc_handler()
                      └──────────┬──────────────────────┘
                                 │
                    syscall_dispatch(num, a0, a1, a2, a3)
                      │                                 │
                      ├─ sys_proc_* (12 个函数)          → proc/ffi.rs, proc/user_proc.rs
                      ├─ sys_fs_*   (13 个函数)          → fs/vfs/ffi.rs
                      ├─ sys_auth_* (10 个函数)          → pwid/ffi.rs
                      ├─ sys_mem_*  (4 个函数)           → mm/pmm, kmalloc
                      ├─ sys_net_*  (8 个函数)           → lwip FFI
                      ├─ sys_env_*  (多个函数)           → vfs, timer
                      └─ sys_disk_* (6 个函数, 仅x86_64) → ata FFI
```

关键文件清单：
| 文件 | 作用 |
|------|------|
| [src/kernel/syscall/types.rs](file:///home/anfer/Code/AntX/src/kernel/syscall/types.rs) | syscall 编号常量 + errno 枚举 (28 通用 + 7 认证) |
| [src/kernel/syscall/mod.rs](file:///home/anfer/Code/AntX/src/kernel/syscall/mod.rs) | 分发器 (59-162行) + 全部 syscall 实现函数 |
| [src/kernel/syscall/ffi.rs](file:///home/anfer/Code/AntX/src/kernel/syscall/ffi.rs) | C FFI 桥接层 |
| [src/user/lib/src/sys.rs](file:///home/anfer/Code/AntX/src/user/lib/src/sys.rs) | 用户态 syscall 封装 (31 个函数) |
| [src/user/lib/src/lib.rs](file:///home/anfer/Code/AntX/src/user/lib/src/lib.rs) | 用户态 lib 入口, 重导出 io/fs/str |
| [src/kernel/arch/aarch64/exception.rs](file:///home/anfer/Code/AntX/src/kernel/arch/aarch64/exception.rs#L259-L289) | aarch64 SVC 入口 |
| [src/kernel/boot/isr.asm](file:///home/anfer/Code/AntX/src/kernel/boot/isr.asm#L173-L199) | x86_64 int 0x80 入口 |
| [src/kernel/proc/ffi.rs](file:///home/anfer/Code/AntX/src/kernel/proc/ffi.rs#L722-L835) | fork 实现 (已存在, 未接入 dispatch) |

### 1.2 用户程序对 syscall 的实际调用

**axsh shell** (`src/user/axsh/src/commands/`)：

| 命令 | 调用的 syscall |
|------|---------------|
| `dir` | `fs_open` → `fs_readdir` → `fs_close` |
| `cd` | `env_chdir` |
| `pwd` | `env_getcwd` |
| `cat` | `fs_open` → `fs_read` → `fs_close` |
| `mkdir` | `fs_mkdir` |
| `touch` | `fs_open(O_CREAT\|O_WRONLY)` → `fs_close` |
| `del` | `fs_unlink` / `fs_rmdir` |
| `cp` | `fs_open`×2 → `fs_read`/`fs_write` → `fs_close`×2 |
| `mv` | `fs_rename` |
| `save` | `fs_open` → `fs_write` → `fs_close` |
| `sync` | `fs_sync` |
| `login` | `auth_login` |
| `logout` | `auth_logout` |
| `who` | `proc_get_pwid` |
| `passwd` | `auth_change_password` |
| `ps` | `proc_list` |
| `host` | `gethostname` / `sethostname` |
| `reboot`/`halt` | `reboot(0)` / `reboot(1)` |

**init** (`src/user/init/src/main.rs`)：
- 读取 fstab → `fs_open`/`fs_read`/`fs_close`
- 磁盘挂载 → `fs_mount`
- 启动 shell → `proc_exec`

**安装向导** (`src/user/install/src/wizard/`)：
- `probe.rs`: `disk_list` → `disk_info`
- `prepare.rs`: `disk_partition` → `fat_format` → `disk_format` → `boot_install`
- `mod.rs`: `fs_mount` / `fs_unmount`
- `deploy.rs`: `file_open` → `file_copy`
- `auth.rs`: `auth_create_first`
- `config.rs`: `sethostname` → `fs_mkdir` → `file_open`/`fs_write`
- `finish.rs`: `fs_sync` → `file_open`/`fs_write` → `reboot`

### 1.3 当前 Syscall 编号空间

```
 0-11:  进程管理 (SYS_PROC_*)
20-33:  文件系统 (SYS_FS_*)
40-57:  认证/PWID (SYS_AUTH_*)
60-63:  内存管理 (SYS_MEM_*)
   80:  IPC (SYS_IPC_PIPE)
81-88:  网络 (SYS_NET_*)
100-118: 环境/系统/磁盘 (SYS_ENV_*/SYS_DISK_*)
120-122: 设备 I/O (SYS_DEV_*)
```

### 1.4 已发现的问题

| 问题 | 严重度 | 文件/位置 |
|------|--------|----------|
| `sys_fork()` 已实现但未接入 dispatch | 高 | [proc/ffi.rs:726](file:///home/anfer/Code/AntX/src/kernel/proc/ffi.rs#L726) |
| 10 个 sys_* 函数已定义但未加入 match 分支 (死代码) | 中 | [mod.rs:947-1011](file:///home/anfer/Code/AntX/src/kernel/syscall/mod.rs#L947-L1011) |
| 网络/设备 syscall 不验证用户指针 | 高 | [mod.rs:854-941](file:///home/anfer/Code/AntX/src/kernel/syscall/mod.rs#L854-L941) |
| PWID fallback 硬编码 `0x0020F45A8B978417` | 中 | [mod.rs:234](file:///home/anfer/Code/AntX/src/kernel/syscall/mod.rs#L234) |
| x86_64 网络/磁盘 syscall 未在 aarch64 可用 | 中 | `#[cfg(target_arch = "x86_64")]` 守卫 |
| 用户态仅定义使用的 syscall 常量子集 (30/60) | 低 | [user lib sys.rs](file:///home/anfer/Code/AntX/src/user/lib/src/sys.rs) |

---

## 2. POSIX Syscall 编号映射表

采用 **x86_64 Linux syscall 编号约定** 作为 POSIX 基准（此约定在 aarch64 上不同，需架构适配）。

### 2.1 已实现且可直接映射

| POSIX # | POSIX 函数 | 当前实现 | 映射方式 |
|---------|-----------|---------|---------|
| 0 | `read` | `sys_fs_read` (22) | 改编号, 进入时检查 fd==0 特殊处理→保持 |
| 1 | `write` | `sys_fs_write` (23) | 改编号, fd<=2 特殊处理→保持 |
| 2 | `open` | `sys_fs_open` (20) | 改编号, flags 定义对齐 O_RDONLY=0/O_WRONLY=1/O_RDWR=2/O_CREAT=0o100→0o100 已对齐 |
| 3 | `close` | `sys_fs_close` (21) | 直接改编号 |
| 4 | `stat` | `sys_fs_stat` (25) | 改编号, stat 结构体需 POSIX 化 |
| 5 | `fstat` | 无 | 新增, 从 fd 查 VFS node 获取 stat |
| 8 | `lseek` | `sys_fs_seek` (24) | 改编号 |
| 12 | `brk` | `sys_mem_brk` (60, 存根) | 需完整实现 |
| 39 | `getpid` | `sys_proc_getid` (4) | 改编号 |
| 57 | `fork` | `sys_fork` (proc/ffi.rs, 已实现) | 接入 dispatch |
| 59 | `execve` | `sys_proc_exec` (1) | 改编号, 签名对齐: path, argv, envp |
| 60 | `exit` | `sys_proc_exit` (2) | 改编号 |
| 61 | `wait4` | `sys_proc_wait` (3) | 改编号, 增加 status 返回 |
| 79 | `getcwd` | `sys_env_getcwd` (100) | 改编号 |
| 80 | `chdir` | `sys_env_chdir` (101) | 改编号 |
| 82 | `rename` | `sys_fs_rename` (30) | 改编号 |
| 83 | `mkdir` | `sys_fs_mkdir` (31) | 改编号 |
| 84 | `rmdir` | `sys_fs_rmdir` (32) | 改编号 |
| 87 | `unlink` | `sys_fs_unlink` (29) | 改编号 |
| 102 | `getuid` | 新增 | 从 PWID session 查 posix_uid |
| 104 | `getgid` | 新增 | 从 PWID session 查 posix_gid |
| 110 | `getppid` | `sys_proc_getppid` (5) | 改编号 |
| 162 | `sync` | `sys_fs_sync` (102) | 改编号 |

### 2.2 已实现但需自适应映射

| POSIX # | POSIX 函数 | 当前实现 | 适配工作 |
|---------|-----------|---------|---------|
| 9 | `mmap` | `sys_mem_map` (61) | 改编号, mmap 语义更复杂: MAP_ANON/MAP_SHARED/prot/flags/offset |
| 10 | `mprotect` | `sys_mem_protect` (63, 存根) | 需完整实现 |
| 11 | `munmap` | `sys_mem_unmap` (62) | 改编号 |
| 41 | `socket` | `sys_net_socket` (81) | 改编号 |
| 42 | `connect` | `sys_net_connect` (85) | 改编号 |
| 43 | `accept` | `sys_net_accept` (84) | 改编号 |
| 44 | `sendto` | `sys_net_send` (86) | 改编号, 增加 dest_addr/dest_len 参数 |
| 45 | `recvfrom` | `sys_net_recv` (87) | 改编号, 增加 src_addr/src_len 参数 |
| 48 | `shutdown` | `sys_net_shutdown` (88) | 改编号 |
| 49 | `bind` | `sys_net_bind` (82) | 改编号 |
| 50 | `listen` | `sys_net_listen` (83) | 改编号 |

### 2.3 需要新增的 POSIX syscall

| POSIX # | POSIX 函数 | 难度 | 说明 |
|---------|-----------|------|------|
| 6 | `lstat` | 低 | 类似 stat 但不跟随符号链接 |
| 7 | `poll` | 中 | 需要等待队列机制 |
| 13 | `rt_sigaction` | 高 | 信号框架 |
| 14 | `rt_sigprocmask` | 高 | 信号掩码 |
| 15 | `rt_sigreturn` | 高 | 信号返回 |
| 16 | `ioctl` | 中 | 设备控制接口 |
| 21 | `access` | 低 | 文件存在性/权限检查 |
| 22 | `pipe` | 低 | 已实现 `sys_ipc_pipe` (80), 映射即可 |
| 23 | `select` | 中 | 需要等待队列 |
| 24 | `sched_yield` | 低 | `sys_proc_yield` (9) 映射 |
| 25 | `mremap` | 低 | 初始存根 E_NOSYS |
| 32 | `dup` | 低 | fd 表复制 |
| 33 | `dup2` | 低 | fd 表重映射 |
| 35 | `nanosleep` | 中 | 需要 sleep 队列 |
| 37 | `alarm` | 低 | 存根 |
| 54 | `ioctl` | 中 | 同 16, 某些系统上重复 |
| 56 | `clone` | 高 | fork 已实现, clone 可选 |
| 62 | `kill` | 中 | 进程信号发送 |
| 63 | `uname` | 低 | 填 utsname 结构 |
| 72 | `fcntl` | 中 | FD 标志位 (F_GETFL/F_SETFL/F_DUPFD) |
| 78 | `getdents` | 低 | readdir 已实现, 适配 dirent 结构 |
| 85 | `creat` | 低 | open(path, O_CREAT\|O_WRONLY\|O_TRUNC, mode) 封装 |
| 90 | `chmod` | 中 | 文件权限修改 (需与 PWID 协调) |
| 91 | `fchmod` | 中 | fd 版本 |
| 92 | `chown` | 中 | 文件所有者修改 |
| 96 | `gettimeofday` | 低 | timer 子系统 |
| 97 | `getrlimit` | 低 | 返回默认值 |
| 99 | `sysinfo` | 低 | 填 sysinfo 结构 |
| 105 | `setuid` | 低 | PWID 模型中禁止, 返回 EPERM (root 除外) |
| 106 | `setgid` | 低 | 同上 |
| 107 | `geteuid` | 低 | 返回 posix_uid |
| 108 | `getegid` | 低 | 返回 posix_gid |
| 165 | `mount` | 低 | `sys_fs_mount` (111) 映射, 加 PWID 鉴权 |
| 166 | `umount2` | 低 | 新增 |
| 186 | `gettid` | 低 | 返回 pid |
| 201 | `time` | 低 | 已有实现但未接入 |
| 228 | `clock_gettime` | 低 | timer 子系统 |
| 231 | `exit_group` | 低 | 同 exit |

### 2.4 QueenX 特有 syscall 的 POSIX 化处理

| 当前 QueenX syscall | 处理方式 |
|--------------------|---------|
| `SYS_PROC_GETPWID/SETPWID` (6-7) | 保留但映射到私有编号空间 (>400) |
| `SYS_PROC_SETPRI` (8) | 保留在私有空间 |
| `SYS_PROC_LIST` (11) | 保留在私有空间, 或通过 /proc fs 暴露 |
| `SYS_AUTH_LOGIN/LOGOUT/CREATE/...` (40-57) | 全部保留在私有空间, POSIX 无等效 |
| `SYS_DISK_LIST/INFO/FORMAT/PARTITION/INSTALL_GRUB` (113-117) | 保留在私有空间 |
| `SYS_FAT_FORMAT` (118) | 保留在私有空间 |
| `SYS_SETHOSTNAME` (109) | 映射到 POSIX sethostname(170) |
| `SYS_FS_MOUNT` (111) | 映射到 POSIX mount(165) |
| `SYS_REBOOT` (103) | 映射到 POSIX reboot(169) |

---

## 3. 实施阶段

### 阶段 0：准备工作 (基础设施, ~1天)

#### 3.0.1 新增 POSIX errno 完整枚举

文件：[src/kernel/syscall/types.rs](file:///home/anfer/Code/AntX/src/kernel/syscall/types.rs)

```rust
// 替换现有 SyscallError 枚举，改为 POSIX errno (正值)
#[repr(i32)]
pub enum Errno {
    EPERM = 1, ENOENT = 2, ESRCH = 3, EINTR = 4,
    EIO = 5, ENXIO = 6, E2BIG = 7, ENOEXEC = 8,
    EBADF = 9, ECHILD = 10, EAGAIN = 11, ENOMEM = 12,
    EACCES = 13, EFAULT = 14, ENOTBLK = 15, EBUSY = 16,
    EEXIST = 17, EXDEV = 18, ENODEV = 19, ENOTDIR = 20,
    EISDIR = 21, EINVAL = 22, ENFILE = 23, EMFILE = 24,
    ENOTTY = 25, ETXTBSY = 26, EFBIG = 27, ENOSPC = 28,
    ESPIPE = 29, EROFS = 30, EMLINK = 31, EPIPE = 32,
    EDOM = 33, ERANGE = 34, EDEADLK = 35, ENAMETOOLONG = 36,
    ENOLCK = 37, ENOSYS = 38, ENOTEMPTY = 39, ELOOP = 40,
    // ... 按需添加
}

// 返回值约定: -1 表示错误, errno 通过线程局部存储返回
// 或使用 Linux 风格: 返回 -errno
```

**决策点**：选择 Linux 风格（`return -EINVAL`）还是传统 POSIX 风格（`return -1; errno = EINVAL`）。建议采用 Linux 风格——简化实现，musl 原生支持。

#### 3.0.2 定义 POSIX syscall 编号表

新建文件：`src/kernel/syscall/posix_nums.rs`（或扩展 types.rs）

该文件包含全部目标 POSIX syscall 编号，按 x86_64 Linux 约定。同时保留 QueenX 私有 syscall 在 400+ 编号空间：

```rust
// === POSIX 标准 syscall (x86_64 Linux 约定) ===
pub const SYS_read: u64 = 0;
pub const SYS_write: u64 = 1;
pub const SYS_open: u64 = 2;
// ... (全部 POSIX syscall)

// === QueenX 私有 syscall (≥400, 不与 POSIX 冲突) ===
pub const SYS_QX_LOGIN: u64 = 400;
pub const SYS_QX_LOGOUT: u64 = 401;
pub const SYS_QX_CREATE_IDENTITY: u64 = 402;
pub const SYS_QX_CHANGE_PASSWORD: u64 = 403;
pub const SYS_QX_DISK_LIST: u64 = 410;
pub const SYS_QX_DISK_INFO: u64 = 411;
pub const SYS_QX_DISK_FORMAT: u64 = 412;
pub const SYS_QX_DISK_PARTITION: u64 = 413;
pub const SYS_QX_DISK_INSTALL_GRUB: u64 = 414;
pub const SYS_QX_FAT_FORMAT: u64 = 415;
pub const SYS_QX_PROC_LIST: u64 = 420;
pub const SYS_QX_PROC_SETPRI: u64 = 421;
pub const SYS_QX_GETPWID: u64 = 422;
pub const SYS_QX_SETPWID: u64 = 423;
```

#### 3.0.3 PWID 扩展：POSIX uid 映射

文件：[src/kernel/pwid/types.rs](file:///home/anfer/Code/AntX/src/kernel/pwid/types.rs#L133-L147)

在 `PwidEntry` 结构体中新增字段：

```rust
pub struct PwidEntry {
    // ... 现有字段 ...
    pub posix_uid: AtomicU32,    // 映射到 POSIX uid
    pub posix_gid: AtomicU32,    // 映射到 POSIX gid (默认 = posix_uid)
}
```

与需要修改的文件：
- [src/kernel/pwid/table.rs](file:///home/anfer/Code/AntX/src/kernel/pwid/table.rs#L155-L240): `create()` 中为新条目分配 uid（自增或基于 PWID hash）
- [src/kernel/pwid/engine.rs](file:///home/anfer/Code/AntX/src/kernel/pwid/engine.rs): 新增 `get_posix_uid(pwid) -> u32` 函数
- [src/kernel/pwid/session.rs](file:///home/anfer/Code/AntX/src/kernel/pwid/session.rs): login 后在 session 中缓存 posix_uid

#### 3.0.4 aarch64 syscall 编号适配

POSIX 在不同架构上有不同的 syscall 编号约定。需要建立架构适配层：

```rust
// src/kernel/syscall/arch_nums.rs
#[cfg(target_arch = "x86_64")]
mod arch_nums {
    pub const SYS_read: u64 = 0;
    pub const SYS_write: u64 = 1;
    pub const SYS_open: u64 = 2;
    // ... (x86_64 Linux 约定)
}

#[cfg(target_arch = "aarch64")]
mod arch_nums {
    // aarch64 Linux 约定 (与 x86_64 不同!)
    pub const SYS_read: u64 = 63;
    pub const SYS_write: u64 = 64;
    pub const SYS_open: u64 = 56; // openat 常用替代
    // ...
}
```

或者更简洁的方案：**在 dispatch 中使用标准化编号，由架构入口做映射**。

---

### 阶段 1：重写 syscall 分发器 (核心改造, ~2天)

#### 3.1.1 重构 `syscall_dispatch()`

文件：[src/kernel/syscall/mod.rs](file:///home/anfer/Code/AntX/src/kernel/syscall/mod.rs#L59-L162)

将当前 `match num { ... }` 从自定义编号改为 POSIX 编号。每个分支复用现有 `sys_*` 函数实现。

**改造后的 dispatch 结构**：

```rust
pub unsafe extern "C" fn syscall_dispatch(
    num: u64, a0: u64, a1: u64, a2: u64, a3: u64
) -> i64 {
    match num {
        // ============ 文件 I/O (0-2x) ============
        SYS_read          => sys_read(a0 as i32, a1 as *mut u8, a2),
        SYS_write         => sys_write(a0 as i32, a1 as *const u8, a2),
        SYS_open          => sys_open(a0 as *const c_char, a1 as i32, a2 as i32),
        SYS_close         => sys_close(a0 as i32),
        SYS_stat          => sys_stat(a0 as *const c_char, a1 as *mut c_void),
        SYS_fstat         => sys_fstat(a0 as i32, a1 as *mut c_void),
        SYS_lseek         => sys_lseek(a0 as i32, a1 as i64, a2 as i32),
        SYS_getdents      => sys_getdents(a0 as i32, a1 as *mut c_void, a2),

        // ============ 进程 (39-6x) ============
        SYS_getpid        => sys_getpid(),
        SYS_getppid       => sys_getppid(),
        SYS_fork          => sys_fork() as i64,
        SYS_execve        => sys_execve(a0 as *const c_char, a1 as *const *const u8, a2 as *const *const u8),
        SYS_exit          => sys_exit(a0 as i32),
        SYS_wait4         => sys_wait4(a0 as i32, a1 as *mut i32, a2 as i32, a3 as *mut c_void),
        SYS_kill          => sys_kill(a0 as i32, a1 as i32),

        // ============ 用户/组 (102-11x) ============
        SYS_getuid        => sys_getuid(),
        SYS_getgid        => sys_getgid(),
        SYS_geteuid       => sys_geteuid(),
        SYS_getegid       => sys_getegid(),

        // ============ 内存 (9-1x) ============
        SYS_mmap          => sys_mmap(a0, a1, a2 as i32, a3 as i32, /* 第5/6参数从栈 */),
        SYS_munmap        => sys_munmap(a0, a1),
        SYS_brk           => sys_brk(a0),
        SYS_mprotect      => sys_mprotect(a0, a1, a2 as i32),

        // ============ 文件系统操作 (79-9x) ============
        SYS_getcwd        => sys_getcwd(a0 as *mut c_char, a1),
        SYS_chdir         => sys_chdir(a0 as *const c_char),
        SYS_rename        => sys_rename(a0 as *const c_char, a1 as *const c_char),
        SYS_mkdir         => sys_mkdir(a0 as *const c_char, a1 as i32),
        SYS_rmdir         => sys_rmdir(a0 as *const c_char),
        SYS_unlink        => sys_unlink(a0 as *const c_char),
        SYS_access        => sys_access(a0 as *const c_char, a1 as i32),
        SYS_chmod         => sys_chmod(a0 as *const c_char, a1 as u32),
        SYS_chown         => sys_chown(a0 as *const c_char, a1 as u32, a2 as u32),
        SYS_sync          => sys_sync(),
        SYS_mount         => sys_mount(a0 as *const c_char, a1 as *const c_char, a2 as *const c_char, a3 as u64, a4 as *const c_void),
        SYS_umount2       => sys_umount2(a0 as *const c_char, a1 as i32),

        // ============ 网络 (41-5x) ============
        SYS_socket        => sys_socket(a0 as i32, a1 as i32, a2 as i32),
        SYS_connect       => sys_connect(a0 as i32, a1 as u64, a2 as u32),
        SYS_accept        => sys_accept(a0 as i32, a1 as u64, a2 as u64),
        SYS_sendto        => sys_sendto(a0 as i32, a1 as u64, a2 as u32, a3 as i32, /* dest_addr, addrlen from stack */),
        SYS_recvfrom      => sys_recvfrom(a0 as i32, a1 as u64, a2 as u32, a3 as i32, /* src_addr, addrlen from stack */),
        SYS_shutdown      => sys_shutdown(a0 as i32, a1 as i32),
        SYS_bind          => sys_bind(a0 as i32, a1 as u64, a2 as u32),
        SYS_listen        => sys_listen(a0 as i32, a1 as i32),

        // ============ 其他 POSIX ============
        SYS_fcntl         => sys_fcntl(a0 as i32, a1 as i32, a2),
        SYS_dup            => sys_dup(a0 as i32),
        SYS_dup2           => sys_dup2(a0 as i32, a1 as i32),
        SYS_pipe           => sys_pipe(a0 as *mut i32),
        SYS_ioctl          => sys_ioctl(a0 as i32, a1 as u64, a2 as u64),
        SYS_sched_yield    => sys_sched_yield(),
        SYS_nanosleep      => sys_nanosleep(a0 as u64, a1 as u64),
        SYS_uname          => sys_uname(a0 as *mut c_void),
        SYS_gettimeofday   => sys_gettimeofday(a0 as *mut c_void, a1 as *mut c_void),
        SYS_clock_gettime  => sys_clock_gettime(a0 as i32, a1 as *mut c_void),
        SYS_time           => sys_time(a0 as *mut u64),

        // ============ 6-arg syscall (mmap/sendto/recvfrom) ============
        // 需要从用户栈读取第 5、6 参数, 由架构入口层处理

        // ============ QueenX 私有 syscall (4xx) ============
        SYS_QX_LOGIN             => sys_auth_login(a0 as *const c_char, a1 as *const c_char),
        SYS_QX_LOGOUT            => sys_auth_logout(),
        SYS_QX_CREATE_IDENTITY   => sys_auth_create(a0 as *const c_char, a1 as *const c_char, a2 as u8),
        SYS_QX_CHANGE_PASSWORD   => sys_auth_changepw(a0 as *const c_char, a1 as *const c_char),
        SYS_QX_DISK_LIST         => sys_disk_list(a0 as *mut u64, a1 as u32),
        SYS_QX_DISK_INFO         => sys_disk_info(a0 as u32, a1 as *mut u8),
        SYS_QX_DISK_FORMAT       => sys_disk_format(a0 as u32, a1 as *const c_char),
        SYS_QX_DISK_PARTITION    => sys_disk_partition(a0 as u32, a1),
        SYS_QX_DISK_INSTALL_GRUB => sys_boot_install(a0 as u32),
        SYS_QX_FAT_FORMAT        => sys_fat_format(a0 as u32),
        SYS_QX_PROC_LIST         => sys_proc_list(a0 as *mut u8, a1 as u32),
        SYS_QX_PROC_SETPRI       => sys_proc_setpri(a0 as u32, a1 as u32),
        SYS_QX_GETPWID           => sys_proc_getpwid(),
        SYS_QX_SETPWID           => sys_proc_setpwid(a0),

        _ => -(Errno::ENOSYS as i64),
    }
}
```

#### 3.1.2 调用链变更清单

| 影响文件 | 变更内容 |
|---------|---------|
| `src/kernel/syscall/types.rs` | 编号常量改为 POSIX + 新增 Errno 枚举 |
| `src/kernel/syscall/mod.rs` | dispatch match 全部替换; 函数重命名 (加 posix 后缀或改为 POSIX 名); 清理死代码 |
| `src/kernel/syscall/ffi.rs` | 不变 (仍然是 C 桥接层) |
| `src/user/lib/src/sys.rs` | syscall 门函数不变; wrapper 改为调用 POSIX 编号 |
| `src/user/lib/src/lib.rs` | 不变 |
| `src/user/axsh/src/commands/*.rs` | 逐个文件改为调用新的 wrapper |
| `src/user/init/src/main.rs` | 同上 |
| `src/user/install/src/wizard/*.rs` | 同上 |

---

### 阶段 2：实现缺失的 POSIX syscall (~3天, 按优先级)

优先实现 musl libc 启动和 busybox 运行所需的最小集：

```
优先级 P0 (musl 启动必需):
  brk, uname, readlink (NOENT存根)

优先级 P1 (busybox sh/ls/cat/echo 必需):
  fstat, getdents, ioctl(TTY), fcntl,
  dup, dup2, pipe, poll/select(存根),
  gettimeofday, clock_gettime, nanosleep(存根)

优先级 P2 (busybox 完整功能):
  access, chmod, chown, kill, sigaction(存根),
  getrlimit, sysinfo, utimes(存根)
```

#### 3.2.1 关键实现：`sys_brk()`

```rust
// 当前 sys_mem_brk 是存根，需完整实现
// brk(0) 返回当前 program break
// brk(addr) 设置新 break，按需分配页面
unsafe fn sys_brk(addr: u64) -> i64 {
    let pid = process_get_current_pid();
    // 查进程的 brk 地址，不足则调用 pmm_alloc_pages + vmm_map_page
    // ...
}
```

#### 3.2.2 关键实现：`sys_getdents()`

```rust
// 已有 sys_fs_readdir() 返回 VfsDirEntry
// getdents 返回 POSIX dirent 结构:
// struct dirent { ino_t d_ino; off_t d_off; u16 d_reclen; char d_name[]; }
```

#### 3.2.3 关键实现：`sys_fstat()` / `sys_stat()`

当前 `sys_fs_stat` 输出 `VfsStat` 结构，需转换为 POSIX `struct stat`。两者字段对齐：

```
VfsStat                   struct stat (POSIX)
───────                   ──────────────────
st_dev                    st_dev
st_ino                    st_ino
st_mode                   st_mode
st_nlink                  st_nlink
st_uid (PWID→uid映射)    st_uid
st_gid (PWID→gid映射)    st_gid
st_rdev                   st_rdev
st_size                   st_size
st_blksize                st_blksize
st_blocks                 st_blocks
st_atime/st_mtime/st_ctime st_atim/st_mtim/st_ctim
```

#### 3.2.4 需要修改的文件（新增 syscall 实现）

| syscall | 修改/新建文件 |
|---------|-------------|
| `fstat` | [mod.rs](file:///home/anfer/Code/AntX/src/kernel/syscall/mod.rs) (新增 `sys_fstat`), 调用 `vfs_fstat` |
| `getdents` | mod.rs (新增), 调用 `vfs_readdir` + dirent 转换 |
| `brk` | mod.rs (重写 `sys_mem_brk` 为 `sys_brk`) |
| `ioctl` | 新建 [src/kernel/syscall/ioctl.rs], 实现 TTY ioctl 子集 |
| `fcntl` | mod.rs (新增), 操作 `FdTable` |
| `dup/dup2` | mod.rs (新增), 操作 `FdTable` |
| `pipe` | mod.rs (新增), 调用 `ipc_pipe_create` |
| `uname` | mod.rs (新增), 填固定 utsname |
| `gettimeofday/clock_gettime` | mod.rs (新增), 调用 `timer_get_ticks` |
| `access` | mod.rs (新增), 调用 `vfs_stat` + PWID 检查 |
| `chmod/fchmod` | mod.rs (新增), 调用 VFS chmod |
| `chown/fchown` | mod.rs (新增), 调用 VFS chown |
| `kill` | 新建 [src/kernel/syscall/signal.rs], 信号框架 |
| `getuid/getgid/...` | mod.rs (新增), 调用 `pwid::engine::get_posix_uid` |
| `poll/select` | 新建 [src/kernel/syscall/poll.rs], 等待队列 |

---

### 阶段 3：移植 musl libc (~2天)

#### 3.3.1 目录结构

```
src/user/
├── musl/               # musl libc 源码 (git submodule 或 复制)
│   ├── arch/
│   │   └── antx/       # QueenX 架构适配
│   │       ├── syscall_arch.h   # __syscall 宏定义
│   │       └── bits/            # 架构相关头文件
├── lib/                # 现有的 Rust 用户态库 (保留, 供 Rust 程序使用)
├── busybox/            # busybox 源码 (可选 submodule)
├── init/               # 保留现有 Rust init
├── axsh/               # 保留现有 Rust shell
└── install/            # 保留现有 Rust 安装向导
```

#### 3.3.2 musl `__syscall` 宏

新建 `src/user/musl/arch/antx/syscall_arch.h`：

```c
// QueenX musl syscall 入口 — 双架构
#ifdef __x86_64__
// x86_64: int 0x80, rax=num, rdi=a1, rsi=a2, rdx=a3, r10=a4, r8=a5, r9=a6
static __inline long __syscall0(long n) {
    unsigned long ret;
    __asm__ volatile("int $0x80" : "=a"(ret) : "a"(n) : "memory");
    return ret;
}
static __inline long __syscall1(long n, long a1) { /* ... rdi=a1 ... */ }
static __inline long __syscall2(long n, long a1, long a2) { /* ... */ }
static __inline long __syscall3(long n, long a1, long a2, long a3) { /* ... */ }
static __inline long __syscall4(long n, long a1, long a2, long a3, long a4) {
    /* ... r10=a4 ... */
}
static __inline long __syscall5(long n, long a1, long a2, long a3, long a4, long a5) {
    /* ... r8=a5 ... */
}
static __inline long __syscall6(long n, long a1, long a2, long a3, long a4, long a5, long a6) {
    /* ... r9=a6 ... */
}
#define __SYSCALL_LL_E(x) (x)
#define __SYSCALL_LL_O(x) (x)

#elif defined(__aarch64__)
// aarch64: svc #0, x0=num, x1-x6=args, x0=返回
static __inline long __syscall0(long n) {
    register long x8 __asm__("x8") = n;
    register long x0 __asm__("x0");
    __asm__ volatile("svc #0" : "=r"(x0) : "r"(x8) : "memory");
    return x0;
}
// ... __syscall1-6 类似, x1-x6 传参
#define __SYSCALL_LL_E(x) (x)
#define __SYSCALL_LL_O(x) (x)
#endif
```

**关键决策**：QueenX 的 syscall ABI 使用不同寄存器约定（x86_64: rax=num/rdi/rsi/rdx/r10；aarch64: x0=num/x1-x4=args）。musl 默认期望 Linux 约定（x86_64: rax=num/rdi/rsi/rdx/r10/r8/r9；aarch64: x8=num/x0-x5=args）。需要**二选一**：

- **方案 A**：修改 musl 适配层使参数寄存器对齐现有 QueenX 约定（工作少）
- **方案 B**：修改内核入口使参数寄存器对齐 Linux 约定（未来兼容性好, musl/busybox 零改动）

**推荐方案 B**：修改 x86_64 `syscall_handler` 和 aarch64 `svc_handler` 使用 Linux 寄存器约定，这样 musl 和 busybox 可以直接编译链接。

#### 3.3.3 x86_64 入口修改

文件：[src/kernel/boot/isr.asm](file:///home/anfer/Code/AntX/src/kernel/boot/isr.asm#L173-L199)

当前 `syscall_dispatch_from_frame` 读 `frame->rax/rdi/rsi/rdx/r10`（5 个参数）。需扩展读 `frame->r8/r9`（第 5、6 参数）传到 dispatch。同时，aarch64 入口需要支持 x8=num 的约定。

#### 3.3.4 aarch64 入口修改

文件：[src/kernel/arch/aarch64/exception.rs](file:///home/anfer/Code/AntX/src/kernel/arch/aarch64/exception.rs#L407-L421)

改为 Linux 约定：

```rust
pub extern "C" fn svc_handler(frame: &mut ExceptionFrame) -> u64 {
    let syscall_num = frame.x8;           // Linux 约定: x8=syscall number
    let arg0 = frame.x0;                   // x0=arg0
    let arg1 = frame.x1;
    let arg2 = frame.x2;
    let arg3 = frame.x3;
    let arg4 = frame.x4;
    let arg5 = frame.x5;
    // ...
    // 返回值也写入 x0
    frame.x0 = result as u64;
    0
}
```

#### 3.3.5 构建集成

Makefile 中新增 musl 构建目标：

```makefile
MUSL_DIR = src/user/musl
MUSL_ARCH = antx

musl:
	cd $(MUSL_DIR) && ./configure --target=$(ARCH)-linux-musl --prefix=$(PWD)/build/musl
	$(MAKE) -C $(MUSL_DIR)

busybox: musl
	cd src/user/busybox && $(MAKE) CC=$(PWD)/build/musl/bin/musl-gcc defconfig
	# ...
```

---

### 阶段 4：用户态迁移 (~2天)

#### 4.1 Rust 用户态库保留

`src/user/lib/` 保留不动。它是 QueenX 的原生 Rust 运行时，现代 Rust 程序仍可使用。但 syscall 编号改为 POSIX。

文件：[src/user/lib/src/sys.rs](file:///home/anfer/Code/AntX/src/user/lib/src/sys.rs)

需要修改的内容：
1. `SYS_PROC_EXEC = 1` → `SYS_execve = 59`
2. `SYS_FS_OPEN = 20` → `SYS_open = 2`
3. ... (全部 31 个常量映射到 POSIX 编号)
4. 新增缺失的 wrapper: `getpid`, `fork`, `getuid`, `pipe`, `dup` 等

#### 4.2 QueenX 应用保持兼容

`init`/`axsh`/`install` 继续用 `src/user/lib/` 库，只需重新编译（syscall 编号变了但 wrapper 函数名不变）。

#### 4.3 新增 C 用户态示例

在 `src/user/` 下新增：

```
src/user/
├── hello/             # C hello world (用 musl)
│   ├── main.c
│   └── Makefile
└── test-posix/        # POSIX 兼容性测试
    ├── test_stat.c
    ├── test_fork.c
    └── Makefile
```

---

### 阶段 5：双架构验证 (~3天)

#### 5.1 测试矩阵

| 测试 | x86_64 | aarch64 | 通过标准 |
|------|--------|---------|---------|
| Rust init 启动 | ✓ | ✓ | 到达 axsh shell 登录 |
| axsh 所有命令 | ✓ | ✓ | dir/cd/cat/ls/... 全部正常 |
| 安装向导 | ✓ | ✓ | 磁盘检测→格式化→部署→重启 |
| C hello world (musl) | ✓ | ✓ | "Hello, World" 输出 |
| busybox sh | ✓ | ✓ | 交互式 shell |
| busybox 基础命令 | ✓ | ✓ | ls/cat/echo/cp/mv/rm/mkdir |
| fork 测试 | ✓ | ✓ | 父进程返回子 PID, 子进程返回 0 |
| execve 测试 | ✓ | ✓ | 加载并执行 ELF |
| getuid/getpid/uname | ✓ | ✓ | 正确返回值 |
| stat/fstat | ✓ | ✓ | 文件信息正确 |
| socket/bind/connect | ✓ | ✓ | 网络连通性 |
| PWID 权限检查 | ✓ | ✓ | 无权限操作被拒绝 |

#### 5.2 回归测试

确保以下预存功能不受影响：
- 栏栈 (Barrier-Stack) 恢复
- GICv3 中断 (aarch64)
- APIC 中断 (x86_64)
- lwIP 网络栈
- VFS 文件系统操作
- PMM/VMM 内存管理

---

## 4. 完整调用链变更图

### 4.1 改造前的调用链

```
用户态 (Rust)                    内核态
─────────────                    ──────
axsh login → auth_login()        int 0x80 → syscall_dispatch(40)
  sys.rs:sys2(SYS_AUTH_LOGIN,..)    match 40 → sys_auth_login(password, note)
                                       → pwid::ffi::pwid_login()
                                       → session::login()
                                   返回 PWID → frame.rax

axsh cat → fs_open() + fs_read()
  sys.rs:sys3(SYS_FS_OPEN,20,...) → syscall_dispatch(20)
                                       → sys_fs_open(path, flags, mode)
                                          → validate_user_ptr
                                          → pwid_get_current()
                                          → vfs_open(path, flags, pwid)
                                   返回 fd → frame.rax
  sys.rs:sys3(SYS_FS_READ,22,...)  → syscall_dispatch(22)
                                       → sys_fs_read(fd, buf, len)
                                   返回字节数 → frame.rax
```

### 4.2 改造后的调用链

```
用户态 (C/musl)                  内核态
───────────────                  ──────
busybox cat → musl open()        int 0x80 → syscall_dispatch(2)
  musl:__syscall3(SYS_open,...)     match 2 → sys_open(path, flags, mode)
                                       → (同一实现)
                                   返回 fd → rax

busybox cat → musl read()        int 0x80 → syscall_dispatch(0)
  musl:__syscall3(SYS_read,...)     match 0 → sys_read(fd, buf, len)
                                       → (同一实现)
                                   返回字节数 → rax

busybox cat → musl write()       int 0x80 → syscall_dispatch(1)
  musl:__syscall3(SYS_write,...)    match 1 → sys_write(fd, buf, len)
                                       → stdout 特殊处理→串口
                                       → vfs_write(fd, buf, len)
                                   返回字节数 → rax
```

### 4.3 改造前后文件影响范围

```
变更类型             涉及文件数   详情
─────────────────────────────────────────────────
syscall 编号常量重定义    1      types.rs (+ 新增 posix_nums.rs)
syscall 分发器重写        1      mod.rs (dispatch match + 函数重命名)
新增 syscall 实现        ~15    mod.rs 内新增函数 / 新建子模块
errno 重映射              1      types.rs (SyscallError→Errno)
PWID 扩展                 2      pwid/types.rs, pwid/engine.rs
架构入口修改              2      isr.asm, aarch64/exception.rs
用户态 lib 编号更新       1      user/lib/src/sys.rs
用户程序更新              3      init, axsh, install (重新编译)
musl 移植                 新建    src/user/musl/ (新增目录)
busybox 集成              新建    src/user/busybox/ (可选)
Makefile 更新             1      新增 musl/busybox 构建目标
```

---

## 5. PWID 与 POSIX 权限模型的协调设计

### 5.1 双轨制权限

```
┌──────────────────────────────────┐
│  POSIX ABI (用户可见)            │
│  uid/gid/mode/st_mode/st_uid     │
├──────────────────────────────────┤
│  权限翻译层 (新增)               │
│  uid → PWID 映射                 │
│  mode bits → CapDomain 检查       │
├──────────────────────────────────┤
│  PWID v5 引擎 (不变)             │
│  16域×64位能力矩阵               │
│  密码认证 / First Token / 审计   │
└──────────────────────────────────┘
```

### 5.2 权限检查流程

```
POSIX open("/etc/passwd", O_RDONLY)
  │
  ├─ 1. getuid() → posix_uid → 查 PWID entry
  ├─ 2. PWID engine::check(pwid, FS, FS_CAP_READ)?
  │        ├─ 检查 entry 是否存在, 是否 DISABLED
  │        └─ 检查 CAP_DOMAIN_FS(1) 是否有 FS_CAP_READ(bit 0)
  ├─ 3. VFS: 检查文件所有者 pwid == 当前 pwid? (等效 st_uid 检查)
  ├─ 4. VFS: 检查 mode bits (等效 st_mode 检查, 可选)
  └─ 5. 全部通过 → fd
```

### 5.3 策略决策点

| 决策 | 推荐 | 理由 |
|------|------|------|
| open() 权限检查点 | PWID engine + VFS owner check | PWID 能力矩阵是权威, VFS owner 是补充 |
| chmod/chown 权限 | 需要 FS_CAP_CHMOD/CHOWN 或 owner==当前 pwid | 兼容 POSIX 期望 |
| setuid() 行为 | 返回 EPERM (PWID 模型不允许切换身份) | 除非 privilege_level=0 (root) |
| root 用户定义 | privilege_level=0 的 PWID entry, posix_uid=0 | 创建时指定 |

---

## 6. 构建系统适配

### 6.1 Makefile 变更

```makefile
# 新增目标
MUSL_TOOLCHAIN = build/musl/bin/musl-gcc
BUSYBOX_BIN = build/user/busybox

musl: $(MUSL_TOOLCHAIN)

$(MUSL_TOOLCHAIN):
	cd src/user/musl && \
	./configure --target=$(ARCH_TARGET) --prefix=$(abspath build/musl) \
		--syslibdir=/lib CFLAGS="-D__ANTX__"
	$(MAKE) -C src/user/musl install

busybox: musl
	cp src/user/busybox/config .config
	$(MAKE) -C src/user/busybox CC=$(abspath $(MUSL_TOOLCHAIN))

# 确保 Rust lib 依赖更新后的用户程序
$(RUST_LIB): $(STAGE1_BIN) $(USER_INIT_ELF) $(USER_SHELL_ELF) $(USER_INSTALL_ELF)
```

### 6.2 用户程序加载路径

当前 init 通过 `include_bytes!("build/user/init.bin")` 嵌入。POSIX 化后，可以可选地嵌入 busybox：

```
$USER_PROGRAMS:
  - build/user/init.bin    (Rust init, 始终嵌入)
  - build/user/axsh.bin    (Rust shell, 可选)
  - build/user/busybox     (busybox, 存储在文件系统中加载)
```

---

## 7. 风险矩阵

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| syscall 编号冲突 (QueenX 私有 vs POSIX) | 低 | 高 | QueenX 私有 syscall 放在 400+, 与 POSIX 无交集 |
| aarch64 寄存器约定不一致 | 中 | 中 | 修改内核入口对齐 Linux 约定 |
| musl 依赖的 POSIX 特性比预期多 | 中 | 中 | 缺失的 syscall 返回 ENOSYS, 逐步实现 |
| 栏栈恢复后 syscall 状态不一致 | 低 | 高 | 栏栈回滚 undo log 只涉及内核模块, 不涉及用户 fd 表 |
| fork 后的内存压力 | 中 | 中 | 页表深拷贝 ON-WRITE 后续优化为 COW |
| 网络 syscall 的指针安全问题 | 高 | 中 | 阶段 1 中加入 `validate_user_buf` 检查 |
| 老用户态 Rust 程序与新 syscall 不兼容 | 高 | 高 | 同时更新 `src/user/lib/src/sys.rs` 的常量; 统一重新编译 |

---

## 8. 测试策略

### 8.1 单元测试

```rust
// src/kernel/syscall/tests.rs (新增)
#[test]
fn test_posix_errno_mapping() { /* ... */ }
#[test]
fn test_syscall_number_forwarding() { /* ... */ }
#[test]
fn test_user_ptr_validation() { /* ... */ }
```

### 8.2 集成测试

| 测试套件 | 内容 |
|---------|------|
| `test-posix-basic` | open/read/write/close/stat/getpid/getuid/uname |
| `test-posix-fork` | fork/exit/wait4 基础操作 |
| `test-posix-fs` | mkdir/rmdir/unlink/rename/chdir/getcwd |
| `test-posix-net` | socket/bind/listen/accept/connect/send/recv |
| `test-posix-busybox` | busybox sh + 20 个常用命令回归 |

### 8.3 回归测试

确保 x86_64 和 aarch64 的 QEMU boot 测试通过：
```bash
# x86_64
make iso && qemu-system-x86_64 -cdrom build/antx.iso -m 512M

# aarch64
make qemu-aarch64
```

---

## 9. 文件清单总览

### 需要修改的文件

| 文件 | 阶段 | 变更量 |
|------|------|--------|
| [src/kernel/syscall/types.rs](file:///home/anfer/Code/AntX/src/kernel/syscall/types.rs) | 0,1 | 高 (重写 enum, 新增 POSIX 编号) |
| [src/kernel/syscall/mod.rs](file:///home/anfer/Code/AntX/src/kernel/syscall/mod.rs) | 1,2 | 高 (重写 dispatch, 新增 syscall 实现) |
| [src/kernel/pwid/types.rs](file:///home/anfer/Code/AntX/src/kernel/pwid/types.rs) | 0 | 中 (新增 posix_uid/gid 字段) |
| [src/kernel/pwid/engine.rs](file:///home/anfer/Code/AntX/src/kernel/pwid/engine.rs) | 0 | 低 (新增 get_posix_uid 函数) |
| [src/kernel/pwid/table.rs](file:///home/anfer/Code/AntX/src/kernel/pwid/table.rs) | 0 | 中 (create 时分配 uid) |
| [src/kernel/boot/isr.asm](file:///home/anfer/Code/AntX/src/kernel/boot/isr.asm) | 3 | 低 (支持 6 参数) |
| [src/kernel/arch/aarch64/exception.rs](file:///home/anfer/Code/AntX/src/kernel/arch/aarch64/exception.rs) | 3 | 低 (改为 x8=num 约定) |
| [src/user/lib/src/sys.rs](file:///home/anfer/Code/AntX/src/user/lib/src/sys.rs) | 4 | 低 (编号常量更新) |
| [src/user/axsh/src/commands/*.rs](file:///home/anfer/Code/AntX/src/user/axsh/src/commands/) | 4 | 无 (重新编译即可) |
| [src/user/init/src/main.rs](file:///home/anfer/Code/AntX/src/user/init/src/main.rs) | 4 | 无 (重新编译即可) |
| [src/user/install/src/wizard/*.rs](file:///home/anfer/Code/AntX/src/user/install/src/wizard/) | 4 | 无 (重新编译即可) |
| [Makefile](file:///home/anfer/Code/AntX/Makefile) | 3,4 | 中 (新增 musl/busybox 目标) |

### 需要新建的文件

| 文件 | 阶段 | 用途 |
|------|------|------|
| `src/kernel/syscall/ioctl.rs` | 2 | TTY ioctl 实现 |
| `src/kernel/syscall/signal.rs` | 2 | kill/sigaction 信号框架 |
| `src/kernel/syscall/poll.rs` | 2 | poll/select 等待队列 |
| `src/user/musl/arch/antx/syscall_arch.h` | 3 | musl __syscall 适配 |
| `src/user/musl/arch/antx/bits/` | 3 | 架构头文件 |
| `src/user/hello/main.c` | 4 | C POSIX 示例 |
| `src/kernel/syscall/tests.rs` | 5 | 单元测试 |

---

## 10. 时间线总览

```
Week 1:
  Day 1-2  阶段 0: 基础设施 (errno, POSIX 编号, PWID uid 映射)
  Day 3-4  阶段 1: 重写 syscall 分发器
  Day 5    阶段 1 收尾 + 代码审查

Week 2:
  Day 1-3  阶段 2: 实现缺失的 POSIX syscall (P0+P1)
  Day 4-5  阶段 3: musl 移植 + busybox 集成

Week 3:
  Day 1-2  阶段 4: 用户态迁移 + C 示例
  Day 3-5  阶段 5: 双架构验证 + 回归测试 + 问题修复
```

---

## 11. 成功标准

1. **busybox sh 在 QueenX 上运行** — 交互式 shell, 所有常用命令正常
2. **musl 编译的 C 程序可运行** — `printf("Hello\n")` 正确输出
3. **双架构通过** — x86_64 和 aarch64 QEMU 均可 boot 到 busybox
4. **Rust 用户程序后向兼容** — init/axsh/install 重新编译后功能不变
5. **PWID 权限模型不受影响** — 认证/能力检查/审计日志正常工作
6. **栏栈恢复可用** — panic 恢复流程不受 syscall 层变更影响
