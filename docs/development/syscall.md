# AntX 系统调用接口设计

> **实现状态说明**：本文档描述的系统调用接口设计，部分已实现，部分仍在开发中。具体实现状态见各系统调用说明。

## 一、设计概述

### 1.1 设计原则

- 简洁优先，减少系统调用数量
- 与 PWID 权限模型深度集成
- 参考 Linux 但不完全兼容
- 支持未来扩展

### 1.2 系统调用机制

AntX 使用软件中断实现系统调用：

```
┌─────────────────────────────────────────────────────────────┐
│                      系统调用机制                             │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  用户态                               内核态                  │
│                                                               │
│  ┌─────────┐                        ┌─────────────┐         │
│  │ syscall │   int 0x80             │ sys_call    │         │
│  │  #1     │ ─────────────────────▶ │  handler    │         │
│  └─────────┘                        └─────────────┘         │
│                                           │                  │
│                                           ▼                  │
│                                    ┌─────────────┐          │
│                                    │ sys_xxx()   │          │
│                                    │ 具体实现     │          │
│                                    └─────────────┘          │
│                                           │                  │
│                                           ▼                  │
│                                    ┌─────────────┐          │
│                                    │ 返回用户态  │          │
│                                    └─────────────┘          │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

### 1.3 调用约定

```
参数传递：
  syscall_num → rax (或 ax)
  arg1       → rdi
  arg2       → rsi
  arg3       → rdx
  arg4       → r10
  arg5       → r8
  arg6       → r9

返回值：
  rax: 返回值 (负数表示错误)
