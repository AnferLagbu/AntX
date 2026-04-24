# AntX 内核日志系统 (KLog)

> **版本**: 1.0.0
> **状态**: 核心基础设施
> **最后更新**: 2026-04-25

---

## 一、系统概述

KLog是AntX操作系统的核心日志基础设施，提供统一的日志记录、过滤和持久化功能。作为系统核心组件，KLog在系统启动的最早阶段初始化，为所有内核模块提供日志服务。

### 1.1 核心地位

```
┌─────────────────────────────────────────────────────────────┐
│                    AntX 系统架构                             │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────────┐   │
│  │                   KLog 日志系统                      │   │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐   │   │
│  │  │  Boot   │ │ Memory  │ │ Process │ │    FS   │   │   │
│  │  └────┬────┘ └────┬────┘ └────┬────┘ └────┬────┘   │   │
│  │       │           │           │           │         │   │
│  │       └───────────┴─────┬─────┴───────────┘         │   │
│  │                         ▼                           │   │
│  │              ┌─────────────────────┐                │   │
│  │              │   KLog Core API     │                │   │
│  │              └─────────────────────┘                │   │
│  │                         │                           │   │
│  │           ┌─────────────┼─────────────┐            │   │
│  │           ▼             ▼             ▼            │   │
│  │     ┌──────────┐  ┌──────────┐  ┌──────────┐      │   │
│  │     │  Serial  │  │  Buffer  │  │   Disk   │      │   │
│  │     │  Output  │  │  Storage │  │ Persist  │      │   │
│  │     └──────────┘  └──────────┘  └──────────┘      │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### 1.2 设计目标

| 目标 | 说明 |
|------|------|
| **统一接口** | 提供一致的日志API，所有模块使用相同方式记录日志 |
| **分级过滤** | 支持日志级别过滤，减少无关信息干扰 |
| **分类管理** | 按模块分类日志，便于问题定位 |
| **持久化存储** | 支持日志保存到磁盘，重启后可恢复 |
| **高性能** | 最小化日志开销，不影响系统性能 |

---

## 二、日志级别

| 级别 | 宏 | 说明 | 使用场景 |
|------|-----|------|----------|
| DEBUG | `KLOG_DEBUG` | 调试信息 | 开发调试时详细跟踪 |
| INFO | `KLOG_INFO` | 一般信息 | 正常操作状态记录 |
| NOTICE | `KLOG_NOTICE` | 注意信息 | 重要但非错误的事件 |
| WARN | `KLOG_WARN` | 警告信息 | 潜在问题提示 |
| ERROR | `KLOG_ERROR` | 错误信息 | 操作失败但系统可继续 |
| CRITICAL | `KLOG_CRITICAL` | 严重错误 | 系统级故障 |

---

## 三、日志分类

| 分类 | 标识 | 说明 |
|------|------|------|
| GENERAL | `LOG_KERN` | 通用内核日志 |
| BOOT | `LOG_BOOT` | 启动引导日志 |
| INIT | `LOG_INIT` | 初始化日志 |
| KERNEL | `LOG_KERN` | 内核核心日志 |
| MEMORY | `LOG_MEM` | 内存管理日志 |
| PROCESS | `LOG_PROC` | 进程管理日志 |
| FS | `LOG_FS` | 文件系统日志 |
| DRIVER | `LOG_DRV` | 设备驱动日志 |
| SYSCALL | `LOG_SYSCALL` | 系统调用日志 |
| IPC | `LOG_IPC` | 进程通信日志 |
| SECURITY | `LOG_SEC` | 安全审计日志 |
| NETWORK | `LOG_NET` | 网络相关日志 |

---

## 四、API 接口

### 4.1 核心函数

```c
// 初始化日志系统
void klog_init(void);

// 设置/获取日志级别
void klog_set_level(klog_level_t level);
klog_level_t klog_get_level(void);

// 设置/获取日志标志
void klog_set_flags(uint32_t flags);
uint32_t klog_get_flags(void);

// 写入日志
int klog_write(klog_level_t level, klog_category_t cat,
               const char *file, const char *func, int line,
               const char *fmt, ...);

// 刷新/转储/清空日志
void klog_flush(void);
void klog_dump(void);
void klog_clear(void);

