/**
 * @file atomic.c
 * @brief 原子操作辅助函数
 *
 * 提供一些不便用内联实现的原子操作辅助函数，
 * 以及原子操作的调试和统计功能。
 */

#include "atomic.h"
#include "klog.h"

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
    klog_kern("=== Atomic Operation Statistics ===");
    klog_kern("  inc operations: %d", atomic_stats.total_inc);
    klog_kern("  dec operations: %d", atomic_stats.total_dec);
    klog_kern("  cmpxchg success: %d", atomic_stats.total_cmpxchg_success);
    klog_kern("  cmpxchg fail: %d", atomic_stats.total_cmpxchg_fail);
}

#endif /* CONFIG_ATOMIC_STATS */
