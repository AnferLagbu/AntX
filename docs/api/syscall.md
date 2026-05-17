# 系统调用

> 用户态系统调用接口

---

## 🎯 系统调用机制

### 调用方式

```c
// 通过int 0x80触发系统调用
static inline long syscall0(long num) {
    long ret;
    asm volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(num)
        : "memory"
    );
    return ret;
}
```

### 参数传递

| 寄存器 | 用途 |
|--------|------|
| RAX | 系统调用号 |
| RDI | 第1个参数 |
| RSI | 第2个参数 |
| RDX | 第3个参数 |
| R10 | 第4个参数 |
| R8 | 第5个参数 |

---

## 📋 系统调用列表

### 进程管理 (0-19)

| 号 | 名称 | 说明 |
|----|------|------|
| 1 | proc_exec | 执行程序 |
| 2 | proc_exit | 退出进程 |
| 3 | proc_wait | 等待子进程 |
| 4 | proc_getid | 获取进程ID |
| 5 | proc_getppid | 获取父进程ID |
| 6 | proc_getpwid | 获取PWID |

### 文件系统 (20-49)

| 号 | 名称 | 说明 |
|----|------|------|
| 20 | fs_open | 打开文件 |
| 21 | fs_close | 关闭文件 |
| 22 | fs_read | 读文件 |
| 23 | fs_write | 写文件 |
| 24 | fs_seek | 移动文件指针 |
| 25 | fs_stat | 获取文件状态 |
| 26 | fs_mkdir | 创建目录 |
| 27 | fs_rmdir | 删除目录 |
| 28 | fs_unlink | 删除文件 |
| 29 | fs_chmod | 修改权限 |
| 30 | fs_chown | 修改所有者 |

### 内存管理 (50-69)

| 号 | 名称 | 说明 |
|----|------|------|
| 50 | mem_mmap | 内存映射 |
| 51 | mem_munmap | 取消映射 |
| 52 | mem_brk | 调整堆边界 |

### 安全 (70-89)

| 号 | 名称 | 说明 |
|----|------|------|
| 70 | auth_login | 登录 |
| 71 | auth_logout | 登出 |
| 72 | auth_create_first | 创建初始身份 |

---

## 📝 使用示例

### 打开文件

```c
int fd = syscall3(SYS_FS_OPEN, 
                  (long)"/test.txt",  // 路径
                  O_RDONLY,           // 标志
                  0);                 // 模式
```

### 写文件

```c
const char *msg = "Hello, AntX!";
long ret = syscall3(SYS_FS_WRITE,
                    fd,                // 文件描述符
                    (long)msg,         // 缓冲区
                    strlen(msg));      // 大小
```

---

**最后更新**: 2026-05-18