```

## 二、系统调用列表

### 2.1 进程管理

| 调用号 | 名称 | 描述 | 参数 | 实现状态 |
|--------|------|------|------|---------|
| 0 | proc_create | 创建进程 | - | ✅ 已实现 |
| 1 | proc_exec | 执行程序 | (path, argv, envp) | ✅ 已实现 |
| 2 | proc_exit | 退出进程 | (exit_code) | ✅ 已实现 |
| 3 | proc_wait | 等待子进程 | (pid, status) | ✅ 已实现 |
| 4 | proc_getid | 获取进程ID | - | ✅ 已实现 |
| 5 | proc_getppid | 获取父进程ID | - | ✅ 已实现 |
| 6 | proc_getpwid | 获取当前PWID | - | ✅ 已实现 |
| 7 | proc_setpwid | 设置当前PWID | (pwid) | ✅ 已实现 |
| 8 | proc_setpri | 调整进程优先级 | (inc) | ⏳ 未实现 |
| 9 | proc_yield | 放弃CPU | - | ✅ 已实现 |
| 10 | proc_sleep | 睡眠 | (milliseconds) | ⏳ 未实现 |

### 2.2 文件操作

| 调用号 | 名称 | 描述 | 参数 | 实现状态 |
|--------|------|------|------|---------|
| 20 | fs_open | 打开文件 | (path, flags, mode) | ✅ 已实现 |
| 21 | fs_close | 关闭文件 | (fd) | ✅ 已实现 |
| 22 | fs_read | 读取文件 | (fd, buf, count) | ✅ 已实现 |
| 23 | fs_write | 写入文件 | (fd, buf, count) | ✅ 已实现 |
| 24 | fs_seek | 移动文件指针 | (fd, offset, whence) | ✅ 已实现 |
| 25 | fs_stat | 获取文件状态 | (path, stat_buf) | ✅ 已实现 |
| 26 | fs_fstat | 获取文件状态(FD) | (fd, stat_buf) | ⏳ 未实现 |
| 27 | fs_chmod | 修改权限 | (path, mode) | ✅ 已实现 |
| 28 | fs_chown | 修改所有者 | (path, pwid) | ✅ 已实现 |
| 29 | fs_unlink | 删除文件 | (path) | ✅ 已实现 |
| 30 | fs_rename | 重命名 | (old, new) | ✅ 已实现 |
| 31 | fs_mkdir | 创建目录 | (path, mode) | ✅ 已实现 |
| 32 | fs_rmdir | 删除目录 | (path) | ✅ 已实现 |
| 33 | fs_readdir | 读取目录 | (fd, dirent) | ✅ 已实现 |

### 2.3 PWID 权限管理

| 调用号 | 名称 | 描述 | 参数 | 实现状态 |
|--------|------|------|------|---------|
| 40 | auth_login | 登录获取PWID | (password, note) | ✅ 已实现 |
| 41 | auth_logout | 注销会话 | - | ✅ 已实现 |
| 42 | auth_elevate | 临时提权 | (command) | ⏳ 未实现 |
| 43 | auth_create | 创建新PWID(Root) | (password, note, level) | ✅ 已实现 |
| 44 | auth_delete | 删除PWID(Root) | (target_pwid) | ✅ 已实现 |
| 45 | auth_list | 列出所有PWID(Root) | - | ✅ 已实现 |
| 46 | auth_info | 获取PWID信息 | (target_pwid) | ✅ 已实现 |
| 47 | auth_setnote | 修改备注 | (new_note) | ⏳ 未实现 |
| 48 | auth_changepw | 修改密码 | (old_pw, new_pw) | ✅ 已实现 |
| 49 | auth_verify | 验证密码 | (password) | ✅ 已实现 |

### 2.4 内存管理

| 调用号 | 名称 | 描述 | 参数 | 实现状态 |
|--------|------|------|------|---------|
| 60 | mem_brk | 设置进程结束地址 | (addr) | ⏳ 未实现 |
| 61 | mem_map | 内存映射 | (addr, length, prot, flags, fd, offset) | ⏳ 未实现 |
| 62 | mem_unmap | 取消内存映射 | (addr, length) | ⏳ 未实现 |
| 63 | mem_protect | 设置内存保护 | (addr, length, prot) | ⏳ 未实现 |

### 2.5 通信

| 调用号 | 名称 | 描述 | 参数 | 实现状态 |
|--------|------|------|------|---------|
| 80 | ipc_pipe | 创建管道 | (fd[2]) | ⏳ 未实现 |
| 81 | net_socket | 创建套接字 | (domain, type, protocol) | ⏳ 未实现 |
| 82 | net_bind | 绑定地址 | (sockfd, addr, addrlen) | ⏳ 未实现 |
| 83 | net_listen | 监听 | (sockfd, backlog) | ⏳ 未实现 |
| 84 | net_accept | 接受连接 | (sockfd, addr, addrlen) | ⏳ 未实现 |
| 85 | net_connect | 连接 | (sockfd, addr, addrlen) | ⏳ 未实现 |
| 86 | net_send | 发送数据 | (sockfd, buf, len, flags) | ⏳ 未实现 |
| 87 | net_recv | 接收数据 | (sockfd, buf, len, flags) | ⏳ 未实现 |
| 88 | net_shutdown | 关闭连接 | (sockfd, how) | ⏳ 未实现 |

### 2.6 系统信息

| 调用号 | 名称 | 描述 | 参数 | 实现状态 |
|--------|------|------|------|---------|
| 100 | env_getcwd | 获取当前目录 | (buf, size) | ✅ 已实现 |
| 101 | env_chdir | 改变目录 | (path) | ✅ 已实现 |
| 102 | fs_sync | 同步文件系统 | - | ✅ 已实现 |
| 103 | sys_reboot | 重启 | (cmd) | ⏳ 未实现 |
| 104 | sys_time | 获取时间 | - | ⏳ 未实现 |
| 105 | sys_info | 获取系统信息 | (buf) | ⏳ 未实现 |
| 106 | env_getvar | 获取环境变量 | (name) | ⏳ 未实现 |
| 107 | env_setvar | 设置环境变量 | (name, value, overwrite) | ⏳ 未实现 |
| 108 | sys_gethostname | 获取主机名 | (buf, size) | ✅ 已实现 |
| 109 | sys_sethostname | 设置主机名(Root) | (name, len) | ✅ 已实现 |

### 2.7 设备操作

| 调用号 | 名称 | 描述 | 参数 | 实现状态 |
|--------|------|------|------|---------|
| 120 | dev_ioctl | 设备控制 | (fd, cmd, arg) | ⏳ 未实现 |
| 121 | dev_read | 读取设备 | (fd, buf, n) | ⏳ 未实现 |
| 122 | dev_write | 写入设备 | (fd, buf, n) | ⏳ 未实现 |

## 三、核心系统调用详解

### 3.1 PWID 登录 (sys_pwid_login)

```c
// 原型
int64_t sys_pwid_login(const char *password, const char *note);

