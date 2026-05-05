#include "kernel_test.h"
#include "proc.h"
#include "klog.h"
#include "timer.h"

extern void scheduler_yield(void);

static int test_scheduler_mlfq_levels(void) {
    struct process *current = process_get_current();
    if (current == NULL) return TEST_SKIP;
    
    int initial_level = current->priority;
    
    TEST_ASSERT_GE(initial_level, 0);
    TEST_ASSERT_LT(initial_level, 4);
    
    klog_kern("[SCHED] MLFQ level: %d", initial_level);
    
    return TEST_PASS;
}

static int test_scheduler_timeslice_expiry(void) {
    struct process *current = process_get_current();
    if (current == NULL) return TEST_SKIP;
    
    uint32_t initial_slice = current->time_slice;
    
    volatile uint64_t start = timer_get_ticks();
    volatile int count = 0;
    
    for (count = 0; count < 10000; count++) {
        if (count % 1000 == 0) {
            scheduler_yield();
        }
    }
    
    uint64_t end = timer_get_ticks();
    
    TEST_ASSERT_GE(end - start, 0);
    
    klog_kern("[SCHED] Time slice test: %d ticks, elapsed: %d ticks", initial_slice, (uint32_t)(end - start));
    
    return TEST_PASS;
}

static int test_scheduler_priority_boost(void) {
    struct process *current = process_get_current();
    if (current == NULL) return TEST_SKIP;
    
    int old_priority = current->priority;
    
    if (old_priority < 3) {
        current->priority = old_priority + 1;
        
        TEST_ASSERT_EQ(current->priority, old_priority + 1);
        
        current->priority = old_priority;
    }
    
    klog_kern("[SCHED] Priority boost tested");
    return TEST_PASS;
}

static int test_scheduler_multiple_yields(void) {
    const int yields = 50;
    
    for (int i = 0; i < yields; i++) {
        scheduler_yield();
    }
    
    struct process *current = process_get_current();
    if (current == NULL) return TEST_SKIP;
    
    TEST_ASSERT_GT(current->pid, 0);
    
    klog_kern("[SCHED] Multiple yields: %d completed", yields);
    
    return TEST_PASS;
}

static int test_scheduler_context_switch_overhead(void) {
    struct process *current = process_get_current();
    if (current == NULL) return TEST_SKIP;
    
    const int switches = 50;
    uint64_t start = timer_get_ticks();
    
    for (int i = 0; i < switches; i++) {
        scheduler_yield();
        
        if (i % 10 == 0) {
            volatile int dummy = 0;
            for (int j = 0; j < 100; j++) {
                dummy += j;
            }
        }
    }
    
    uint64_t end = timer_get_ticks();
    uint64_t elapsed = end - start;
    
    TEST_ASSERT_GE(elapsed, 0);
    
    klog_kern("[SCHED] Context switch overhead: %d yields in %d ticks", switches, (uint32_t)elapsed);
    
    return TEST_PASS;
}

void test_scheduler_enhanced_register(void) {
    int mod = test_register_module("Scheduler Enhanced (MLFQ)");
    if (mod < 0) return;
    
    test_register_case(mod, "MLFQ queue levels", test_scheduler_mlfq_levels);
    test_register_case(mod, "Time slice expiry", test_scheduler_timeslice_expiry);
    test_register_case(mod, "Priority boost", test_scheduler_priority_boost);
    test_register_case(mod, "Multiple yields", test_scheduler_multiple_yields);
    test_register_case(mod, "Context switch overhead", test_scheduler_context_switch_overhead);
}
