#ifndef _KERNEL_TEST_H
#define _KERNEL_TEST_H

#include "types.h"
#include "string.h"

#define TEST_PASS  1
#define TEST_FAIL  0
#define TEST_SKIP  -1

#define TEST_MODULE_MAX 32
#define TEST_CASE_MAX   64

typedef int (*test_func_t)(void);

struct test_case {
    const char *name;
    test_func_t func;
    int result;
    const char *message;
    uint64_t duration_us;
};

struct test_module {
    const char *name;
    struct test_case cases[TEST_CASE_MAX];
    int case_count;
    int passed;
    int failed;
    int skipped;
};

struct test_report {
    struct test_module modules[TEST_MODULE_MAX];
    int module_count;
    int total_passed;
    int total_failed;
    int total_skipped;
    uint64_t start_time;
    uint64_t end_time;
};

void test_framework_init(void);
int test_register_module(const char *name);
int test_register_case(int module_idx, const char *name, test_func_t func);
void test_run_all(void);
void test_run_module(int module_idx);
void test_print_report(void);
void test_set_message(int module_idx, int case_idx, const char *msg);

#define TEST_ASSERT(cond) do { \
    if (!(cond)) { \
        return TEST_FAIL; \
    } \
} while(0)

#define TEST_ASSERT_MSG(cond, msg) do { \
    if (!(cond)) { \
        test_set_message(current_module, current_case, msg); \
        return TEST_FAIL; \
    } \
} while(0)

#define TEST_ASSERT_EQ(a, b) TEST_ASSERT((a) == (b))
#define TEST_ASSERT_NE(a, b) TEST_ASSERT((a) != (b))
#define TEST_ASSERT_GT(a, b) TEST_ASSERT((a) > (b))
#define TEST_ASSERT_GE(a, b) TEST_ASSERT((a) >= (b))
#define TEST_ASSERT_LT(a, b) TEST_ASSERT((a) < (b))
#define TEST_ASSERT_LE(a, b) TEST_ASSERT((a) <= (b))
#define TEST_ASSERT_NOT_NULL(p) TEST_ASSERT((p) != NULL)
#define TEST_ASSERT_NULL(p) TEST_ASSERT((p) == NULL)
#define TEST_ASSERT_STR(a, b) TEST_ASSERT(strcmp((a), (b)) == 0)

extern int current_module;
extern int current_case;

void test_pmm_register(void);
void test_vmm_register(void);
void test_kmalloc_register(void);
void test_process_register(void);
void test_scheduler_register(void);
void test_vfs_register(void);
void test_syscall_register(void);
void test_ipc_register(void);
void test_hvfs_register(void);
void test_pwid_enhanced_register(void);
void test_persistence_register(void);

void run_kernel_tests(void);

#endif