// 持久化操作
int klog_save_to_disk(void);
int klog_load_from_disk(void);
```

### 4.2 便捷宏

```c
// 通用日志宏
#define klog(level, cat, fmt, ...) \
    klog_write(level, cat, __FILE__, __func__, __LINE__, fmt, ##__VA_ARGS__)

// 级别宏
#define klog_debug(cat, fmt, ...)   klog(KLOG_DEBUG, cat, fmt, ##__VA_ARGS__)
#define klog_info(cat, fmt, ...)    klog(KLOG_INFO, cat, fmt, ##__VA_ARGS__)
#define klog_notice(cat, fmt, ...)  klog(KLOG_NOTICE, cat, fmt, ##__VA_ARGS__)
#define klog_warn(cat, fmt, ...)    klog(KLOG_WARN, cat, fmt, ##__VA_ARGS__)
#define klog_error(cat, fmt, ...)   klog(KLOG_ERROR, cat, fmt, ##__VA_ARGS__)
#define klog_crit(cat, fmt, ...)    klog(KLOG_CRITICAL, cat, fmt, ##__VA_ARGS__)

// 模块便捷宏
#define klog_boot(fmt, ...)    klog_info(LOG_BOOT, fmt, ##__VA_ARGS__)
#define klog_init_msg(fmt, ...) klog_info(LOG_INIT, fmt, ##__VA_ARGS__)
#define klog_kern(fmt, ...)    klog_info(LOG_KERN, fmt, ##__VA_ARGS__)
#define klog_mem(fmt, ...)     klog_info(LOG_MEM, fmt, ##__VA_ARGS__)
#define klog_proc(fmt, ...)    klog_info(LOG_PROC, fmt, ##__VA_ARGS__)
#define klog_fs(fmt, ...)      klog_info(LOG_FS, fmt, ##__VA_ARGS__)
#define klog_drv(fmt, ...)     klog_info(LOG_DRV, fmt, ##__VA_ARGS__)

// 安全日志宏
#define klog_sec_info(fmt, ...)  klog_info(LOG_SEC, fmt, ##__VA_ARGS__)
#define klog_sec_warn(fmt, ...)  klog_warn(LOG_SEC, fmt, ##__VA_ARGS__)
#define klog_sec_err(fmt, ...)   klog_error(LOG_SEC, fmt, ##__VA_ARGS__)
```

### 4.3 兼容性接口

```c
// 兼容旧版 printk 接口
int printk(const char *fmt, ...);
int vprintk(const char *fmt, va_list args);

// 兼容 Linux 风格宏
#define pr_debug(fmt, ...)  klog_debug(KLOG_CAT_GENERAL, fmt, ##__VA_ARGS__)
#define pr_info(fmt, ...)   klog_info(KLOG_CAT_GENERAL, fmt, ##__VA_ARGS__)
#define pr_warn(fmt, ...)   klog_warn(KLOG_CAT_GENERAL, fmt, ##__VA_ARGS__)
#define pr_err(fmt, ...)    klog_error(KLOG_CAT_GENERAL, fmt, ##__VA_ARGS__)
```

---

## 五、日志标志

| 标志 | 值 | 说明 |
|------|-----|------|
| `KLOG_FLAG_OUTPUT_SERIAL` | 0x01 | 输出到串口 |
| `KLOG_FLAG_OUTPUT_BUFFER` | 0x02 | 输出到缓冲区 |
| `KLOG_FLAG_OUTPUT_CONSOLE` | 0x04 | 输出到控制台 |
| `KLOG_FLAG_TIMESTAMP` | 0x08 | 包含时间戳 |
| `KLOG_FLAG_LOCATION` | 0x10 | 包含位置信息 |
| `KLOG_FLAG_PERSIST` | 0x20 | 自动持久化 |

---

## 六、使用示例

### 6.1 基本使用

```c
#include "klog.h"

void some_function(void) {
    // 记录信息日志
    klog_info(LOG_KERN, "Function started");
    
    // 记录警告
    klog_warn(LOG_MEM, "Low memory: %d bytes free", free_bytes);
    
    // 记录错误
    klog_error(LOG_FS, "Failed to open file: %s", filename);
    
    // 记录严重错误
    klog_crit(LOG_DRV, "Device %s not responding", dev_name);
}
```

### 6.2 调试日志

```c
// 在开发阶段启用调试日志
klog_set_level(KLOG_DEBUG);

// 调试信息
klog_debug(LOG_PROC, "Process %d state: %d", pid, state);
klog_debug(LOG_MEM, "Allocated block at 0x%x, size %d", addr, size);
```

### 6.3 安全审计

```c
// 记录安全相关事件
klog_sec_info("User %s logged in", username);
klog_sec_warn("Failed login attempt for %s", username);
klog_sec_err("Permission denied for operation %s", op_name);
```

---

## 七、配置

### 7.1 编译时配置

```c
// 设置默认日志级别
#ifndef KLOG_DEFAULT_LEVEL
#define KLOG_DEFAULT_LEVEL KLOG_INFO
#endif

// 设置默认日志标志
#ifndef KLOG_DEFAULT_FLAGS
#define KLOG_DEFAULT_FLAGS (KLOG_FLAG_OUTPUT_SERIAL | KLOG_FLAG_OUTPUT_BUFFER | KLOG_FLAG_TIMESTAMP)
#endif
```

### 7.2 运行时配置

```c
// 设置日志级别为警告及以上
klog_set_level(KLOG_WARN);

// 启用时间戳和位置信息
klog_set_flags(KLOG_FLAG_OUTPUT_SERIAL | KLOG_FLAG_TIMESTAMP | KLOG_FLAG_LOCATION);

// 设置特定分类的日志级别
klog_set_category_level(LOG_FS, KLOG_DEBUG);
```

---

## 八、持久化

日志系统支持将日志缓冲区保存到磁盘：

```c
// 保存日志到 /cfg/system/klog.db
klog_save_to_disk();

// 从磁盘加载日志
klog_load_from_disk();
```

---

## 九、性能考虑

1. **缓冲区大小**: 默认128KB，可根据需要调整 `KLOG_BUFFER_SIZE`
2. **级别过滤**: 在编译时和运行时都进行过滤，避免不必要的格式化开销
3. **异步输出**: 日志写入缓冲区后立即返回，串口输出不阻塞

---

## 十、文件位置

| 文件 | 说明 |
|------|------|
| [include/klog.h](file:///home/anfer/Code/C/AntX/src/include/klog.h) | 日志系统头文件 |
| [kernel/klog.c](file:///home/anfer/Code/C/AntX/src/kernel/klog.c) | 日志系统实现 |

---

*KLog - AntX 内核日志系统*
