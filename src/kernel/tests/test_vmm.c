#include "kernel_test.h"
#include "mm.h"
#include "serial.h"
#include "string.h"

#define TEST_VIRT_ADDR  0xFFFF800002000000ULL
#define TEST_PATTERN    0xDEADBEEFCAFEBABEULL

static int test_vmm_map_page(void) {
    void *phys = pmm_alloc_page();
    TEST_ASSERT_NOT_NULL(phys);
    
    vmm_map_page(TEST_VIRT_ADDR, (uint64_t)phys, PAGE_PRESENT | PAGE_WRITABLE);
    
    volatile uint64_t *ptr = (volatile uint64_t *)TEST_VIRT_ADDR;
    *ptr = TEST_PATTERN;
    
    TEST_ASSERT_EQ(*ptr, TEST_PATTERN);
    
    vmm_unmap_page(TEST_VIRT_ADDR);
    pmm_free_page(phys);
    
    return TEST_PASS;
}

static int test_vmm_unmap_page(void) {
    void *phys = pmm_alloc_page();
    TEST_ASSERT_NOT_NULL(phys);
    
    vmm_map_page(TEST_VIRT_ADDR, (uint64_t)phys, PAGE_PRESENT | PAGE_WRITABLE);
    
    volatile uint64_t *ptr = (volatile uint64_t *)TEST_VIRT_ADDR;
    *ptr = TEST_PATTERN;
    TEST_ASSERT_EQ(*ptr, TEST_PATTERN);
    
    vmm_unmap_page(TEST_VIRT_ADDR);
    
    pmm_free_page(phys);
    
    return TEST_PASS;
}

static int test_vmm_multiple_mappings(void) {
    const int count = 5;
    void *phys_pages[count];
    uint64_t base_addr = 0xFFFF800003000000ULL;
    
    for (int i = 0; i < count; i++) {
        phys_pages[i] = pmm_alloc_page();
        TEST_ASSERT_NOT_NULL(phys_pages[i]);
        
        vmm_map_page(base_addr + i * PAGE_SIZE, (uint64_t)phys_pages[i], 
                     PAGE_PRESENT | PAGE_WRITABLE);
        
        volatile uint64_t *ptr = (volatile uint64_t *)(base_addr + i * PAGE_SIZE);
        *ptr = (uint64_t)i * 0x1000;
    }
    
    for (int i = 0; i < count; i++) {
        volatile uint64_t *ptr = (volatile uint64_t *)(base_addr + i * PAGE_SIZE);
        TEST_ASSERT_EQ(*ptr, (uint64_t)i * 0x1000);
    }
    
    for (int i = 0; i < count; i++) {
        vmm_unmap_page(base_addr + i * PAGE_SIZE);
        pmm_free_page(phys_pages[i]);
    }
    
    return TEST_PASS;
}

static int test_vmm_get_physical(void) {
    void *phys = pmm_alloc_page();
    TEST_ASSERT_NOT_NULL(phys);
    
    vmm_map_page(TEST_VIRT_ADDR, (uint64_t)phys, PAGE_PRESENT | PAGE_WRITABLE);
    
    uint64_t mapped_phys = vmm_get_physical(TEST_VIRT_ADDR);
    TEST_ASSERT_EQ(mapped_phys, (uint64_t)phys);
    
    vmm_unmap_page(TEST_VIRT_ADDR);
    pmm_free_page(phys);
    
    return TEST_PASS;
}

static int test_vmm_permissions(void) {
    void *phys = pmm_alloc_page();
    TEST_ASSERT_NOT_NULL(phys);
    
    vmm_map_page(TEST_VIRT_ADDR, (uint64_t)phys, PAGE_PRESENT | PAGE_WRITABLE);
    
    volatile uint64_t *ptr = (volatile uint64_t *)TEST_VIRT_ADDR;
    *ptr = TEST_PATTERN;
    TEST_ASSERT_EQ(*ptr, TEST_PATTERN);
    
    vmm_unmap_page(TEST_VIRT_ADDR);
    pmm_free_page(phys);
    
    return TEST_PASS;
}

void test_vmm_register(void) {
    int mod = test_register_module("VMM (Virtual Memory Manager)");
    
    test_register_case(mod, "Map single page", test_vmm_map_page);
    test_register_case(mod, "Unmap page", test_vmm_unmap_page);
    test_register_case(mod, "Multiple mappings", test_vmm_multiple_mappings);
    test_register_case(mod, "Get physical address", test_vmm_get_physical);
    test_register_case(mod, "Page permissions", test_vmm_permissions);
}
