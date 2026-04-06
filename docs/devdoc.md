# AntX 系统调用接口命名优化：更精准的英文表达 + 贴合自研特色
## 一、核心优化原则
1. **语义精准**：用更贴合操作本质的英文单词，替代 Unix/Linux 历史遗留的模糊命名；
2. **统一风格**：所有接口采用「动作+对象」的动宾结构（如 `proc_create` → 保留，`fs_open` → 保留，`auth_login` → 优化），避免混合风格；
3. **弱化 Unix 影子**：替换 `fork`/`brk`/`chmod` 等强 Unix 印记的命名，同时保留开发者易理解的核心逻辑；
4. **贴合 AntX 特色**：结合「蚁巢」「PWID」等自研概念，让命名更有辨识度。

## 二、系统调用列表优化（按模块）
### 2.1 进程管理（Process Management）
| 原调用号 | 原名称 | 优化后名称 | 优化理由 |
|----------|--------|------------|----------|
| 0 | proc_create | `proc_create` | 保留（语义精准：create 明确「创建进程」） |
| 1 | proc_exec | `proc_execute` | execute 比 exec 更完整，符合动宾结构 |
| 2 | proc_exit | `proc_terminate` | exit 偏「退出」，terminate 更贴合「进程终止」的内核语义 |
| 3 | proc_wait | `proc_waitpid` | 保留（行业通用，无需修改，精准指向「等待指定PID」） |
| 4 | proc_getid | `proc_get_pid` | 下划线分隔，更清晰（动宾：get + pid） |
| 5 | proc_getppid | `proc_get_parent_pid` | 替换缩写 ppid 为完整 parent_pid，语义无歧义 |
| 6 | proc_getpwid | `proc_get_pwid` | 保留（贴合自研 PWID 体系） |
| 7 | proc_setpwid | `proc_set_pwid` | 保留（同上） |
| 8 | proc_setpri | `proc_set_priority` | 替换缩写 pri 为完整 priority，语义清晰 |
| 9 | proc_yield | `proc_yield_cpu` | 补充 cpu，明确「放弃CPU资源」，避免歧义 |
| 10 | proc_sleep | `proc_sleep_ms` | 补充 ms，明确「毫秒级睡眠」，语义更精准 |

### 2.2 文件操作（File Operations）
| 原调用号 | 原名称 | 优化后名称 | 优化理由 |
|----------|--------|------------|----------|
| 20 | fs_open | `fs_open` | 保留（行业通用，语义精准） |
| 21 | fs_close | `fs_close` | 保留（同上） |
| 22 | fs_read | `fs_read` | 保留（同上） |
| 23 | fs_write | `fs_write` | 保留（同上） |
| 24 | fs_seek | `fs_seek_offset` | 补充 offset，明确「移动文件偏移量」 |
| 25 | fs_stat | `fs_get_stat` | 动宾结构（get + stat），比单纯 stat 更清晰 |
| 26 | fs_fstat | `fs_get_stat_fd` | 补充 fd，区分「通过路径」和「通过文件描述符」获取状态 |
| 27 | fs_chmod | `fs_set_permissions` | chmod 是 Unix 缩写，set_permissions 语义更通用，贴合 PWID 权限体系 |
| 28 | fs_chown | `fs_set_owner` | chown 是 Unix 缩写，set_owner 更清晰，且贴合 PWID 作为「所有者」的设计 |
| 29 | fs_unlink | `fs_delete` | unlink 语义模糊（Unix 中是「解除链接」），delete 直接表达「删除文件」 |
| 30 | fs_rename | `fs_rename` | 保留（语义精准） |
| 31 | fs_mkdir | `fs_make_dir` | 替换缩写 mkdir 为完整 make_dir，风格统一 |
| 32 | fs_rmdir | `fs_remove_dir` | 替换缩写 rmdir 为完整 remove_dir，风格统一 |
| 33 | fs_readdir | `fs_read_dir` | 替换缩写 readdir 为完整 read_dir，风格统一 |

