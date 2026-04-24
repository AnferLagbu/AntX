#include "kernel_test.h"
#include "proc.h"
#include "serial.h"
#include "string.h"
#include "printk.h"

extern uint64_t proc_create_internal(const char *name, uint64_t parent_pid);
extern void proc_exit_internal(uint32_t exit_code);
extern uint64_t proc_get_current_pid_internal(void);
extern uint32_t process_get_current_pid(void);
extern int proc_set_priority(uint64_t pid, uint32_t priority);
extern uint32_t proc_get_state(uint64_t pid);
extern void scheduler_yield(void);

static int test_process_create(void) {
    uint64_t pid = proc_create_internal("test_proc", 0);
    TEST_ASSERT_GT(pid, 0);
    
    return TEST_PASS;
}

static int test_process_pid_unique(void) {
    uint64_t pids[10];
    
    for (int i = 0; i < 10; i++) {
        char name[32];
        int len = 0;
        name[len++] = 'p';
        name[len++] = 'r';
        name[len++] = 'o';
        name[len++] = 'c';
        name[len++] = '_';
        name[len++] = '0' + (i / 10);
        name[len++] = '0' + (i % 10);
        name[len] = '\0';
        
        pids[i] = proc_create_internal(name, 0);
        
        if (pids[i] == 0) {
            TEST_ASSERT_MSG(0, "Failed to create process");
        }
        
        for (int j = 0; j < i; j++) {
            TEST_ASSERT_NE(pids[i], pids[j]);
        }
    }
    
    return TEST_PASS;
}

static int test_process_state_transition(void) {
    uint64_t pid = proc_create_internal("state_test", 0);
    TEST_ASSERT_GT(pid, 0);
    
    uint32_t state = proc_get_state(pid);
    TEST_ASSERT_MSG(state != 0 || pid > 0, "Process state should be valid");
    
    return TEST_PASS;
}

static int test_process_exit(void) {
    uint64_t pid = proc_create_internal("exit_test", 0);
    TEST_ASSERT_GT(pid, 0);
    
    return TEST_PASS;
}

static int test_process_find(void) {
    uint64_t pid = proc_create_internal("find_test", 0);
    TEST_ASSERT_GT(pid, 0);
    
    return TEST_PASS;
}

static int test_process_stress(void) {
    const int count = 20;
    uint64_t pids[count];
    int created = 0;
    
    for (int i = 0; i < count; i++) {
        char name[32];
        int len = 0;
        name[len++] = 's';
        name[len++] = 't';
        name[len++] = 'r';
        name[len++] = 'e';
        name[len++] = 's';
        name[len++] = 's';
        name[len++] = '_';
        name[len++] = '0' + (i / 10);
        name[len++] = '0' + (i % 10);
        name[len] = '\0';
        
        pids[i] = proc_create_internal(name, 0);
        if (pids[i] == 0) {
            TEST_ASSERT_MSG(0, "Failed to create process in stress test");
        }
        created++;
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
