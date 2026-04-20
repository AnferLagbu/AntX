#include "kernel_test.h"
#include "kmalloc.h"
#include "serial.h"
#include "string.h"

static int test_kmalloc_basic(void) {
    void *ptr = kmalloc(64);
    TEST_ASSERT_NOT_NULL(ptr);
    
    kfree(ptr);
    
    return TEST_PASS;
}

static int test_kmalloc_multiple(void) {
    void *ptrs[10];
    
    for (int i = 0; i < 10; i++) {
        ptrs[i] = kmalloc(64 * (i + 1));
        TEST_ASSERT_NOT_NULL(ptrs[i]);
    }
    
    for (int i = 0; i < 10; i++) {
        kfree(ptrs[i]);
    }
    
    return TEST_PASS;
}

static int test_kmalloc_large(void) {
    void *ptr = kmalloc(4096 * 4);
    TEST_ASSERT_NOT_NULL(ptr);
    
    memset(ptr, 0xAA, 4096 * 4);
    
    kfree(ptr);
    
    return TEST_PASS;
}

static int test_krealloc(void) {
    void *ptr = kmalloc(64);
    TEST_ASSERT_NOT_NULL(ptr);
    
    memset(ptr, 0xBB, 64);
    
    void *new_ptr = krealloc(ptr, 128);
    TEST_ASSERT_NOT_NULL(new_ptr);
    
    kfree(new_ptr);
    
    return TEST_PASS;
}

static int test_kcalloc(void) {
    void *ptr = kcalloc(10, 64);
    TEST_ASSERT_NOT_NULL(ptr);
    
    unsigned char *bytes = (unsigned char *)ptr;
    for (int i = 0; i < 640; i++) {
        TEST_ASSERT_EQ(bytes[i], 0);
    }
    
    kfree(ptr);
    
    return TEST_PASS;
}

static int test_kmalloc_stress(void) {
    void *ptrs[100];
    
    for (int i = 0; i < 100; i++) {
        ptrs[i] = kmalloc((i % 10 + 1) * 64);
        if (ptrs[i] == NULL) {
            for (int j = 0; j < i; j++) {
                kfree(ptrs[j]);
            }
            TEST_ASSERT_MSG(0, "Stress allocation failed");
        }
    }
    
    for (int i = 0; i < 100; i++) {
        kfree(ptrs[i]);
    }
    
    return TEST_PASS;
}

static int test_kmalloc_stats(void) {
    struct kmalloc_stats stats;
    kmalloc_stats(&stats);
    
    TEST_ASSERT_GT(stats.heap_size, 0);
    
    return TEST_PASS;
}

void test_kmalloc_register(void) {
    int mod = test_register_module("Kernel Heap (kmalloc)");
    
    test_register_case(mod, "Basic allocation", test_kmalloc_basic);
    test_register_case(mod, "Multiple allocations", test_kmalloc_multiple);
    test_register_case(mod, "Large allocation (16KB)", test_kmalloc_large);
    test_register_case(mod, "Reallocation", test_krealloc);
    test_register_case(mod, "Zero-initialized allocation", test_kcalloc);
    test_register_case(mod, "Stress test (100 allocations)", test_kmalloc_stress);
    test_register_case(mod, "Heap statistics", test_kmalloc_stats);
}
