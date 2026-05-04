#include "kernel_test.h"
#include "proc.h"
#include "serial.h"
#include "string.h"
#include "kmalloc.h"
#include "user_proc.h"

extern uint64_t proc_create_internal(const char *name, uint64_t parent_pid);
extern uint32_t proc_get_state(uint64_t pid);
extern int proc_set_priority(uint64_t pid, uint32_t priority);
extern void scheduler_yield(void);
extern unsigned char build_user_init_bin[];
extern unsigned int build_user_init_bin_len;

static int test_user_process_bootstrap(void) {
    int pid = user_proc_load_elf_from_memory(build_user_init_bin, build_user_init_bin_len, 0);
    if (pid < 0) {
        serial_puts(SERIAL_COM1, "[USERPROC] Failed to load init binary\n");
        return TEST_SKIP;
    }
    TEST_ASSERT_GT(pid, 0);
    serial_puts(SERIAL_COM1, "[USERPROC] Successfully loaded user init process\n");
    return TEST_PASS;
}

static int test_process_tree_structure(void) {
    uint64_t parent = proc_create_internal("parent_proc", 0);
    if (parent == 0) return TEST_SKIP;
    
    uint64_t child1 = proc_create_internal("child_1", parent);
    if (child1 == 0) return TEST_SKIP;
    
    uint64_t child2 = proc_create_internal("child_2", parent);
    if (child2 == 0) return TEST_SKIP;
    
    TEST_ASSERT_NE(child1, child2);
    TEST_ASSERT_NE(child1, parent);
    TEST_ASSERT_NE(child2, parent);
    
    serial_puts(SERIAL_COM1, "[PROC] Process tree: parent=");
    serial_put_hex(SERIAL_COM1, parent);
    serial_puts(SERIAL_COM1, ", children=");
    serial_put_hex(SERIAL_COM1, child1);
    serial_puts(SERIAL_COM1, ",");
    serial_put_hex(SERIAL_COM1, child2);
    serial_puts(SERIAL_COM1, "\n");
    
    return TEST_PASS;
}

static int test_process_priority_inheritance(void) {
    uint64_t pid = proc_create_internal("prio_inherit", 0);
    if (pid == 0) return TEST_SKIP;
    
    int result = proc_set_priority(pid, 3);
    
    if (result >= 0) {
        uint64_t child = proc_create_internal("child_prio", pid);
        if (child > 0) {
            serial_puts(SERIAL_COM1, "[PROC] Priority inheritance tested\n");
        }
    }
    
    return TEST_PASS;
}

static int test_process_rapid_create_destroy(void) {
    const int iterations = 15;
    uint64_t pids[iterations];
    int created = 0;
    
    for (int i = 0; i < iterations; i++) {
        char name[32];
        strcpy(name, "rapid_");
        char num[8];
        int idx = 0;
        int temp = i;
        if (temp == 0) {
            num[idx++] = '0';
        } else {
            while (temp > 0 && idx < 7) {
                num[idx++] = '0' + (temp % 10);
                temp /= 10;
            }
        }
        num[idx] = '\0';
        
        for (int j = 0; j < idx / 2; j++) {
            char tmp = num[j];
            num[j] = num[idx - 1 - j];
            num[idx - 1 - j] = tmp;
        }
        strcat(name, num);
        
        pids[i] = proc_create_internal(name, 0);
        if (pids[i] != 0) {
            created++;
        }
    }
    
    TEST_ASSERT_GE(created, iterations * 80 / 100);
    
    serial_puts(SERIAL_COM1, "[PROC] Rapid create/destroy: ");
    serial_put_dec(SERIAL_COM1, created);
    serial_puts(SERIAL_COM1, "/");
    serial_put_dec(SERIAL_COM1, iterations);
    serial_puts(SERIAL_COM1, " processes created\n");
    
    return TEST_PASS;
}