// 参数
//   password: 密码
//   note: 备注

// 返回值
//   成功: 新的会话 ID
//   失败: 负数错误码

// 示例
session_id = syscall(40, "mypassword", "日常使用");
```

### 3.2 PWID 临时提权 (sys_pwid_elevate)

```c
// 原型
int64_t sys_pwid_elevate(const char *command, const char **argv);

// 参数
//   command: 要执行的命令
//   argv: 命令行参数

// 返回值
//   成功: 0
//   失败: 负数错误码

// 流程
// 1. 验证原 Root 密码
// 2. 创建临时子进程 (PWID=原Root)
// 3. 执行命令
// 4. 命令结束，销毁临时进程
// 5. 恢复原 PWID

// 示例
syscall(42, "/sbin/useradd", "alice");
```

### 3.3 文件打开 (sys_open)

```c
// 原型
int64_t sys_open(const char *pathname, int flags, int mode);

// 参数
//   pathname: 文件路径
//   flags: 打开标志 (O_RDONLY, O_WRONLY, O_RDWR, O_CREAT, etc.)
//   mode: 创建文件时的权限

// 返回值
//   成功: 文件描述符
//   失败: 负数错误码

// 权限检查
// 1. 获取调用者的 PWID
// 2. 查找文件的 Inode
// 3. 检查 PWID 权限
// 4. 通过后返回 FD
```

### 3.4 进程创建 (sys_fork)

```c
// 原型
int64_t sys_fork(void);

// 返回值
//   父进程: 子进程 PID
//   子进程: 0
//   失败: 负数错误码

// 流程
// 1. 复制当前进程结构
// 2. 复制页表（COW）
// 3. 继承 session_id 和 pwid
// 4. 设置为 READY 状态
// 5. 返回
```

### 3.5 文件读取 - 双源输入支持 (2026-04-19 更新)

`sys_fs_read` (调用号 22) 支持从键盘和串口双源读取输入，适用于不同运行环境：

```c
// 原型
int64_t sys_fs_read(int fd, void *buf, uint64_t count);
```

**输入源优先级**:

```
┌─────────────────────────────────────────────┐
│           sys_fs_read(fd=0) 输入流程        │
├─────────────────────────────────────────────┤
│                                             │
│  ┌─────────────────┐                        │
│  │ keyboard_has_data? │──Yes──▶ keyboard_get_char() │
│  └────────┬────────┘                        │
│           │ No                              │
│           ▼                                 │
│  ┌─────────────────┐                        │
│  │ serial_has_data?  │──Yes──▶ serial_getc()      │
│  └────────┬────────┘                        │
│           │ No                              │
│           ▼                                 │
│  已有数据? ─Yes──▶ 返回已读数据             │
│     │ No                                    │
│     ▼                                       │
│  __asm__ volatile ("hlt")  // 等待中断       │
│     │                                       │
│     ▼ 重新循环                               │
│                                             │
└─────────────────────────────────────────────┘
```

**适用场景**:
- **QEMU 物理终端**: 键盘输入通过 PS/2 驱动获取
- **QEMU 串口终端 (`-serial stdio`)**: 输入通过串口 COM1 获取
- **混合环境**: 自动检测并使用可用的输入源

**实现文件**: `src/kernel/syscall.c`, `src/kernel/serial.c`

### 3.5 主机名操作

```c
// 获取主机名
int64_t sys_gethostname(char *buf, size_t size);

// 参数
//   buf: 存储主机名的缓冲区
//   size: 缓冲区大小

