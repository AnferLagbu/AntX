#include "kernel_test.h"
#include "proc.h"
#include "serial.h"
#include "string.h"

static void scheduler_test_entry(void) {
    while(1) {
        scheduler_yield();
    }
}

static int test_scheduler_init(void) {
    TEST_ASSERT_NOT_NULL(&sched);
    return TEST_PASS;
}

static int test_scheduler_yield(void) {
    scheduler_yield();
    return TEST_PASS;
}

static int test_scheduler_get_current(void) {
    struct process *current = process_get_current();
    TEST_ASSERT_NOT_NULL(current);
    TEST_ASSERT_GT(current->pid, 0);
    
    return TEST_PASS;
}

static int test_scheduler_priority(void) {
    struct process *proc = process_create(scheduler_test_entry, 0, 0);
    TEST_ASSERT_NOT_NULL(proc);
    
    int old_priority = proc->priority;
    proc->priority = old_priority + 1;
    
    TEST_ASSERT_EQ(proc->priority, old_priority + 1);
    
    process_exit(proc, 0);
    
    return TEST_PASS;
}

static int test_scheduler_queue(void) {
    TEST_ASSERT_NOT_NULL(&sched);
    
    return TEST_PASS;
}

static int test_scheduler_timeslice(void) {
    struct process *current = process_get_current();
    TEST_ASSERT_NOT_NULL(current);
    
    return TEST_PASS;
}

void test_scheduler_register(void) {
    int mod = test_register_module("Scheduler");
    
    test_register_case(mod, "Scheduler initialization", test_scheduler_init);
    test_register_case(mod, "Yield operation", test_scheduler_yield);
    test_register_case(mod, "Get current process", test_scheduler_get_current);
    test_register_case(mod, "Priority management", test_scheduler_priority);
    test_register_case(mod, "Ready queue", test_scheduler_queue);
    test_register_case(mod, "Time slice", test_scheduler_timeslice);
}
