/**
 * @file mutex.h
 * @brief 睡眠锁 (Mutex) 接口定义
 *
 * 提供基于等待队列的睡眠锁实现，适用于进程上下文中长时间持有临界区的场景。
 * 与自旋锁不同，Mutex 在无法获取锁时会将当前进程挂起，让出 CPU。
 *
 * 重要：**禁止在中断上下文中使用 Mutex**
 */

#ifndef MUTEX_H
#define MUTEX_H

#include "spinlock.h"
#include "types.h"

/* ============================================================
 * 调试配置
 * ============================================================ */

#ifdef CONFIG_DEBUG_MUTEX

#define MUTEX_DEBUG_INIT(name) .name = (name)

#else

#define MUTEX_DEBUG_INIT(name)

#endif /* CONFIG_DEBUG_MUTEX */

/* ============================================================
 * Mutex 结构体
 * ============================================================ */

/**
 * @brief 等待队列节点结构体
 */
typedef struct WaitNode {
    struct Process *proc;      /**< 等待的进程指针 */
    struct WaitNode *next;     /**< 下一个等待节点 */
    int woken;                 /**< 是否已被唤醒 */
} WaitNode;

/**
 * @brief 等待队列结构体
 */
typedef struct {
    WaitNode *head;            /**< 队列头 */
    WaitNode *tail;            /**< 队列尾 */
    spinlock_t lock;           /**< 保护队列的自旋锁 */
} wait_queue_t;

/**
 * @brief Mutex 锁结构体
 *
 * 基于等待队列实现的睡眠锁，支持以下特性：
 * - 进程挂起/唤醒机制
 * - 可配置的超时时间
 * - 死锁检测（调试模式）
 * - 持有者跟踪
 */
typedef struct mutex {
    volatile int locked;       /**< 0=未锁定, 1=已锁定 */
    int owner;                 /**< 当前持有者的 PID (-1=无) */
    uint64_t acquire_time;     /**< 获取时间戳 (用于调试和统计) */
    unsigned int depth;        /**< 递归锁深度 (v1.0 不强制递归，但记录) */
    wait_queue_t wait_queue;   /**< 等待队列 */
    spinlock_t lock_spinlock;  /**< 保护内部状态的自旋锁 */
#ifdef CONFIG_DEBUG_MUTEX
    const char *name;          /**< 锁名称 (调试用) */
    const char *file;          /**< 最后获取位置 (文件名) */
    int line;                  /**< 最后获取位置 (行号) */
#endif
} mutex_t;

/* ============================================================
 * 初始化宏
 * ============================================================ */

/**
 * @brief 静态初始化 Mutex
 *
 * @param name 锁变量名
 */
#define MUTEX_INIT(name) \
    { \
        .locked = 0, \
        .owner = -1, \
        .acquire_time = 0, \
        .depth = 0, \
        .wait_queue = { NULL, NULL, SPINLOCK_INIT }, \
        .lock_spinlock = SPINLOCK_INIT \
        MUTEX_DEBUG_INIT(#name) \
    }

/**
 * @brief 定义并初始化静态 Mutex
 *
 * @param name 锁变量名
 */
#define DEFINE_MUTEX(name) \
    mutex_t name = MUTEX_INIT(name)

/* ============================================================
 * 核心接口函数声明
 * ============================================================ */

/**
 * @brief 初始化 Mutex
 *
 * @param m Mutex 指针
 */
void mutex_init(mutex_t *m);

/**
 * @brief 阻塞式获取 Mutex
 *
 * 如果锁已被持有，当前进程将被挂起到等待队列中。
 * **不可在中断上下文中调用此函数。**
 *
 * @param m Mutex 指针
 */
void mutex_lock(mutex_t *m);

/**
 * @brief 非阻塞式尝试获取 Mutex
 *
 * 如果锁已被持有，立即返回失败。
 *
 * @param m Mutex 指针
 * @return 非0 表示获取成功，0 表示失败
 */
int mutex_trylock(mutex_t *m);

/**
 * @brief 带超时的获取 Mutex
 *
 * 如果在指定时间内无法获取锁，返回失败。
 *
 * @param m Mutex 指针
 * @param timeout_ms 超时时间（毫秒），0 表示无限等待
 * @return 非0 表示获取成功，0 表示超时
 */
int mutex_lock_timeout(mutex_t *m, unsigned int timeout_ms);

/**
 * @brief 释放 Mutex
 *
 * 唤醒等待队列中的一个进程。
 *
 * @param m Mutex 指针
 */
void mutex_unlock(mutex_t *m);

/**
 * @brief 查询 Mutex 状态
 *
 * @param m Mutex 指针
 * @return 非0 表示已锁定，0 表示未锁定
 */
int mutex_is_locked(mutex_t *m);

/**
 * @brief 获取当前持有者 PID
 *
 * @param m Mutex 指针
 * @return 持有者 PID，-1 表示无持有者
 */
int mutex_owner(mutex_t *m);

/* ============================================================
 * 条件变量接口 (Condition Variables)
 * ============================================================ */

/**
 * @brief 条件变量结构体
 */
typedef struct {
    wait_queue_t wait_queue;   /**< 等待队列 */
    spinlock_t lock;           /**< 保护内部状态的自旋锁 */
} cond_var_t;

/**
 * @brief 初始化条件变量
 *
 * @param cv 条件变量指针
 */
void cond_init(cond_var_t *cv);

/**
 * @brief 等待条件变量
 *
 * 自动释放关联的 Mutex 并挂起当前进程。
 * 当被唤醒时，会重新获取 Mutex。
 *
 * @param cv 条件变量指针
 * @param m 关联的 Mutex 指针
 */
void cond_wait(cond_var_t *cv, mutex_t *m);

/**
 * @brief 带超时的条件等待
 *
 * @param cv 条件变量指针
 * @param m 关联的 Mutex 指针
 * @param timeout_ms 超时时间（毫秒）
 * @return 非0 表示被唤醒，0 表示超时
 */
int cond_wait_timeout(cond_var_t *cv, mutex_t *m, unsigned int timeout_ms);

/**
 * @brief 唤醒一个等待的进程
 *
 * @param cv 条件变量指针
 */
void cond_signal(cond_var_t *cv);

/**
 * @brief 唤醒所有等待的进程
 *
 * @param cv 条件变量指针
 */
void cond_broadcast(cond_var_t *cv);

/* ============================================================
 * 调试接口
 * ============================================================ */

#ifdef CONFIG_DEBUG_MUTEX

/**
 * @brief 断言当前进程持有该 Mutex
 *
 * 如果不持有，触发 panic。
 *
 * @param m Mutex 指针
 */
void mutex_assert_held(mutex_t *m);

/**
 * @brief 打印 Mutex 状态信息
 *
 * @param m Mutex 指针
 */
void mutex_dump(mutex_t *m);

#endif /* CONFIG_DEBUG_MUTEX */

#endif /* MUTEX_H */
