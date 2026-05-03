/**
 * @file rwlock.c
 * @brief 读写锁实现
 *
 * 实现写者优先的读写锁，防止写者饥饿。
 * 基于自旋锁保护内部状态，适用于读多写少场景。
 */

#include "rwlock.h"
#include "serial.h"

/* ============================================================
 * 初始化
 * ============================================================ */

void rwlock_init(rwlock_t *rw)
{
    spin_init(&rw->lock);
    rw->readers = 0;
    rw->writer = 0;
    rw->pending_writers = 0;
}

/* ============================================================
 * 读锁操作
 * ============================================================ */

void read_lock(rwlock_t *rw)
{
retry:
    spin_lock(&rw->lock);

    if (rw->writer) {
        spin_unlock(&rw->lock);
        while (rw->writer) {
            __asm__ volatile("pause" ::: "memory");
        }
        goto retry;
    }

    if (rw->pending_writers > 0) {
        spin_unlock(&rw->lock);
        __asm__ volatile("pause" ::: "memory");
        goto retry;
    }

    rw->readers++;
    spin_unlock(&rw->lock);
}

void read_unlock(rwlock_t *rw)
{
    spin_lock(&rw->lock);

    if (rw->readers > 0) {
        rw->readers--;
    } else {
        serial_puts(SERIAL_COM1, "[RWLOCK] ERROR: read_unlock without read_lock\n");
    }

    spin_unlock(&rw->lock);
}

int read_trylock(rwlock_t *rw)
{
    int success = 0;

    spin_lock(&rw->lock);

    if (!rw->writer && rw->pending_writers == 0) {
        rw->readers++;
        success = 1;
    }

    spin_unlock(&rw->lock);
    return success;
}

/* ============================================================
 * 写锁操作
 * ============================================================ */

void write_lock(rwlock_t *rw)
{
    spin_lock(&rw->lock);
    rw->pending_writers++;
    spin_unlock(&rw->lock);

retry:
    spin_lock(&rw->lock);

    if (rw->readers > 0 || rw->writer) {
        spin_unlock(&rw->lock);
        while (rw->readers > 0 || rw->writer) {
            __asm__ volatile("pause" ::: "memory");
        }
        goto retry;
    }

    rw->pending_writers--;
    rw->writer = 1;
    spin_unlock(&rw->lock);
}

void write_unlock(rwlock_t *rw)
{
    spin_lock(&rw->lock);

    if (rw->writer) {
        rw->writer = 0;
    } else {
        serial_puts(SERIAL_COM1, "[RWLOCK] ERROR: write_unlock without write_lock\n");
    }

    spin_unlock(&rw->lock);
}

int write_trylock(rwlock_t *rw)
{
    int success = 0;

    spin_lock(&rw->lock);

    if (!rw->writer && rw->readers == 0) {
        rw->writer = 1;
        success = 1;
    }

    spin_unlock(&rw->lock);
    return success;
}

/* ============================================================
 * 中断安全版本
 * ============================================================ */

void read_lock_irqsave(rwlock_t *rw, unsigned long *flags)
{
    unsigned long f;
    __asm__ volatile(
        "pushfq\n\t"
        "popq %0\n\t"
        "cli\n\t"
        : "=r"(f)
        :: "memory", "cc"
    );
    *flags = f;

    read_lock(rw);
}

void read_unlock_irqrestore(rwlock_t *rw, unsigned long flags)
{
    read_unlock(rw);

    if (flags & (1UL << 9)) {
        __asm__ volatile("sti" ::: "memory");
    }
}

void write_lock_irqsave(rwlock_t *rw, unsigned long *flags)
{
    unsigned long f;
    __asm__ volatile(
        "pushfq\n\t"
        "popq %0\n\t"
        "cli\n\t"
        : "=r"(f)
        :: "memory", "cc"
    );
    *flags = f;

    write_lock(rw);
}

void write_unlock_irqrestore(rwlock_t *rw, unsigned long flags)
{
    write_unlock(rw);

    if (flags & (1UL << 9)) {
        __asm__ volatile("sti" ::: "memory");
    }
}
