/**
 * @file spinlock.c
 * @brief 自旋锁实现
 *
 * 实现 x86_64 架构下的高效自旋锁，使用 xchg 指令进行原子交换，
 * 并在自旋循环中插入 pause 指令以降低功耗和总线压力。
 */

#include "spinlock.h"
#include "klog.h"

/* ============================================================
 * 内联汇编辅助函数
 * ============================================================ */

/**
 * @brief 使用 xchg 指令原子交换值
 *
 * @param addr 目标地址
 * @param new_val 要写入的新值
 * @return 交换前的旧值
 */
static inline int xchg(int *addr, int new_val)
{
    int result;
    __asm__ volatile(
        "xchgl %0, %1"
        : "=r"(result), "+m"(*addr)
        : "0"(new_val)
        : "memory", "cc"
    );
    return result;
}

/**
 * @brief 禁用中断并返回中断标志
 *
 * @return 之前的中断标志 (IF位)
 */
static inline unsigned long local_irq_disable(void)
{
    unsigned long flags;
    __asm__ volatile(
        "pushfq\n\t"
        "popq %0\n\t"
        "cli\n\t"
        : "=r"(flags)
        :: "memory", "cc"
    );
    return flags;
}

/**
 * @brief 恢复中断标志
 *
 * @param flags 要恢复的中断标志
 */
static inline void local_irq_restore(unsigned long flags)
{
    if (flags & (1UL << 9)) {
        __asm__ volatile("sti" ::: "memory");
    }
}

/* ============================================================
 * 基础接口实现
 * ============================================================ */

void spin_init(spinlock_t *lock)
{
    lock->locked = 0;
#ifdef CONFIG_DEBUG_SPINLOCK
    lock->owner = NULL;
    lock->acquire_time = 0;
#endif
}

void spin_lock_raw(spinlock_t *lock)
{
    while (xchg(&lock->locked, 1) != 0) {
        __asm__ volatile("pause" ::: "memory");
    }

#ifdef CONFIG_DEBUG_SPINLOCK
    __asm__ volatile("mov %%rsp, %0" : "=r"(lock->owner));
    __asm__ volatile("rdtsc" : "=A"(lock->acquire_time));
#endif
}

void spin_unlock(spinlock_t *lock)
{
#ifdef CONFIG_DEBUG_SPINLOCK
    lock->owner = NULL;
    lock->acquire_time = 0;
#endif

    smp_wmb();
    lock->locked = 0;
}

int spin_trylock(spinlock_t *lock)
{
    if (xchg(&lock->locked, 1) == 0) {
#ifdef CONFIG_DEBUG_SPINLOCK
        __asm__ volatile("mov %%rsp, %0" : "=r"(lock->owner));
        __asm__ volatile("rdtsc" : "=A"(lock->acquire_time));
#endif
        return 1;
    }
    return 0;
}

int spin_is_locked(spinlock_t *lock)
{
    return lock->locked != 0;
}

void spin_assert_held(spinlock_t *lock)
{
#ifdef CONFIG_DEBUG_SPINLOCK
    if (!spin_is_locked(lock) || lock->owner == NULL) {
        klog_kern_err("SPINLOCK: ASSERT FAILED: lock not held");
        while (1) {
            __asm__ volatile("hlt");
        }
    }
#else
    (void)lock;
#endif
}

/* ============================================================
 * 中断安全版本实现
 * ============================================================ */

void spin_lock_irqsave_raw(spinlock_t *lock, unsigned long *flags)
{
    *flags = local_irq_disable();
    spin_lock_raw(lock);
}

void spin_unlock_irqrestore(spinlock_t *lock, unsigned long flags)
{
    spin_unlock(lock);
    local_irq_restore(flags);
}

void spin_lock_irq(spinlock_t *lock)
{
    local_irq_disable();
    spin_lock_raw(lock);
}

void spin_unlock_irq(spinlock_t *lock)
{
    spin_unlock(lock);
    __asm__ volatile("sti" ::: "memory");
}

/* ============================================================
 * 调试接口实现
 * ============================================================ */

#ifdef CONFIG_DEBUG_SPINLOCK

void spin_lock_debug(spinlock_t *lock, const char *file, int line)
{
    uint64_t start, end;

    __asm__ volatile("rdtsc" : "=A"(start));

    spin_lock_raw(lock);

    __asm__ volatile("rdtsc" : "=A"(end));

    klog_kern("SPINLOCK: Acquired: %s at %s:%d%s",
              lock->name ? lock->name : "(unnamed)",
              file, line,
              (end - start > 1000000ULL) ?
              " WARNING: long wait" : "");
}

void spin_lock_irqsave_debug(spinlock_t *lock, unsigned long *flags,
                              const char *file, int line)
{
    *flags = local_irq_disable();
    spin_lock_debug(lock, file, line);
}

#endif /* CONFIG_DEBUG_SPINLOCK */