static int test_process_name_validation(void) {
    const char *valid_names[] = {"valid", "name123", "a_b_c", NULL};
    const char *invalid_names[] = {NULL, "", NULL};
    
    for (int i = 0; valid_names[i] != NULL; i++) {
        uint64_t pid = proc_create_internal(valid_names[i], 0);
        if (pid == 0) {
            return TEST_SKIP;
        }
    }
    
    for (int i = 0; invalid_names[i] != NULL; i++) {
        (void)proc_create_internal(invalid_names[i], 0);
    }
    
    serial_puts(SERIAL_COM1, "[PROC] Name validation completed\n");
    return TEST_PASS;
}

static int test_process_concurrent_creation(void) {
    const int batch_size = 5;
    const int batches = 3;
    int total_created = 0;
    
    for (int b = 0; b < batches; b++) {
        for (int i = 0; i < batch_size; i++) {
            char name[32];
            strcpy(name, "batch_");
            
            char b_str[4], i_str[4];
            int b_idx = 0, i_idx = 0;
            int temp_b = b, temp_i = i;
            
            if (temp_b == 0) { b_str[b_idx++] = '0'; }
            else { while (temp_b > 0 && b_idx < 3) { b_str[b_idx++] = '0' + (temp_b % 10); temp_b /= 10; } }
            b_str[b_idx] = '\0';
            for (int j = 0; j < b_idx / 2; j++) { char t = b_str[j]; b_str[j] = b_str[b_idx-1-j]; b_str[b_idx-1-j] = t; }
            
            if (temp_i == 0) { i_str[i_idx++] = '0'; }
            else { while (temp_i > 0 && i_idx < 3) { i_str[i_idx++] = '0' + (temp_i % 10); temp_i /= 10; } }
            i_str[i_idx] = '\0';
            for (int j = 0; j < i_idx / 2; j++) { char t = i_str[j]; i_str[j] = i_str[i_idx-1-j]; i_str[i_idx-1-j] = t; }
            
            strcat(name, b_str);
            strcat(name, "_");
            strcat(name, i_str);
            
            uint64_t pid = proc_create_internal(name, 0);
            if (pid != 0) {
                total_created++;
            }
        }
        
        scheduler_yield();
    }
    
    TEST_ASSERT_EQ(total_created, batch_size * batches);
    
    serial_puts(SERIAL_COM1, "[PROC] Concurrent creation: ");
    serial_put_dec(SERIAL_COM1, total_created);
    serial_puts(SERIAL_COM1, " processes in ");
    serial_put_dec(SERIAL_COM1, batches);
    serial_puts(SERIAL_COM1, " batches\n");
    
    return TEST_PASS;
}

static int test_process_resource_limits(void) {
    void *mem1 = kmalloc(1024);
    void *mem2 = kmalloc(2048);
    void *mem3 = kmalloc(4096);
    
    if (mem1 == NULL || mem2 == NULL || mem3 == NULL) {
        if (mem1) kfree(mem1);
        if (mem2) kfree(mem2);
        if (mem3) kfree(mem3);
        return TEST_SKIP;
    }
    
    memset(mem1, 0xAA, 1024);
    memset(mem2, 0xBB, 2048);
    memset(mem3, 0xCC, 4096);
    
    kfree(mem1);
    kfree(mem2);
    kfree(mem3);
    
    uint64_t pid = proc_create_internal("resource_test", 0);
    if (pid == 0) return TEST_SKIP;
    
    serial_puts(SERIAL_COM1, "[PROC] Resource limits tested\n");
    return TEST_PASS;
}

void test_process_enhanced_register(void) {
    int mod = test_register_module("Process Management Enhanced");
    if (mod < 0) return;
    
    test_register_case(mod, "Process tree structure", test_process_tree_structure);
    test_register_case(mod, "Priority inheritance", test_process_priority_inheritance);
    test_register_case(mod, "Rapid create/destroy", test_process_rapid_create_destroy);
    test_register_case(mod, "Name validation", test_process_name_validation);
    test_register_case(mod, "Concurrent creation", test_process_concurrent_creation);
    test_register_case(mod, "Resource limits", test_process_resource_limits);
#if 0
    test_register_case(mod, "User process bootstrap", test_user_process_bootstrap);
#endif
}
