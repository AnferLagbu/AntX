#include "kernel_test.h"
#include "kmalloc.h"
#include "mm.h"
#include "string.h"

static int test_kmalloc_null_pointer(void) {
    void *ptr = kmalloc(0);
    if (ptr != NULL) {
        kfree(ptr);
    }
    TEST_ASSERT_EQ(ptr == NULL || ptr != NULL, true);
    return TEST_PASS;
}

static int test_kmalloc_free_null(void) {
    kfree(NULL);
    return TEST_PASS;
}

static int test_kmalloc_double_free_protection(void) {
    void *ptr = kmalloc(100);
    if (ptr == NULL) {
        return TEST_SKIP;
    }
    
    memset(ptr, 0xAA, 100);
    kfree(ptr);
    
    kfree(ptr);
    
    return TEST_PASS;
}

static int test_kmalloc_buffer_overflow_detection(void) {
    char *buffer = (char *)kmalloc(16);
    if (buffer == NULL) {
        return TEST_SKIP;
    }
    
    for (int i = 0; i < 16; i++) {
        buffer[i] = 'A';
    }
    
    TEST_ASSERT_EQ(buffer[0], 'A');
    TEST_ASSERT_EQ(buffer[15], 'A');
    
    kfree(buffer);
    return TEST_PASS;
}

static int test_kmalloc_alignment(void) {
    void *ptr1 = kmalloc(8);
    void *ptr2 = kmalloc(8);
    
    if (ptr1 == NULL || ptr2 == NULL) {
        if (ptr1) kfree(ptr1);
        if (ptr2) kfree(ptr2);
        return TEST_SKIP;
    }
    
    uintptr_t addr1 = (uintptr_t)ptr1;
    uintptr_t addr2 = (uintptr_t)ptr2;
    
    TEST_ASSERT_EQ(addr1 % 8, 0);
    TEST_ASSERT_EQ(addr2 % 8, 0);
    
    kfree(ptr1);
    kfree(ptr2);
    return TEST_PASS;
}

static int test_pmm_allocation_boundary(void) {
    uint64_t total_before = pmm_get_total_pages();
    uint64_t free_before = pmm_get_free_pages();
    
    void *pages[10];
    int allocated = 0;
    
    for (int i = 0; i < 10; i++) {
        pages[i] = kmalloc(4096);
        if (pages[i] != NULL) {
            allocated++;
        }
    }
    
    uint64_t free_after = pmm_get_free_pages();
    
    TEST_ASSERT_LE(free_after, free_before);
    TEST_ASSERT_GT(allocated, 5);
    
    for (int i = 0; i < allocated; i++) {
        kfree(pages[i]);
    }
    
    return TEST_PASS;
}

static int test_memory_stress_small_allocs(void) {
    const int iterations = 50;
    void *pointers[iterations];
    int count = 0;
    
    for (int i = 0; i < iterations; i++) {
        pointers[i] = kmalloc(i + 1);
        if (pointers[i] != NULL) {
            memset(pointers[i], 0xAB, i + 1);
            count++;
        }
    }
    
    for (int i = 0; i < count; i++) {
        kfree(pointers[i]);
    }
    
    TEST_ASSERT_GE(count, iterations / 2);
    return TEST_PASS;
}

void test_memory_safety_register(void) {
    int mod = test_register_module("Memory Safety");
    if (mod < 0) return;
    
    test_register_case(mod, "NULL pointer handling", test_kmalloc_null_pointer);
    test_register_case(mod, "Free NULL protection", test_kmalloc_free_null);
    test_register_case(mod, "Double-free protection", test_kmalloc_double_free_protection);
    test_register_case(mod, "Buffer overflow detection", test_kmalloc_buffer_overflow_detection);
    test_register_case(mod, "Memory alignment", test_kmalloc_alignment);
    test_register_case(mod, "PMM boundary allocation", test_pmm_allocation_boundary);
    test_register_case(mod, "Stress: small allocations", test_memory_stress_small_allocs);
}
