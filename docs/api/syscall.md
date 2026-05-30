# 系统调用接口

> AntX 用户态系统调用接口 — 采用 POSIX 标准编号，Credo 私有调用分配在 400+

---

## 调用机制

| 架构 | 指令 | 参考实现 |
|------|------|----------|
| x86_64 | `syscall` (优先) / `int 0x80` | [userlib sys.rs](../../src/user/lib/src/sys.rs) |
| aarch64 | `svc #0` | |

**参数寄存器** (x86_64):
| 寄存器 | 用途 |
|--------|------|
| RAX | 系统调用号 |
| RDI | 第1个参数 |
| RSI | 第2个参数 |
| RDX | 第3个参数 |
| R10 | 第4个参数 |
| R8 | 第5个参数 |
| RAX | 返回值 (负值 = -errno) |

---

## POSIX 标准 syscall

> 源码: [syscall/types.rs](../../src/kernel/syscall/types.rs)

| 编号 | 名称 | 功能 | 状态 |
|------|------|------|------|
| 0 | `read` | 文件读取 | ✅ |
| 1 | `write` | 文件写入 | ✅ |
| 2 | `open` | 打开文件 | ✅ |
| 3 | `close` | 关闭文件 | ✅ |
| 4 | `stat` | 获取文件状态 (路径) | ✅ |
| 5 | `fstat` | 获取文件状态 (fd) | ✅ |
| 6 | `lstat` | 获取符号链接状态 | ✅ (同 stat) |
| 7 | `poll` | I/O 多路复用 | ✅ |
| 8 | `lseek` | 文件定位 | ✅ |
| 9 | `mmap` | 内存映射 | ✅ |
| 10 | `mprotect` | 内存保护 | 🔴 ENOSYS |
| 11 | `munmap` | 解除内存映射 | ✅ |
| 12 | `brk` | 修改堆边界 | ✅ |
| 13 | `rt_sigaction` | 信号处理注册 | ✅ |
| 14 | `rt_sigprocmask` | 信号掩码 | ✅ |
| 15 | `rt_sigreturn` | 信号返回 | ✅ |
| 16 | `ioctl` | 设备控制 | ✅ |
| 21 | `access` | 文件访问检查 | ✅ |
| 22 | `pipe` | 创建管道 | ✅ |
| 23 | `select` | 同步 I/O 复用 | ✅ (同 poll) |
| 24 | `sched_yield` | 让出 CPU | ✅ |
| 25 | `mremap` | 重新映射 | 🔴 ENOSYS |
| 32 | `dup` | 复制文件描述符 | ✅ |
| 33 | `dup2` | 复制到指定 fd | ✅ |
| 34 | `nice` | 修改优先级 | ✅ |
| 35 | `nanosleep` | 纳秒级睡眠 | ✅ |
| 36 | `getitimer` | 获取定时器 | 🔴 ENOSYS |
| 37 | `alarm` | 设置闹钟 | 🔴 ENOSYS |
| 38 | `setitimer` | 设置定时器 | 🔴 ENOSYS |
| 39 | `getpid` | 获取进程 PID | ✅ |
| 41–55 | `socket`/`connect`/... | 网络 socket 族 | ✅ (feature=net) |
| 56 | `clone` | 创建线程 | 🔴 ENOSYS |
| 57 | `fork` | 创建进程 | ✅ |
| 59 | `execve` | 执行程序 | ✅ |
| 60 | `exit` | 退出进程 | ✅ |
| 61 | `wait4` | 等待子进程 | ✅ |
| 62 | `kill` | 发送信号 | ✅ |
| 63 | `uname` | 系统信息 | ✅ |
| 72 | `fcntl` | 文件描述符操作 | ✅ |
| 76 | `truncate` | 文件截断 (路径) | ✅ |
| 77 | `ftruncate` | 文件截断 (fd) | ✅ |
| 78 | `getdents` | 读取目录项 | ✅ |
| 79 | `getcwd` | 获取当前目录 | ✅ |
| 80 | `chdir` | 切换工作目录 | ✅ |
| 82 | `rename` | 文件重命名 | ✅ |
| 83 | `mkdir` | 创建目录 | ✅ |
| 84 | `rmdir` | 删除目录 | ✅ |
| 85 | `creat` | 创建文件 | ✅ (同 open) |
| 86 | `link` | 硬链接 | 🔴 ENOSYS |
| 87 | `unlink` | 删除文件 | ✅ |
| 88 | `symlink` | 符号链接 | 🔴 ENOSYS |
| 89 | `readlink` | 读取符号链接 | ✅ |
| 90 | `chmod` | 修改权限 (路径) | ✅ |
| 91 | `fchmod` | 修改权限 (fd) | ✅ |
| 92 | `chown` | 修改所有者 (路径) | ✅ |
| 93 | `fchown` | 修改所有者 (fd) | 🔴 ENOSYS |
| 95 | `umask` | 设置文件创建掩码 | ✅ |
| 96 | `gettimeofday` | 获取时间 | ✅ |
| 97 | `getrlimit` | 获取资源限制 | ✅ |
| 98 | `getrusage` | 获取资源使用 | ✅ |
| 99 | `sysinfo` | 系统统计 | ✅ |
| 100 | `times` | 进程时间 | 🔴 ENOSYS |
| 102 | `getuid` | 获取 UID | ✅ |
| 104 | `getgid` | 获取 GID | ✅ |
| 105 | `setuid` | 设置 UID | ✅ |
| 106 | `setgid` | 设置 GID | ✅ |
| 107 | `geteuid` | 获取有效 UID | ✅ |
| 108 | `getegid` | 获取有效 GID | ✅ |
| 110 | `getppid` | 获取父进程 PID | ✅ |
| 111 | `getpgid` | 获取进程组 | ✅ |
| 112 | `setsid` | 创建新会话 | ✅ |
| 113 | `seteuid` | 设置有效 UID | ✅ |
| 114 | `setegid` | 设置有效 GID | ✅ |
| 115 | `setreuid` | 设置真实/有效 UID | ✅ |
| 116 | `setregid` | 设置真实/有效 GID | ✅ |
| 140 | `getpriority` | 获取调度优先级 | ✅ |
| 141 | `setpriority` | 设置调度优先级 | ✅ |
| 162 | `sync` | 同步文件系统 | ✅ |
| 165 | `mount` | 挂载文件系统 | ✅ |
| 166 | `umount2` | 卸载文件系统 | ✅ |
| 170 | `fsync` | 同步单个文件 | ✅ |
| 186 | `gettid` | 获取线程 ID | ✅ |
| 201 | `time` | 获取时间戳 | ✅ |
| 228 | `clock_gettime` | 获取时钟时间 | ✅ |
| 231 | `exit_group` | 退出所有线程 | ✅ |
| 234 | `tgkill` | 线程信号 | ✅ |

