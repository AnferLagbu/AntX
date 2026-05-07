# QueenX 进程间通信 (IPC)

> 本文档描述 QueenX 内核的进程间通信机制实现。
> **最后更新**: 2026-05-07

## 一、概述

QueenX 实现了完整的 IPC 子系统，支持多种进程间通信方式：

| IPC 类型 | 说明 | 用途 | 实现状态 |
|----------|------|------|---------|
| **管道 (Pipe)** | 单向字节流 | 父子进程通信 | ✅ 完整实现 |
| **信号 (Signal)** | 异步通知 | 进程控制、异常处理 | ⚠ 基础实现 (注册/分发为stub) |
| **共享内存 (SHM)** | 高速数据共享 | 大数据传输 | ✅ 完整实现 |
| **消息队列 (MsgQ)** | 结构化消息 | 异步消息传递 | ✅ 完整实现 |
| **信号量 (Sem)** | 同步原语 | 资源互斥访问 | ✅ 完整实现 |

## 二、管道 (Pipe)

### 2.1 数据结构

```c
struct pipe {
    ipc_id_t id;
    char buffer[PIPE_BUFFER_SIZE];  // 4KB 缓冲区
    uint32_t read_pos;
    uint32_t write_pos;
    uint32_t count;
    
    pid_t read_pid;
    pid_t write_pid;
    
    int read_fd;    // 读端文件描述符
    int write_fd;   // 写端文件描述符
    
    int readers;    // 读进程数
    int writers;    // 写进程数
    
    struct wait_queue read_wait;   // 读等待队列
    struct wait_queue write_wait;  // 写等待队列
};
```

### 2.2 API

```c
// 创建管道
// pipefd[0] = 读端, pipefd[1] = 写端
int pipe_create(int pipefd[2]);

// 从管道读取
int pipe_read(int fd, void *buf, uint32_t count);

// 写入管道
int pipe_write(int fd, const void *buf, uint32_t count);

// 关闭管道
int pipe_close(int fd);
```

### 2.3 使用示例

```c
int pipefd[2];
pipe_create(pipefd);

// 父进程写入
if (fork() == 0) {
    pipe_close(pipefd[1]);  // 子进程关闭写端
    char buf[100];
    pipe_read(pipefd[0], buf, 100);
    pipe_close(pipefd[0]);
} else {
    pipe_close(pipefd[0]);  // 父进程关闭读端
    pipe_write(pipefd[1], "Hello", 5);
    pipe_close(pipefd[1]);
}
```

## 三、信号 (Signal)

### 3.1 支持的信号

| 信号 | 值 | 说明 |
|------|-----|------|
| `SIG_INT` | 1 | 中断 (Ctrl+C) |
| `SIG_ILL` | 2 | 非法指令 |
| `SIG_FPE` | 3 | 浮点异常 |
| `SIG_SEGV` | 4 | 段错误 |
| `SIG_TERM` | 5 | 终止请求 |
| `SIG_KILL` | 6 | 强制终止 |
| `SIG_STOP` | 7 | 停止进程 |
| `SIG_CONT` | 8 | 继续运行 |
| `SIG_CHLD` | 9 | 子进程状态改变 |
| `SIG_USR1` | 10 | 用户自定义 1 |
| `SIG_USR2` | 11 | 用户自定义 2 |
| `SIG_ALARM` | 12 | 定时器闹钟 |
| `SIG_PIPE` | 13 | 管道破裂 |

### 3.2 API

```c
// 发送信号
int signal_send(pid_t pid, int sig);

// 注册信号处理函数
int signal_register(int sig, void (*handler)(int), uint32_t flags);

// 阻塞/解除阻塞信号
int signal_block(int sig);
int signal_unblock(int sig);

// 分发待处理信号
void signal_dispatch(void);
```

## 四、共享内存 (Shared Memory)

### 4.1 数据结构

```c
struct shm_segment {
    ipc_id_t id;
    uint64_t phys_addr;   // 物理地址
    uint64_t size;        // 大小
    
    pid_t creator;        // 创建者
    uint32_t ref_count;   // 引用计数
    
    pid_t attached_pids[16];  // 附加进程列表
    uint32_t attach_count;
    
    int flags;
    int perm;             // 权限
};
```

### 4.2 API

```c
// 创建共享内存段
ipc_id_t shm_create(uint64_t size, int perm);

// 附加到共享内存
int shm_attach(ipc_id_t id, void **addr);

// 分离共享内存
int shm_detach(ipc_id_t id);

// 销毁共享内存
int shm_destroy(ipc_id_t id);
```

### 4.3 使用示例

