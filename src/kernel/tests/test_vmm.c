/**
 * @file test_vmm.c
 * @brief VMM (Virtual Memory Manager) 单元测试
 *
 * 通过 FFI 接口测试 Rust 实现的 VMM 功能。
 * 使用项目统一的 kernel_test.h 测试框架。
 */

#include "tests/kernel_test.h"
#include "mm.h"
#include "serial.h"

/* ============================================================
 * 页表项 (PTE) 操作测试
 * ============================================================ */

/**
 * @brief 测试页表项基本操作（通过映射/查询验证）
 */
static int test_pte_operations(void)
{
    /* 分配物理页 */
    void *phys_page = pmm_alloc_page();
    TEST_ASSERT_NOT_NULL(phys_page);
    
    uint64_t virt_addr = 0xA0000000;
    uint64_t phys_addr = (uint64_t)phys_page;
    uint64_t flags = PAGE_PRESENT | PAGE_WRITABLE;
    
    /* 映射页面 */
    int result = vmm_map_page(virt_addr, phys_addr, flags);
    TEST_ASSERT_EQ(result, 0);
    
    /* 查询物理地址 */
    uint64_t mapped_phys = vmm_get_physical(virt_addr);
    TEST_ASSERT_EQ(mapped_phys, phys_addr);
    
    /* 取消映射 */
    vmm_unmap_page(virt_addr);
    
    /* 验证已取消映射 */
    mapped_phys = vmm_get_physical(virt_addr);
    TEST_ASSERT_EQ(mapped_phys, 0);  /* 未映射应返回 0 */
    
    /* 释放物理页 */
    pmm_free_page(phys_page);
    
    return TEST_PASS;
}

/* ============================================================
 * 地址转换测试
 * ============================================================ */

/**
 * @brief 测试虚拟地址到物理地址的转换
 */
static int test_address_translation(void)
{
    void *phys = pmm_alloc_page();
    TEST_ASSERT_NOT_NULL(phys);
    
    uint64_t test_virts[] = {
        0xB0000000,
        0xC0001000,
        0xD0002000
    };
    int i;
    
    for (i = 0; i < 3; i++) {
        int ret = vmm_map_page(test_virts[i], (uint64_t)phys, PAGE_PRESENT);
        TEST_ASSERT_EQ(ret, 0);
        
        uint64_t result = vmm_get_physical(test_virts[i]);
        TEST_ASSERT_EQ(result, (uint64_t)phys);
        
        vmm_unmap_page(test_virts[i]);
    }
    
    pmm_free_page(phys);
    return TEST_PASS;
}

/* ============================================================
 * 用户空间页表测试
 * ============================================================ */

/**
 * @brief 测试用户空间页表创建与销毁
 */
