/**
 * @file atomic.c
 * @brief 原子操作辅助函数
 *
 * 提供一些不便用内联实现的原子操作辅助函数，
 * 以及原子操作的调试和统计功能。
 */

#include "atomic.h"
#include "serial.h"

/* ============================================================
 * 统计信息 (可选)
 * ============================================================ */

#ifdef CONFIG_ATOMIC_STATS

static struct {
    unsigned long total_inc;
    unsigned long total_dec;
    unsigned long total_cmpxchg_success;
    unsigned long total_cmpxchg_fail;
} atomic_stats;

void atomic_inc_stats(void)
{
    atomic_stats.total_inc++;
}

void atomic_dec_stats(void)
{
    atomic_stats.total_dec++;
}

void atomic_dump_stats(void)
{
    serial_puts(SERIAL_COM1, "\n=== Atomic Operation Statistics ===\n");
    serial_puts(SERIAL_COM1, "  inc operations: ");
    serial_put_dec(SERIAL_COM1, atomic_stats.total_inc);
    serial_puts(SERIAL_COM1, "\n  dec operations: ");
    serial_put_dec(SERIAL_COM1, atomic_stats.total_dec);
    serial_puts(SERIAL_COM1, "\n  cmpxchg success: ");
    serial_put_dec(SERIAL_COM1, atomic_stats.total_cmpxchg_success);
    serial_puts(SERIAL_COM1, "\n  cmpxchg fail: ");
    serial_put_dec(SERIAL_COM1, atomic_stats.total_cmpxchg_fail);
    serial_puts(SERIAL_COM1, "\n=====================================\n");
}

#endif /* CONFIG_ATOMIC_STATS */
