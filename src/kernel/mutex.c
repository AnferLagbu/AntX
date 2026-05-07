/**
 * @file mutex.c
 * @brief 睡眠锁 (Mutex) 实现
 *
 * 基于等待队列实现睡眠锁，支持进程挂起/唤醒机制。
 * 适用于长时间持有临界区的场景（如文件系统操作、IO等）。
 *
 * 注意：当前版本为简化实现，完整功能需要调度器的完整支持。
 */

#include "mutex.h"
#include "proc.h"
#include "klog.h"

/* ============================================================
 * 等待队列内部实现 (简化版)
 * ============================================================ */

/**
 * @brief 初始化等待队列
 */
static void wait_queue_init(wait_queue_t *wq)
{
    spin_init(&wq->lock);
    wq->head = NULL;
    wq->tail = NULL;
}

/* ============================================================
 * Mutex 核心实现
 * ============================================================ */

void mutex_init(mutex_t *m)
{
    m->locked = 0;
    m->owner = -1;
    m->acquire_time = 0;
    m->depth = 0;
    wait_queue_init(&m->wait_queue);
    spin_init(&m->lock_spinlock);

#ifdef CONFIG_DEBUG_MUTEX
    m->name = NULL;
    m->file = NULL;
    m->line = 0;
#endif
}

void mutex_lock(mutex_t *m)
{
    struct process *current;

    /* Fast path: uncontended lock */
    spin_lock(&m->lock_spinlock);

    if (!m->locked) {
        current = process_get_current();
        m->owner = current ? (int)current->pid : 0;
        m->locked = 1;
        m->depth = 1;
        __asm__ volatile("rdtsc" : "=A"(m->acquire_time));
        spin_unlock(&m->lock_spinlock);
        return;
    }

#ifdef CONFIG_DEBUG_MUTEX
    klog_kern_warn("MUTEX: lock contention detected");
#endif

    spin_unlock(&m->lock_spinlock);

    /*
     * Slow path: lock is contended.
     * Yield to the scheduler so the lock holder gets CPU time
     * to finish its critical section and release the lock.
     * This avoids wasting CPU cycles on busy-waiting.
     */
    while (1) {
        spin_lock(&m->lock_spinlock);

        if (!m->locked) {
            current = process_get_current();
            m->owner = current ? (int)current->pid : 0;
            m->locked = 1;
            m->depth = 1;
            __asm__ volatile("rdtsc" : "=A"(m->acquire_time));
            spin_unlock(&m->lock_spinlock);
            return;
        }

        spin_unlock(&m->lock_spinlock);

        extern void scheduler_yield(void);
        scheduler_yield();
    }
}

int mutex_trylock(mutex_t *m)
{
    int success = 0;
    struct process *current;

    spin_lock(&m->lock_spinlock);

    if (!m->locked) {
        current = process_get_current();
        if (current) {
            m->owner = (int)current->pid;
        } else {
            m->owner = 0;
        }
        m->locked = 1;
        m->depth = 1;
        __asm__ volatile("rdtsc" : "=A"(m->acquire_time));
        success = 1;
    }

    spin_unlock(&m->lock_spinlock);
    return success;
}

int mutex_lock_timeout(mutex_t *m, unsigned int timeout_ms)
{
    uint64_t start, current;
    struct process *proc;

    __asm__ volatile("rdtsc" : "=A"(start));

    while (1) {
        if (mutex_trylock(m)) {
            return 1;
        }

        if (timeout_ms > 0) {
            __asm__ volatile("rdtsc" : "=A"(current));

            if ((current - start) > (uint64_t)timeout_ms * 2400000ULL) {
                return 0;
            }
        }

        __asm__ volatile("pause" ::: "memory");
    }
}

void mutex_unlock(mutex_t *m)
{
    spin_lock(&m->lock_spinlock);

#ifdef CONFIG_DEBUG_MUTEX
    if (!m->locked) {
        klog_kern_err("MUTEX: unlock on unlocked mutex");
    }
#endif

    if (m->depth > 0) {
        m->depth--;
    }

    if (m->depth == 0) {
        m->locked = 0;
        m->owner = -1;
        m->acquire_time = 0;
    }

    spin_unlock(&m->lock_spinlock);
}

int mutex_is_locked(mutex_t *m)
{
    return m->locked != 0;
}

int mutex_owner(mutex_t *m)
{
    return m->owner;
}

/* ============================================================
 * 条件变量实现 (简化版)
 * ============================================================ */

void cond_init(cond_var_t *cv)
{
    wait_queue_init(&cv->wait_queue);
    spin_init(&cv->lock);
}

void cond_wait(cond_var_t *cv, mutex_t *m)
{
    mutex_unlock(m);
    extern void scheduler_yield(void);
    scheduler_yield();
    mutex_lock(m);
    (void)cv;
}

int cond_wait_timeout(cond_var_t *cv, mutex_t *m, unsigned int timeout_ms)
{
    (void)timeout_ms;
    mutex_unlock(m);
    extern void scheduler_yield(void);
    scheduler_yield();
    mutex_lock(m);
    (void)cv;
    return 1;
}

void cond_signal(cond_var_t *cv)
{
    (void)cv;
}

void cond_broadcast(cond_var_t *cv)
{
    (void)cv;
}

/* ============================================================
 * 调试接口实现
 * ============================================================ */

#ifdef CONFIG_DEBUG_MUTEX

void mutex_assert_held(mutex_t *m)
{
    struct process *current = process_get_current();

    if (!m->locked || (current && m->owner != (int)current->pid)) {
        klog_kern_err("MUTEX: ASSERT FAILED: not held by current process");
        while (1) {
            __asm__ volatile("hlt");
        }
    }
}

void mutex_dump(mutex_t *m)
{
    klog_kern("=== Mutex Status ===");
    klog_kern("  Name: %s", m->name ? m->name : "(unnamed)");
    klog_kern("  Locked: %d", m->locked);
    klog_kern("  Owner PID: %d", m->owner);
    klog_kern("  Depth: %d", m->depth);

    if (m->file) {
        klog_kern("  Acquired at: %s:%d", m->file, m->line);
    }
}

#endif /* CONFIG_DEBUG_MUTEX */