static int test_user_page_table(void)
{
    /* 创建用户页表 */
    uint64_t user_pml4 = vmm_create_user_page_table();
    
    if (user_pml4 != 0) {
        /* 在用户页表中映射页面 */
        void *phys = pmm_alloc_page();
        if (phys != NULL) {
            vmm_map_page_in_table(user_pml4, 0x100000, (uint64_t)phys, 
                                  PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
            
            /* 在该页表上下文中查询 */
            uint64_t result = vmm_get_physical_in_table(user_pml4, 0x100000);
            TEST_ASSERT_EQ(result, (uint64_t)phys);
            
            pmm_free_page(phys);
        }
        
        /* 销毁用户页表 */
        vmm_destroy_page_table(user_pml4);
    }
    
    return TEST_PASS;
}

/* ============================================================
 * 页表切换测试
 * ============================================================ */

/**
 * @brief 测试 CR3 切换（保存/恢复）
 */
static int test_page_table_switch(void)
{
    /* 获取当前 PML4（假设 kernel_pml4 已设置） */
    uint64_t current_cr3 = kernel_pml4;  /* 全局变量，由 VMM 维护 */
    
    if (current_cr3 == 0) {
        /* 如果尚未初始化，先初始化 VMM */
        vmm_init();
        current_cr3 = kernel_pml4;
    }
    
    TEST_ASSERT(current_cr3 != 0);
    
    /* 创建新页表并切换 */
    uint64_t new_pml4 = vmm_create_user_page_table();
    if (new_pml4 != 0) {
        /* 切换到新页表 */
        vmm_switch_page_table(new_pml4);
        
        /* 切回内核页表 */
        vmm_switch_page_table(current_cr3);
        
        /* 清理 */
        vmm_destroy_page_table(new_pml4);
    }
    
    return TEST_PASS;
}

/* ============================================================
 * 大页映射测试
 * ============================================================ */

/**
 * @brief 测试 2MB 大页映射
 */
static int test_huge_page_mapping(void)
{
    void *huge_phys = pmm_alloc_huge_page(PAGE_SIZE_2M);
    
    if (huge_phys != NULL) {
        uint64_t huge_virt = 0xE0000000;  /* 2MB 对齐的地址 */
        
        int ret = vmm_map_huge_page(huge_virt, (uint64_t)huge_phys, 
                                    PAGE_PRESENT | PAGE_WRITABLE, PAGE_SIZE_2M);
        TEST_ASSERT_EQ(ret, 0);
        
        /* 验证映射 */
        uint64_t result = vmm_get_physical(huge_virt);
        TEST_ASSERT_EQ(result, (uint64_t)huge_phys);
        
        /* 清理 */
        vmm_unmap_page(huge_virt);
        pmm_free_huge_page(huge_phys, PAGE_SIZE_2M);
    } else {
        /* 内存不足时跳过此测试 */
        serial_puts(SERIAL_COM1, "[SKIP] 无法分配 2MB 大页\n");
    }
    
    return TEST_PASS;
}

/* ============================================================
 * 边界条件测试
 * ============================================================ */

/**
 * @brief 测试重复映射同一地址
 */
static int test_remap_same_address(void)
{
    void *phys1 = pmm_alloc_page();
    void *phys2 = pmm_alloc_page();
    
    TEST_ASSERT_NOT_NULL(phys1);
    TEST_ASSERT_NOT_NULL(phys2);
    
    uint64_t virt = 0xF0000000;
    
    /* 第一次映射 */
    int ret = vmm_map_page(virt, (uint64_t)phys1, PAGE_PRESENT);
    TEST_ASSERT_EQ(ret, 0);
    TEST_ASSERT_EQ(vmm_get_physical(virt), (uint64_t)phys1);
    
    /* 第二次映射同一地址（应该更新） */
    ret = vmm_map_page(virt, (uint64_t)phys2, PAGE_PRESENT | PAGE_WRITABLE);
    TEST_ASSERT_EQ(ret, 0);
    TEST_ASSERT_EQ(vmm_get_physical(virt), (uint64_t)phys2);
    
    /* 清理 */
    vmm_unmap_page(virt);
    pmm_free_page(phys1);
    pmm_free_page(phys2);
    
    return TEST_PASS;
}

/**
 * @brief 测试未映射地址的查询
 */
static int test_query_unmapped_address(void)
{
    /* 查询一个不太可能被使用的地址 */
    uint64_t result = vmm_get_physical(0xFFF00000);
    
    /* 应该返回 0（未映射） */
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

/* ============================================================
 * 权限标志测试
 * ============================================================ */

/**
 * @brief 测试不同权限标志的组合
 */
static int test_permission_flags(void)
{
    void *phys = pmm_alloc_page();
    TEST_ASSERT_NOT_NULL(phys);
    
    uint64_t base_virt = 0x80000000;
    
    /* 测试只读映射 */
    int ret = vmm_map_page(base_virt, (uint64_t)phys, PAGE_PRESENT);
    TEST_ASSERT_EQ(ret, 0);
    vmm_unmap_page(base_virt);
    
    /* 测试读写映射 */
    ret = vmm_map_page(base_virt + 0x1000, (uint64_t)phys, 
                       PAGE_PRESENT | PAGE_WRITABLE);
    TEST_ASSERT_EQ(ret, 0);
    vmm_unmap_page(base_virt + 0x1000);
    
    /* 测试用户可访问映射 */
    ret = vmm_map_page(base_virt + 0x2000, (uint64_t)phys, 
                       PAGE_PRESENT | PAGE_USER);
    TEST_ASSERT_EQ(ret, 0);
    vmm_unmap_page(base_virt + 0x2000);
    
    pmm_free_page(phys);
    return TEST_PASS;
}

/* ============================================================
 * 模块注册
 * ============================================================ */

void test_vmm_register(void)
{
    int mod = test_register_module("VMM (Rust)");
    if (mod < 0) {
        return;
    }
    
    /* 页表项操作 */
    test_register_case(mod, "PTE基本操作", test_pte_operations);
    
    /* 地址转换 */
    test_register_case(mod, "地址转换", test_address_translation);
    
    /* 用户空间 */
    test_register_case(mod, "用户页表", test_user_page_table);
    
    /* 页表切换 */
    test_register_case(mod, "CR3切换", test_page_table_switch);
    
    /* 大页支持 */
    test_register_case(mod, "2MB大页映射", test_huge_page_mapping);
    
    /* 边界条件 */
    test_register_case(mod, "重复映射", test_remap_same_address);
    test_register_case(mod, "未映射查询", test_query_unmapped_address);
    
    /* 权限标志 */
    test_register_case(mod, "权限标志", test_permission_flags);
}
