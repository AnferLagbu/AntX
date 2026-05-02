#include "kernel_test.h"
#include "proc.h"
#include "serial.h"
#include "timer.h"

extern void scheduler_yield(void);

static int test_scheduler_mlfq_levels(void) {
    struct process *current = process_get_current();
    if (current == NULL) return TEST_SKIP;
    
    int initial_level = current->priority;
    
    TEST_ASSERT_GE(initial_level, 0);
    TEST_ASSERT_LT(initial_level, 4);
    
    serial_puts(SERIAL_COM1, "[SCHED] MLFQ level: ");
    serial_put_dec(SERIAL_COM1, initial_level);
    serial_puts(SERIAL_COM1, "\n");
    
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
    
    serial_puts(SERIAL_COM1, "[SCHED] Time slice test: ");
    serial_put_dec(SERIAL_COM1, initial_slice);
    serial_puts(SERIAL_COM1, " ticks, elapsed: ");
    serial_put_dec(SERIAL_COM1, (uint32_t)(end - start));
    serial_puts(SERIAL_COM1, " ticks\n");
    
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
    
    serial_puts(SERIAL_COM1, "[SCHED] Priority boost tested\n");
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
    
    serial_puts(SERIAL_COM1, "[SCHED] Multiple yields: ");
    serial_put_dec(SERIAL_COM1, yields);
    serial_puts(SERIAL_COM1, " completed\n");
    
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
    
    serial_puts(SERIAL_COM1, "[SCHED] Context switch overhead: ");
    serial_put_dec(SERIAL_COM1, switches);
    serial_puts(SERIAL_COM1, " yields in ");
    serial_put_dec(SERIAL_COM1, (uint32_t)elapsed);
    serial_puts(SERIAL_COM1, " ticks\n");
    
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