### 2.3 PWID 权限管理（PWID Authorization）
| 原调用号 | 原名称 | 优化后名称 | 优化理由 |
|----------|--------|------------|----------|
| 40 | auth_login | `auth_authenticate` | login 偏「登录」，authenticate 更贴合「验证密码获取 PWID 会话」的内核语义 |
| 41 | auth_logout | `auth_invalidate_session` | logout 偏用户态，invalidate_session 更精准表达「注销会话（内核态）」 |
| 42 | auth_elevate | `auth_elevate_privileges` | 补充 privileges，明确「提升权限」，语义完整 |
| 43 | auth_create | `auth_create_pwid` | 补充 pwid，明确操作对象（避免和 proc_create 混淆） |
| 44 | auth_delete | `auth_delete_pwid` | 补充 pwid，明确操作对象 |
| 45 | auth_list | `auth_list_pwids` | 补充 pwids（复数），明确「列出所有 PWID」 |
| 46 | auth_info | `auth_get_pwid_info` | 动宾结构，明确「获取 PWID 信息」 |
| 47 | auth_setnote | `auth_set_pwid_note` | 补充 pwid，明确操作对象 |
| 48 | auth_changepw | `auth_change_pwid_password` | 替换缩写 changepw 为完整 change_pwid_password，语义无歧义 |
| 49 | auth_verify | `auth_verify_pwid_password` | 补充 pwid_password，明确「验证 PWID 密码」 |

### 2.4 内存管理（Memory Management）
| 原调用号 | 原名称 | 优化后名称 | 优化理由 |
|----------|--------|------------|----------|
| 60 | mem_brk | `mem_set_break_addr` | brk 是 Unix 历史命名（模糊），set_break_addr 明确「设置进程地址空间结束地址」 |
| 61 | mem_map | `mem_map_region` | 补充 region，明确「映射内存区域」，语义完整 |
| 62 | mem_unmap | `mem_unmap_region` | 补充 region，和 mem_map_region 对应 |
| 63 | mem_protect | `mem_set_protection` | set_protection 比 protect 更贴合「设置内存保护属性」的操作本质 |

### 2.5 通信（Interprocess/Network Communication）
| 原调用号 | 原名称 | 优化后名称 | 优化理由 |
|----------|--------|------------|----------|
| 80 | ipc_pipe | `ipc_create_pipe` | 补充 create，明确「创建管道」，动宾结构 |
| 81 | net_socket | `net_create_socket` | 补充 create，明确「创建套接字」，动宾结构 |
| 82 | net_bind | `net_bind_socket` | 补充 socket，明确操作对象 |
| 83 | net_listen | `net_listen_socket` | 补充 socket，明确操作对象 |
| 84 | net_accept | `net_accept_connection` | 补充 connection，明确「接受网络连接」，语义更精准 |
| 85 | net_connect | `net_connect_socket` | 补充 socket，明确操作对象 |
| 86 | net_send | `net_send_data` | 补充 data，明确「发送数据」，避免歧义 |
| 87 | net_recv | `net_receive_data` | receive 比 recv 更完整，补充 data 明确操作对象 |
| 88 | net_shutdown | `net_shutdown_socket` | 补充 socket，明确操作对象 |

### 2.6 系统信息（System Information）
| 原调用号 | 原名称 | 优化后名称 | 优化理由 |
|----------|--------|------------|----------|
| 100 | env_getcwd | `env_get_current_dir` | 替换缩写 cwd 为完整 current_dir，语义清晰 |
| 101 | env_chdir | `env_set_current_dir` | chdir 是 Unix 缩写，set_current_dir 更通用 |
| 102 | fs_sync | `fs_sync_all` | 补充 all，明确「同步所有文件系统」 |
| 103 | sys_reboot | `sys_reboot_system` | 补充 system，明确操作对象 |
| 104 | sys_time | `sys_get_system_time` | 补充 get/system，明确「获取系统时间」 |
| 105 | sys_info | `sys_get_system_info` | 补充 get/system，明确「获取系统信息」 |
| 106 | env_getvar | `env_get_variable` | 替换缩写 var 为完整 variable，语义清晰 |
| 107 | env_setvar | `env_set_variable` | 替换缩写 var 为完整 variable，语义清晰 |
| 108 | sys_gethostname | `sys_get_hostname` | 下划线分隔，风格统一 |
| 109 | sys_sethostname | `sys_set_hostname` | 下划线分隔，风格统一 |