// 返回值
//   成功: 0
//   失败: 负数错误码

// 示例
char hostname[64];
syscall(108, hostname, 64);


// 设置主机名（仅 Root）
int64_t sys_sethostname(const char *name, size_t len);

// 参数
//   name: 新主机名
//   len: 主机名长度（最大 64 字符）

// 返回值
//   成功: 0
//   失败: 负数错误码 (EPWID_NOT_ROOT, EINVAL)

// 说明
//   - 主机名存储在 /etc/hostname（用户态文件）
//   - 内核不存储主机名，仅提供系统调用接口
//   - 需要 Root 权限
//   - 主机名长度不超过 64 字符
//   - 默认主机名为 "localhost"

// 示例
syscall(109, "my-antx", 7);
```

## 四、错误码

### 4.1 通用错误码

| 错误码 | 值 | 说明 |
|--------|-----|------|
| E_PERM | -1 | 操作不允许 |
| E_NOTFOUND | -2 | 文件不存在 |
| E_INTR | -4 | 系统调用被中断 |
| E_IO | -5 | I/O 错误 |
| E_BADFD | -9 | 错误文件描述符 |
| E_ACCES | -10 | 权限不足 |
| E_EXIST | -17 | 文件已存在 |
| E_NOTDIR | -20 | 不是目录 |
| E_ISDIR | -21 | 是目录 |
| E_NOMEM | -12 | 内存不足 |
| E_FAULT | -14 | 错误地址 |
| E_BUSY | -16 | 设备或资源忙 |
| E_INVAL | -22 | 无效参数 |
| E_RANGE | -34 | 结果超出范围 |

### 4.2 权限相关错误码

| 错误码 | 值 | 说明 |
|--------|-----|------|
| E_AUTH_INVALID | -100 | 无效的 PWID |
| E_AUTH_NOTFOUND | -101 | PWID 不存在 |
| E_AUTH_DISABLED | -102 | PWID 已禁用 |
| E_AUTH_EXPIRED | -103 | PWID 已过期 |
| E_AUTH_PWERR | -104 | 密码错误 |
| E_AUTH_NOROOT | -105 | 需要 Root 权限 |
| E_AUTH_DENY | -106 | 不允许的操作 |

## 五、安全检查

### 5.1 PWID 验证流程

```
系统调用入口
      │
      ▼
┌──────────────────┐
│ 获取调用者 PWID  │
└──────────────────┘
      │
      ▼
┌──────────────────┐
│ 检查是否需要     │
│ PWID 验证        │
└──────────────────┘
      │
   ┌──┴──┐
   ▼     ▼
  需要   不需要
   │     │
   ▼     ▼
┌──────────┐  继续执行
│ 验证PWID │
└──────────┘
      │
      ▼
┌──────────────────┐
│ 检查权限级别     │
│ (Root/Trustworthy/Untrustworthy)│
└──────────────────┘
      │
      ▼
┌──────────────────┐
│ 执行系统调用     │
└──────────────────┘
```

### 5.2 权限级别检查

```c
// 内核中的权限检查
int check_pwid_permission(int required_level) {
    uint64_t current_pwid = get_current_pwid();
    int current_level = get_pwid_level(current_pwid);
    
    if (current_level > required_level) {
        return -EPWID_NOT_ROOT;
    }
    return 0;
}
```

## 六、未来扩展

### 6.1 可扩展设计

- 系统调用号预留（1000+ 用于未来扩展）
- 动态系统调用注册（模块化支持）
- 兼容层（支持 Linux 系统调用）

### 6.2 可能的扩展调用

```c
// 进程相关
sys_clone()        // 轻量级进程创建
sys_execve()       // 执行程序(带环境)
sys_kill()         // 发送信号

// 文件相关
sys_fcntl()        // 文件控制
sys_flock()        // 文件锁
sys_mknod()        // 创建设备文件

// 内存相关
sys_madvise()      // 内存使用建议
sys_mincore()      // 检查页面驻留

// 其他
sys_prctl()        // 进程控制
sys_ptrace()       // 进程追踪
```
