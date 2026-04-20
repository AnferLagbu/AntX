#include "kernel_test.h"
#include "proc.h"
#include "serial.h"
#include "string.h"

static void test_process_entry(void) {
    while(1) {
        scheduler_yield();
    }
}

static int test_process_create(void) {
    struct process *proc = process_create(test_process_entry, 0, 0);
    TEST_ASSERT_NOT_NULL(proc);
    TEST_ASSERT_GT(proc->pid, 0);
    TEST_ASSERT_EQ(proc->state, PROC_READY);
    
    return TEST_PASS;
}

static int test_process_pid_unique(void) {
    struct process *procs[10];
    uint64_t pids[10];
    
    for (int i = 0; i < 10; i++) {
        procs[i] = process_create(test_process_entry, 0, 0);
        if (procs[i] == NULL) {
            for (int j = 0; j < i; j++) {
                process_exit(procs[j], 0);
            }
            TEST_ASSERT_MSG(0, "Failed to create process");
        }
        pids[i] = procs[i]->pid;
        
        for (int j = 0; j < i; j++) {
            TEST_ASSERT_NE(pids[i], pids[j]);
        }
    }
    
    for (int i = 0; i < 10; i++) {
        process_exit(procs[i], 0);
    }
    
    return TEST_PASS;
}

static int test_process_state_transition(void) {
    struct process *proc = process_create(test_process_entry, 0, 0);
    TEST_ASSERT_NOT_NULL(proc);
    
    TEST_ASSERT_EQ(proc->state, PROC_READY);
    
    proc->state = PROC_RUNNING;
    TEST_ASSERT_EQ(proc->state, PROC_RUNNING);
    
    proc->state = PROC_BLOCKED;
    TEST_ASSERT_EQ(proc->state, PROC_BLOCKED);
    
    proc->state = PROC_READY;
    TEST_ASSERT_EQ(proc->state, PROC_READY);
    
    process_exit(proc, 0);
    
    return TEST_PASS;
}

static int test_process_exit(void) {
    struct process *proc = process_create(test_process_entry, 0, 0);
    TEST_ASSERT_NOT_NULL(proc);
    TEST_ASSERT_EQ(proc->state, PROC_READY);
    
    process_exit(proc, 0);
    
    return TEST_PASS;
}

static int test_process_find(void) {
    struct process *proc = process_create(test_process_entry, 0, 0);
    TEST_ASSERT_NOT_NULL(proc);
    
    uint64_t pid = proc->pid;
    
    struct process *found = process_find_by_pid(pid);
    TEST_ASSERT_NOT_NULL(found);
    TEST_ASSERT_EQ(found->pid, pid);
    
    struct process *invalid = process_find_by_pid(99999);
    TEST_ASSERT_NULL(invalid);
    
    process_exit(proc, 0);
    
    return TEST_PASS;
}

static int test_process_stress(void) {
    const int count = 20;
    struct process *procs[count];
    int created = 0;
    
    for (int i = 0; i < count; i++) {
        procs[i] = process_create(test_process_entry, 0, 0);
        if (procs[i] == NULL) {
            for (int j = 0; j < created; j++) {
                process_exit(procs[j], 0);
            }
            TEST_ASSERT_MSG(0, "Failed to create process in stress test");
        }
        created++;
    }
    
    for (int i = 0; i < created; i++) {
        process_exit(procs[i], 0);
    }
    
    return TEST_PASS;
}

void test_process_register(void) {
    int mod = test_register_module("Process Management");
    
    test_register_case(mod, "Create process", test_process_create);
    test_register_case(mod, "PID uniqueness", test_process_pid_unique);
    test_register_case(mod, "State transitions", test_process_state_transition);
    test_register_case(mod, "Process exit", test_process_exit);
    test_register_case(mod, "Find process by PID", test_process_find);
    test_register_case(mod, "Stress test (20 processes)", test_process_stress);
}
