#include "kernel_test.h"
#include "mm.h"
#include "kmalloc.h"
#include "serial.h"
#include "string.h"

static int test_pmm_alloc_single(void) {
    void *page = pmm_alloc_page();
    TEST_ASSERT_NOT_NULL(page);
    
    uint64_t addr = (uint64_t)page;
    TEST_ASSERT_EQ(addr % PAGE_SIZE, 0);
    TEST_ASSERT_GT(addr, 0);
    
    uint64_t free_before = pmm_get_free_pages();
    pmm_free_page(page);
    uint64_t free_after = pmm_get_free_pages();
    TEST_ASSERT_GE(free_after, free_before);
    
    return TEST_PASS;
}

static int test_pmm_alloc_multiple(void) {
    const int count = 10;
    void *pages[count];
    int allocated = 0;
    
    uint64_t free_before = pmm_get_free_pages();
    
    for (int i = 0; i < count; i++) {
        pages[i] = pmm_alloc_page();
        if (pages[i] == NULL) {
            for (int j = 0; j < allocated; j++) {
                pmm_free_page(pages[j]);
            }
            TEST_ASSERT_MSG(0, "Failed to allocate page");
        }
        allocated++;
        
        uint64_t addr = (uint64_t)pages[i];
        TEST_ASSERT_EQ(addr % PAGE_SIZE, 0);
    }
    
    uint64_t free_after_alloc = pmm_get_free_pages();
    TEST_ASSERT_LE(free_after_alloc, free_before);
    
    for (int i = 0; i < count; i++) {
        pmm_free_page(pages[i]);
    }
    
    uint64_t free_after_free = pmm_get_free_pages();
    TEST_ASSERT_GE(free_after_free, free_after_alloc);
    
    return TEST_PASS;
}

static int test_pmm_alloc_consecutive(void) {
    const size_t count = 4;
    
    void *pages = pmm_alloc_pages(count);
    TEST_ASSERT_NOT_NULL(pages);
    
    uint64_t base = (uint64_t)pages;
    TEST_ASSERT_EQ(base % PAGE_SIZE, 0);
    
    pmm_free_pages(pages, count);
    
    return TEST_PASS;
}

static int test_pmm_realloc_freed(void) {
    void *page1 = pmm_alloc_page();
    TEST_ASSERT_NOT_NULL(page1);
    
    pmm_free_page(page1);
    
    void *page2 = pmm_alloc_page();
    TEST_ASSERT_NOT_NULL(page2);
    
    pmm_free_page(page2);
    
    return TEST_PASS;
}

static int test_pmm_stress(void) {
    const int iterations = 50;
    void *pages[iterations];
    int allocated = 0;
    
    for (int i = 0; i < iterations; i++) {
        pages[i] = pmm_alloc_page();
        if (pages[i] == NULL) {
            for (int j = 0; j < allocated; j++) {
                pmm_free_page(pages[j]);
            }
            TEST_ASSERT_MSG(0, "Stress test allocation failed");
        }
        allocated++;
    }
    
    for (int i = 0; i < allocated; i++) {
        pmm_free_page(pages[i]);
    }
    
    return TEST_PASS;
}

static int test_pmm_boundary(void) {
    uint64_t total = pmm_get_total_pages();
    uint64_t free = pmm_get_free_pages();
    uint64_t used = pmm_get_used_pages();
    
    TEST_ASSERT_GT(total, 0);
    TEST_ASSERT_GT(free, 0);
    
    return TEST_PASS;
}

void test_pmm_register(void) {
    int mod = test_register_module("PMM (Physical Memory Manager)");
    
    test_register_case(mod, "Allocate single page", test_pmm_alloc_single);
    test_register_case(mod, "Allocate multiple pages", test_pmm_alloc_multiple);
    test_register_case(mod, "Allocate consecutive pages", test_pmm_alloc_consecutive);
    test_register_case(mod, "Reallocate freed page", test_pmm_realloc_freed);
    test_register_case(mod, "Stress test (50 allocations)", test_pmm_stress);
    test_register_case(mod, "Boundary check", test_pmm_boundary);
}
