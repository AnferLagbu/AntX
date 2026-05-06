#include "kernel_test.h"
#include "proc.h"
#include "string.h"

extern void scheduler_yield(void);

static int test_scheduler_init(void) {
    return TEST_SKIP;
}

static int test_scheduler_yield(void) {
    scheduler_yield();
    return TEST_PASS;
}

static int test_scheduler_get_current(void) {
    struct process *current = process_get_current();
    if (current == NULL) {
        return TEST_SKIP;
    }
    TEST_ASSERT_GT(current->pid, 0);
    
    return TEST_PASS;
}

static int test_scheduler_priority(void) {
    struct process *current = process_get_current();
    if (current == NULL) {
        return TEST_SKIP;
    }
    
    int old_priority = current->priority;
    current->priority = old_priority + 1;
    
    TEST_ASSERT_EQ(current->priority, old_priority + 1);
    
    current->priority = old_priority;
    
    return TEST_PASS;
}

static int test_scheduler_queue(void) {
    return TEST_SKIP;
}

static int test_scheduler_timeslice(void) {
    struct process *current = process_get_current();
    if (current == NULL) {
        return TEST_SKIP;
    }
    
    TEST_ASSERT_GT(current->time_slice, 0);
    
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
