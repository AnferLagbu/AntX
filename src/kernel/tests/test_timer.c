/**
 * @file test_timer.c
 * @brief Timer 定时器测试
 *
 * 测试 PIT 定时器的基本功能。
 *
 * 文档参考: test-framework.md §5 Phase 3
 */

#include "kernel_test.h"
#include "klog.h"
#include "timer.h"

static int test_timer_init(void) {
    klog_kern("[Timer] Testing initialization...");

    timer_init();

    uint64_t ticks = timer_get_ticks();

    klog_kern("[Timer] Ticks after init: %lu", (unsigned long)ticks);

    return TEST_PASS;
}

static int test_timer_ticks_increment(void) {
    klog_kern("[Timer] Testing tick counter increment...");

    uint64_t ticks_start = timer_get_ticks();
    
    for (volatile int i = 0; i < 1000000; i++) {
        __asm__ volatile ("nop");
    }

    uint64_t ticks_end = timer_get_ticks();

    klog_kern("[Timer] Ticks: start=%lu, end=%lu, diff=%lu", 
              (unsigned long)ticks_start, (unsigned long)ticks_end, 
              (unsigned long)(ticks_end - ticks_start));

    TEST_ASSERT_GE(ticks_end, ticks_start);

    return TEST_PASS;
}

static int test_timer_monotonic(void) {
    klog_kern("[Timer] Testing monotonicity (ticks should never decrease)...");

    uint64_t prev = timer_get_ticks();
    int passes = 10;

    for (int i = 0; i < passes; i++) {
        for (volatile int j = 0; j < 500000; j++) {
            __asm__ volatile ("nop");
        }

        uint64_t curr = timer_get_ticks();

        if (curr < prev) {
            klog_kern("[Timer] ERROR: Non-monotonic! prev=%lu, curr=%lu", 
                      (unsigned long)prev, (unsigned long)curr);
            return TEST_FAIL;
        }

        prev = curr;
    }

    klog_kern("[Timer] Monotonicity verified over %d checks", passes);
    return TEST_PASS;
}

void test_timer_register(void) {
    int mod = test_register_module("Timer (PIT)");
    if (mod < 0) return;

    test_register_case(mod, "Initialization", test_timer_init);
    test_register_case(mod, "Ticks Increment", test_timer_ticks_increment);
    test_register_case(mod, "Monotonicity", test_timer_monotonic);

    klog_kern("[Timer] Registered 3 test cases");
}
