/**
 * @file spinlock.h
 * @brief 自旋锁接口定义
 *
 * 提供 x86_64 架构下的高效自旋锁实现，用于保护临界区。
 * 自旋锁适用于中断上下文和极短临界区的场景（<1μs）。
 */

#ifndef SPINLOCK_H
#define SPINLOCK_H

#include "atomic.h"

/* ============================================================
 * 调试配置
 * ============================================================ */

#ifdef CONFIG_DEBUG_SPINLOCK

#define SPINLOCK_DEBUG_INIT(name) \
    .name = (name), \
    .owner = NULL, \
    .acquire_time = 0

#define spin_lock(l) spin_lock_debug((l), __FILE__, __LINE__)
#define spin_lock_irqsave(l, f) spin_lock_irqsave_debug((l), &(f), __FILE__, __LINE__)

#else

#define SPINLOCK_DEBUG_INIT(name)
#define spin_lock(l) spin_lock_raw(l)
#define spin_lock_irqsave(l, f) spin_lock_irqsave_raw(l, f)

#endif /* CONFIG_DEBUG_SPINLOCK */

/* ============================================================
 * 自旋锁结构体
 * ============================================================ */

/**
 * @brief 自旋锁结构体
 *
 * 使用 x86 的 xchg 或 lock bts 指令实现原子操作。
 */
typedef struct spinlock {
    volatile int locked;   /**< 0=未锁定, 1=已锁定 */
#ifdef CONFIG_DEBUG_SPINLOCK
    const char *name;      /**< 锁名称 (调试用) */
    void *owner;           /**< 持有者 (CPU ID) */
    uint64_t acquire_time; /**< 获取时间戳 */
#endif
} spinlock_t;

/* ============================================================
 * 初始化宏
 * ============================================================ */

/**
 * @brief 静态初始化自旋锁
 *
 * @param name 锁变量名
 */
#define SPINLOCK_INIT(name) \
    { .locked = 0 SPINLOCK_DEBUG_INIT(#name) }

/**
 * @brief 定义并初始化静态自旋锁
 *
 * @param name 锁变量名
 */
#define DEFINE_SPINLOCK(name) \
    spinlock_t name = SPINLOCK_INIT(name)

/* ============================================================
 * 核心接口函数声明
 * ============================================================ */

/**
 * @brief 初始化自旋锁
 *
 * @param lock 自旋锁指针
 */
void spin_init(spinlock_t *lock);

/**
 * @brief 阻塞式获取自旋锁
 *
 * 如果锁已被持有，则当前 CPU 会自旋等待直到获取成功。
 * 注意：此函数在关闭中断的情况下调用以避免死锁。
 *
 * @param lock 自旋锁指针
 */
void spin_lock(spinlock_t *lock);

/**
 * @brief 原始锁获取函数（无调试信息）
 *
 * @param lock 自旋锁指针
 */
void spin_lock_raw(spinlock_t *lock);

/**
 * @brief 释放自旋锁
 *
 * @param lock 自旋锁指针
 */
void spin_unlock(spinlock_t *lock);

/**
 * @brief 非阻塞式尝试获取自旋锁
 *
 * 如果锁已被持有，立即返回失败，不会自旋等待。
 *
 * @param lock 自旋锁指针
 * @return 非0 表示获取成功，0 表示失败
 */
int spin_trylock(spinlock_t *lock);

/**
 * @brief 查询自旋锁状态
 *
 * @param lock 自旋锁指针
 * @return 非0 表示已锁定，0 表示未锁定
 */
int spin_is_locked(spinlock_t *lock);

/**
 * @brief 持有者断言（调试用）
 *
 * 如果当前 CPU 不持有该锁，则触发 panic。
 *
 * @param lock 自旋锁指针
 */
void spin_assert_held(spinlock_t *lock);

/* ============================================================
 * 中断安全版本
 * ============================================================ */

/**
 * @brief 保存中断标志并获取锁
 *
 * 在获取锁前保存当前中断状态并禁用中断，
 * 防止在中断处理程序中再次获取同一把锁导致死锁。
 *
 * @param lock 自旋锁指针
 * @param flags 用于保存中断标志的变量
 */
void spin_lock_irqsave(spinlock_t *lock, unsigned long *flags);

/**
 * @brief 原始版本的 irqsave（无调试信息）
 *
 * @param lock 自旋锁指针
 * @param flags 用于保存中断标志的变量
 */
void spin_lock_irqsave_raw(spinlock_t *lock, unsigned long *flags);

/**
 * @brief 释放锁并恢复中断标志
 *
 * @param lock 自旋锁指针
 * @param flags 之前保存的中断标志
 */
void spin_unlock_irqrestore(spinlock_t *lock, unsigned long flags);

/**
 * @brief 禁用中断并获取锁
 *
 * 无条件禁用中断并获取锁。
 *
 * @param lock 自旋锁指针
 */
void spin_lock_irq(spinlock_t *lock);

/**
 * @brief 释放锁并启用中断
 *
 * @param lock 自旋锁指针
 */
void spin_unlock_irq(spinlock_t *lock);

/* ============================================================
 * 调试接口
 * ============================================================ */

#ifdef CONFIG_DEBUG_SPINLOCK

/**
 * @brief 带调试信息的锁获取
 *
 * 记录文件、行号等调试信息。
 *
 * @param lock 自旋锁指针
 * @param file 源文件名
 * @param line 行号
 */
void spin_lock_debug(spinlock_t *lock, const char *file, int line);

/**
 * @brief 带调试信息的 irqsave 锁获取
 *
 * @param lock 自旋锁指针
 * @param flags 用于保存中断标志的变量
 * @param file 源文件名
 * @param line 行号
 */
void spin_lock_irqsave_debug(spinlock_t *lock, unsigned long *flags,
                              const char *file, int line);

#endif /* CONFIG_DEBUG_SPINLOCK */

#endif /* SPINLOCK_H */
