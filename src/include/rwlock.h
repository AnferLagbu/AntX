/**
 * @file rwlock.h
 * @brief 读写锁接口定义
 *
 * 提供读者-写者锁（Read-Write Lock）实现，允许多个读者并行访问，
 * 但写者需要独占访问。适用于读多写少的场景（如文件系统）。
 */

#ifndef RWLOCK_H
#define RWLOCK_H

#include "spinlock.h"

/* ============================================================
 * 读写锁结构体
 * ============================================================ */

/**
 * @brief 读写锁结构体
 *
 * 基于自旋锁实现的读写锁，支持以下特性：
 * - 多个读者可以同时持有锁
 * - 写者独占访问，阻塞所有其他读者和写者
 * - 写者优先策略（防止写者饥饿）
 */
typedef struct rwlock {
    spinlock_t lock;        /**< 保护内部状态的自旋锁 */
    volatile int readers;   /**< 当前活跃的读者计数 */
    volatile int writer;    /**< 写者标志 (0=无写者, 1=有写者) */
    volatile int pending_writers; /**< 等待中的写者数 */
} rwlock_t;

/* ============================================================
 * 初始化宏
 * ============================================================ */

/**
 * @brief 静态初始化读写锁
 *
 * @param name 锁变量名
 */
#define RWLOCK_INIT \
    { \
        .lock = {.locked = 0}, \
        .readers = 0, \
        .writer = 0, \
        .pending_writers = 0 \
    }

/**
 * @brief 定义并初始化静态读写锁
 *
 * @param name 锁变量名
 */
#define DEFINE_RWLOCK(name) \
    rwlock_t name = RWLOCK_INIT

/* ============================================================
 * 核心接口函数声明
 * ============================================================ */

/**
 * @brief 初始化读写锁
 *
 * @param rw 读写锁指针
 */
void rwlock_init(rwlock_t *rw);

/**
 * @brief 获取读锁
 *
 * 如果当前有写者持有锁或有待处理的写者，则等待。
 * 允许多个读者同时持有读锁。
 *
 * @param rw 读写锁指针
 */
void read_lock(rwlock_t *rw);

/**
 * @brief 释放读锁
 *
 * 减少读者计数，如果有待处理的写者，唤醒它们。
 *
 * @param rw 读写锁指针
 */
void read_unlock(rwlock_t *rw);

/**
 * @brief 非阻塞式尝试获取读锁
 *
 * 如果无法立即获取，返回失败。
 *
 * @param rw 读写锁指针
 * @return 非0 表示获取成功，0 表示失败
 */
int read_trylock(rwlock_t *rw);

/**
 * @brief 获取写锁
 *
 * 独占访问，阻塞所有其他读者和写者。
 * 实现写者优先策略，防止写者饥饿。
 *
 * @param rw 读写锁指针
 */
void write_lock(rwlock_t *rw);

/**
 * @brief 释放写锁
 *
 * 唤醒可能正在等待的读者或其他写者。
 *
 * @param rw 读写锁指针
 */
void write_unlock(rwlock_t *rw);

/**
 * @brief 非阻塞式尝试获取写锁
 *
 * 如果无法立即获取（有读者或写者），返回失败。
 *
 * @param rw 读写锁指针
 * @return 非0 表示获取成功，0 表示失败
 */
int write_trylock(rwlock_t *rw);

/* ============================================================
 * 中断安全版本
 * ============================================================ */

/**
 * @brief 保存中断标志并获取读锁
 *
 * @param rw 读写锁指针
 * @param flags 用于保存中断标志的变量
 */
void read_lock_irqsave(rwlock_t *rw, unsigned long *flags);

/**
 * @brief 释放读锁并恢复中断标志
 *
 * @param rw 读写锁指针
 * @param flags 之前保存的中断标志
 */
void read_unlock_irqrestore(rwlock_t *rw, unsigned long flags);

/**
 * @brief 保存中断标志并获取写锁
 *
 * @param rw 读写锁指针
 * @param flags 用于保存中断标志的变量
 */
void write_lock_irqsave(rwlock_t *rw, unsigned long *flags);

/**
 * @brief 释放写锁并恢复中断标志
 *
 * @param rw 读写锁指针
 * @param flags 之前保存的中断标志
 */
void write_unlock_irqrestore(rwlock_t *rw, unsigned long flags);

#endif /* RWLOCK_H */