### 2.7 设备操作（Device Operations）
| 原调用号 | 原名称 | 优化后名称 | 优化理由 |
|----------|--------|------------|----------|
| 120 | dev_ioctl | `dev_control` | ioctl 是 Unix 缩写（模糊），control 更通用，贴合「设备控制」的本质 |
| 121 | dev_read | `dev_read_data` | 补充 data，明确「读取设备数据」 |
| 122 | dev_write | `dev_write_data` | 补充 data，明确「写入设备数据」 |

## 三、核心系统调用详解（优化后）
### 3.1 PWID 认证（sys_auth_authenticate）
```c
// 原型
int64_t sys_auth_authenticate(const char *password, const char *note);

// 参数
//   password: 密码
//   note: 备注（用于标识会话用途）

// 返回值
//   成功: 新的会话 ID
//   失败: 负数错误码

// 示例（用户态调用）
session_id = syscall(40, "mypassword", "日常使用");
```

### 3.2 PWID 临时提权（sys_auth_elevate_privileges）
```c
// 原型
int64_t sys_auth_elevate_privileges(const char *command, const char **argv);

// 参数
//   command: 要执行的特权命令路径
//   argv: 命令行参数数组（NULL 结尾）

// 返回值
//   成功: 0
//   失败: 负数错误码

// 核心流程
// 1. 验证当前会话的 Root 密码哈希
// 2. 创建临时子进程（继承 Root PWID）
// 3. 执行指定命令
// 4. 命令执行完毕后销毁临时进程
// 5. 恢复原进程的 PWID 权限

// 示例
syscall(42, "/sbin/create_pwid", (const char*[]){"create_pwid", "alice", NULL});
```

### 3.3 文件权限设置（sys_fs_set_permissions）
```c
// 原型
int64_t sys_fs_set_permissions(const char *path, int permissions);

// 参数
//   path: 文件/目录路径
//   permissions: 权限位（贴合 AntX PWID 权限体系，如 0644/0755）

// 返回值
//   成功: 0
//   失败: 负数错误码（如 E_ACCES - 权限不足）

// 权限检查逻辑
// 1. 获取调用进程的当前 PWID
// 2. 查找文件 Inode 中的所有者 PWID
// 3. 验证调用者 PWID 是否为文件所有者或 Root
// 4. 验证通过后更新 Inode 权限位
```

### 3.4 进程终止（sys_proc_terminate）
```c
// 原型
int64_t sys_proc_terminate(int exit_code);

// 参数
//   exit_code: 进程退出码（0 表示正常终止，非 0 表示异常）

// 返回值
//   成功: 0（进程不会执行到返回，由内核回收资源）
//   失败: 负数错误码

// 核心流程
// 1. 标记进程状态为 TERMINATED
// 2. 释放进程占用的内存页、文件描述符等资源
// 3. 通知父进程（通过 proc_waitpid）
// 4. 将进程加入僵尸队列，等待父进程回收 PID
```

### 3.5 主机名设置（sys_set_hostname）
```c
// 原型
int64_t sys_set_hostname(const char *name, size_t len);

// 参数
//   name: 新主机名字符串
//   len: 主机名长度（最大 64 字符）

// 返回值
//   成功: 0
//   失败: 负数错误码（E_AUTH_NOROOT - 非 Root PWID，E_INVAL - 无效参数）

// 核心约束
// 1. 仅 Root PWID（级别 0）可调用
// 2. 主机名存储在 HvFS 的 /etc/hostname 文件（用户态）
// 3. 内核不持久化存储，仅通过接口完成写入
// 4. 默认主机名："antx"

// 示例
syscall(109, "my-antx-node", 11);
```