---

## Credo 私有 syscall (400+)

> 不与 POSIX 编号冲突，用于 PWID 能力系统管理

| 编号 | 名称 | 功能 |
|------|------|------|
| 400 | `CREDO_LOGIN` | 登录 |
| 401 | `CREDO_LOGOUT` | 登出 |
| 402 | `CREDO_CREATE_IDENTITY` | 创建身份 |
| 403 | `CREDO_DELETE_IDENTITY` | 删除身份 |
| 404 | `CREDO_IDENTITY_INFO` | 查询身份信息 |
| 405 | `CREDO_CHANGE_PASSWORD` | 修改密码 |
| 406 | `CREDO_VERIFY_PASSWORD` | 验证密码 |
| 407 | `CREDO_CREATE_FIRST` | 创建首个 root 身份 |
| 411 | `CREDO_GRANT` | 授予能力 |
| 412 | `CREDO_REVOKE` | 撤销能力 |
| 413 | `CREDO_CHECK_CAP` | 检查能力 |
| 414 | `CREDO_GET_CAPS` | 获取能力列表 |
| 415 | `CREDO_GET_PWM` | 获取当前 PWM |
| 416 | `CREDO_SET_PWM` | 切换当前 PWM |
| 420 | `CREDO_DISK_LIST` | 列出磁盘 |
| 421 | `CREDO_DISK_INFO` | 磁盘详情 |
| 422 | `CREDO_DISK_FORMAT` | 格式化磁盘 |
| 423 | `CREDO_DISK_PARTITION` | 磁盘分区 |
| 424 | `CREDO_DISK_INSTALL` | 安装系统 |
| 425 | `CREDO_FAT_FORMAT` | FAT 格式化 |
| 430 | `CREDO_PROC_LIST` | 列出进程 |
| 431 | `CREDO_PROC_SETPRI` | 设置进程优先级 |
| 432 | `CREDO_PROC_SLEEP` | 进程睡眠 |
| 433 | `CREDO_GETHOSTNAME` | 获取主机名 |
| 434 | `CREDO_SETHOSTNAME` | 设置主机名 |
| 435 | `CREDO_BOOT_CHECK` | 启动自检 |
| 436 | `CREDO_REBOOT` | 重启系统 |
| 437 | `CREDO_HOTPLUG_STATUS` | 热插拔状态 |
| 438 | `CREDO_PROC_CPUTIME` | 进程 CPU 时间 |

## 帧缓冲区 syscall (450+)

| 编号 | 名称 | 功能 |
|------|------|------|
| 450 | `FB_OPEN` | 打开帧缓冲区 |
| 451 | `FB_MMAP` | 映射帧缓冲区 |
| 452 | `FB_RELEASE` | 释放帧缓冲区 |

---

## 错误码 (errno)

> 使用 Linux 风格: 系统调用返回 `-errno`

| 符号 | 编号 | 含义 |
|------|------|------|
| EPERM | 1 | 权限不足 |
| ENOENT | 2 | 文件/目录不存在 |
| ESRCH | 3 | 进程不存在 |
| EINTR | 4 | 系统调用被中断 |
| EIO | 5 | I/O 错误 |
| ENXIO | 6 | 设备不存在 |
| E2BIG | 7 | 参数列表过长 |
| ENOEXEC | 8 | 可执行文件格式错误 |
| EBADF | 9 | 无效文件描述符 |
| ECHILD | 10 | 无子进程 |
| EAGAIN | 11 | 资源暂时不可用 |
| ENOMEM | 12 | 内存不足 |
| EACCES | 13 | 拒绝访问 |
| EFAULT | 14 | 无效地址 |
| ENOTBLK | 15 | 非块设备 |
| EBUSY | 16 | 设备忙 |
| EEXIST | 17 | 文件已存在 |
| EXDEV | 18 | 跨设备链接 |
| ENODEV | 19 | 无此设备 |
| ENOTDIR | 20 | 非目录 |
| EISDIR | 21 | 是目录 |
| EINVAL | 22 | 无效参数 |
| ENFILE | 23 | 文件表溢出 |
| EMFILE | 24 | 文件描述符用尽 |
| ENOTTY | 25 | 非终端 |
| ETXTBSY | 26 | 文本文件忙 |
| ENOSYS | 38 | 系统调用未实现 |