```c
// 进程 A 创建共享内存
ipc_id_t shmid = shm_create(4096, 0666);
void *addr;
shm_attach(shmid, &addr);
memcpy(addr, "Hello", 5);

// 进程 B 访问共享内存
shm_attach(shmid, &addr);
printf("%s\n", (char *)addr);  // 输出 "Hello"
```

## 五、消息队列 (Message Queue)

### 5.1 数据结构

```c
struct message {
    uint64_t type;        // 消息类型
    uint64_t sender;      // 发送者 PID
    uint64_t size;        // 消息大小
    char data[MSG_MAX_SIZE];  // 消息数据 (4KB)
    struct message *next;
};

struct msg_queue {
    ipc_id_t id;
    pid_t owner;
    
    struct message *head;
    struct message *tail;
    uint32_t count;
    uint32_t max_msgs;    // 最大消息数 (64)
    uint32_t max_size;    // 最大消息大小 (4KB)
    
    struct wait_queue send_wait;
    struct wait_queue recv_wait;
};
```

### 5.2 API

```c
// 创建消息队列
ipc_id_t msgq_create(int perm);

// 发送消息
int msgq_send(ipc_id_t id, uint64_t type, const void *data, uint64_t size);

// 接收消息
// 返回值: 成功时返回读取的字节数，队列为空返回 -1
int msgq_recv(ipc_id_t id, uint64_t *type, void *data, uint64_t *size);

// 销毁消息队列
int msgq_destroy(ipc_id_t id);
```

### 5.3 使用示例

```c
// 进程 A 发送消息
ipc_id_t mqid = msgq_create(0666);
msgq_send(mqid, 1, "Hello", 5);

// 进程 B 接收消息
uint64_t type;
char buf[100];
uint64_t size;
msgq_recv(mqid, &type, buf, &size);
```

## 六、信号量 (Semaphore)

### 6.1 数据结构

```c
struct semaphore {
    ipc_id_t id;
    pid_t owner;
    
    int32_t count;        // 当前计数
    uint32_t max_count;   // 最大计数
    
    struct wait_queue wait;
    
    int flags;
    int perm;
};
```

### 6.2 API

```c
// 创建信号量
ipc_id_t sem_create(uint32_t count, uint32_t max_count);

// P 操作 (等待/减 1)
int sem_wait(ipc_id_t id);

// V 操作 (释放/加 1)
int sem_post(ipc_id_t id);

// 销毁信号量
int sem_destroy(ipc_id_t id);
```

### 6.3 使用示例

```c
// 创建二值信号量 (互斥锁)
ipc_id_t mutex = sem_create(1, 1);

// 临界区
sem_wait(mutex);
// ... 访问共享资源 ...
sem_post(mutex);

sem_destroy(mutex);
```

## 七、系统调用号

| 系统调用 | 号码 | 说明 |
|----------|------|------|
| `SYS_IPC_PIPE` | 80 | 创建管道 |
| `SYS_IPC_SIGNAL` | 81 | 发送信号 |
| `SYS_IPC_SHM_CREATE` | 82 | 创建共享内存 |
| `SYS_IPC_SHM_ATTACH` | 83 | 附加共享内存 |
| `SYS_IPC_SHM_DETACH` | 84 | 分离共享内存 |
| `SYS_IPC_SHM_DESTROY` | 85 | 销毁共享内存 |
| `SYS_IPC_MSGQ_CREATE` | 86 | 创建消息队列 |
| `SYS_IPC_MSGQ_SEND` | 87 | 发送消息 |
| `SYS_IPC_MSGQ_RECV` | 88 | 接收消息 |
| `SYS_IPC_MSGQ_DESTROY` | 89 | 销毁消息队列 |
| `SYS_IPC_SEM_CREATE` | 90 | 创建信号量 |
| `SYS_IPC_SEM_WAIT` | 91 | 信号量等待 |
| `SYS_IPC_SEM_POST` | 92 | 信号量释放 |
| `SYS_IPC_SEM_DESTROY` | 93 | 销毁信号量 |

## 八、文件位置

| 文件 | 说明 |
|------|------|
| `src/include/ipc.h` | IPC 数据结构定义 |
| `src/ipc/ipc.c` | IPC 实现 |
| `src/include/syscall.h` | 系统调用号定义 |

## 九、限制

| 资源 | 限制 |
|------|------|
| 最大管道数 | 64 |
| 最大共享内存段数 | 16 |
| 最大消息队列数 | 32 |
| 最大信号量数 | 64 |
| 管道缓冲区大小 | 4KB |
| 共享内存最大大小 | 16MB |
| 消息最大大小 | 4KB |
| 消息队列最大消息数 | 64 |

---

*最后更新: 2026-04-20*