## 四、错误码优化（语义统一）
### 4.1 通用错误码（优化后）
| 原错误码 | 原值 | 原名称 | 优化后名称 | 优化理由 |
|----------|------|--------|------------|----------|
| - | -1 | E_PERM | `E_PERMISSION_DENIED` | 完整表达「权限被拒绝」，语义无歧义 |
| - | -2 | E_NOTFOUND | `E_ENTITY_NOT_FOUND` | entity 涵盖「文件/目录/PWID」，更通用 |
| - | -4 | E_INTR | `E_INTERRUPTED` | 保留（语义精准） |
| - | -5 | E_IO | `E_IO_ERROR` | 补充 ERROR，风格统一 |
| - | -9 | E_BADFD | `E_INVALID_FD` | invalid 比 bad 更贴合内核语义 |
| - | -10 | E_ACCES | `E_ACCESS_DENIED` | 保留（行业通用，和 E_PERMISSION_DENIED 区分：前者是文件权限，后者是系统权限） |
| - | -17 | E_EXIST | `E_ENTITY_EXISTS` | entity 更通用 |
| - | -20 | E_NOTDIR | `E_NOT_DIRECTORY` | 下划线分隔，风格统一 |
| - | -21 | E_ISDIR | `E_IS_DIRECTORY` | 下划线分隔，风格统一 |
| - | -12 | E_NOMEM | `E_OUT_OF_MEMORY` | 完整表达「内存不足」，语义清晰 |
| - | -14 | E_FAULT | `E_INVALID_ADDRESS` | invalid_address 比 fault 更精准 |
| - | -16 | E_BUSY | `E_RESOURCE_BUSY` | 补充 resource，明确「资源忙」 |
| - | -22 | E_INVAL | `E_INVALID_ARGUMENT` | 补充 argument，明确「无效参数」 |
| - | -34 | E_RANGE | `E_OUT_OF_RANGE` | 保留（语义精准） |

### 4.2 权限相关错误码（优化后）
| 原错误码 | 原值 | 原名称 | 优化后名称 | 优化理由 |
|----------|------|--------|------------|----------|
| - | -100 | E_AUTH_INVALID | `E_INVALID_PWID` | 明确「无效的 PWID」 |
| - | -101 | E_AUTH_NOTFOUND | `E_PWID_NOT_FOUND` | 明确「PWID 不存在」 |
| - | -102 | E_AUTH_DISABLED | `E_PWID_DISABLED` | 保留（语义精准） |
| - | -103 | E_AUTH_EXPIRED | `E_PWID_SESSION_EXPIRED` | 补充 session，明确「PWID 会话过期」 |
| - | -104 | E_AUTH_PWERR | `E_PWID_PASSWORD_INCORRECT` | 完整表达「PWID 密码错误」 |
| - | -105 | E_AUTH_NOROOT | `E_REQUIRES_ROOT_PWID` | 明确「需要 Root PWID 权限」 |
| - | -106 | E_AUTH_DENY | `E_PWID_OPERATION_DENIED` | 明确「PWID 操作被拒绝」 |

## 五、优化总结
### 核心优化点
1. **命名风格统一**：所有接口采用「动作+对象」的动宾结构，替换 Unix 缩写（如 `chmod`→`set_permissions`、`brk`→`set_break_addr`）；
2. **语义精准无歧义**：补充关键修饰词（如 `sleep`→`sleep_ms`、`stat`→`get_stat_fd`），避免模糊表达；
3. **弱化 Unix 印记**：替换 `fork`/`ioctl`/`chdir` 等强历史遗留命名，同时保留 `open`/`read`/`write` 等行业通用且语义精准的命名；
4. **贴合自研特色**：PWID 相关接口均补充 `pwid` 前缀，强化 AntX 权限体系的辨识度；
5. **错误码语义完整**：替换缩写错误码为完整表达（如 `E_PERM`→`E_PERMISSION_DENIED`），便于调试和理解。

### 关键保留项
- 保留 `fs_open`/`fs_read`/`fs_write` 等行业通用命名（降低开发者学习成本）；
- 保留 `proc_waitpid`/`net_socket` 等无歧义且通用的命名；
- 保留 PWID 核心概念（`auth_create_pwid`/`auth_verify_pwid_password`），强化自研特色。

优化后的接口既摆脱了 Unix/Linux 的历史包袱，又保持了「易理解、易实现」的特性，同时贴合 AntX 「极简高效、自研特色」的设计理念。