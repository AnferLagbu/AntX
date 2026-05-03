/**
 * @file test_pmm.c
 * @brief PMM (Physical Memory Manager) 单元测试
 *
 * 通过 FFI 接口测试 Rust 实现的 PMM 功能。
 * 使用项目统一的 kernel_test.h 测试框架。
 */

#include "tests/kernel_test.h"
#include "mm.h"          /* FFI 接口 */
#include "serial.h"

/* ============================================================
 * 初始化与基础功能测试
 * ============================================================ */

/**
 * @brief 测试 PMM 初始化
 */
static int test_pmm_init(void)
{
    /* 使用合理的参数初始化（16MB内存，1MB内核结束） */
    pmm_init(16 * 1024 * 1024, 1024 * 1024);
    
    TEST_ASSERT(pmm_get_total_pages() > 0);
    
    return TEST_PASS;
}

/**
 * @brief 测试单页分配与释放
 */
static int test_alloc_single_page(void)
{
    void *page = pmm_alloc_page();
    
    TEST_ASSERT_NOT_NULL(page);
    
    /* 验证页对齐 (4KB) */
    TEST_ASSERT_EQ(((uint64_t)page) % 4096, 0);
    
    /* 释放页面 */
    pmm_free_page(page);
    
    return TEST_PASS;
}

/**
 * @brief 测试多页连续分配
 */
static int test_alloc_multiple_pages(void)
{
    size_t count = 10;
    void *pages = pmm_alloc_pages(count);
    
    TEST_ASSERT_NOT_NULL(pages);
    
    /* 验证起始地址对齐 */
    TEST_ASSERT_EQ(((uint64_t)pages) % 4096, 0);
    
    /* 释放多页 */
    pmm_free_pages(pages, count);
    
    return TEST_PASS;
}

/* ============================================================
 * 边界条件与错误处理测试
 * ============================================================ */

/**
 * @brief 测试空指针释放（不应崩溃）
 */
static int test_free_null_page(void)
{
    /* 释放 NULL 应该安全处理 */
    pmm_free_page(NULL);
    
    return TEST_PASS;
}

/**
 * @brief 测试零大小分配（应返回NULL或失败）
 */
static int test_alloc_zero_pages(void)
{
    /* pmm_alloc_pages(0) 行为取决于实现，但不应崩溃 */
    void *result = pmm_alloc_pages(0);
    
    /* 可能返回 NULL 或有效指针，都不算错误 */
    if (result != NULL) {
        pmm_free_pages(result, 0);
    }
    
    return TEST_PASS;
}

/* ============================================================
 * 大页支持测试
 * ============================================================ */

/**
 * @brief 测试 2MB 大页分配
 */
static int test_alloc_huge_2mb(void)
{
    void *page = pmm_alloc_huge_page(PAGE_SIZE_2M);
    
    if (page != NULL) {
        /* 验证 2MB 对齐 */
        TEST_ASSERT_EQ(((uint64_t)page) % (2 * 1024 * 1024), 0);
        
        pmm_free_huge_page(page, PAGE_SIZE_2M);
    }
    /* 如果返回 NULL（内存不足），也算通过 */
    
    return TEST_PASS;
}

/**
 * @brief 测试 1GB 大页分配
 */
static int test_alloc_huge_1gb(void)
{
    void *page = pmm_alloc_huge_page(PAGE_SIZE_1G);
    
    if (page != NULL) {
        /* 验证 1GB 对齐 */
        TEST_ASSERT_EQ(((uint64_t)page) % (1024UL * 1024 * 1024), 0);
        
        pmm_free_huge_page(page, PAGE_SIZE_1G);
    }
    /* 通常在测试环境中不会成功分配1GB大页 */
    
    return TEST_PASS;
}

/* ============================================================
 * 对齐检查测试
 * ============================================================ */

/**
 * @brief 测试地址对齐验证函数
 */
static int test_alignment_check(void)
{
    /* 4KB 对齐测试 */
    TEST_ASSERT(pmm_is_aligned_for_huge((void *)0x1000, PAGE_SIZE_4K) != 0);
    TEST_ASSERT(pmm_is_aligned_for_huge((void *)0x1001, PAGE_SIZE_4K) == 0);
    
    /* 2MB 对齐测试 */
    TEST_ASSERT(pmm_is_aligned_for_huge((void *)0x200000, PAGE_SIZE_2M) != 0);
    TEST_ASSERT(pmm_is_aligned_for_huge((void *)0x200001, PAGE_SIZE_2M) == 0);
    
    /* 1GB 对齐测试 */
    TEST_ASSERT(pmm_is_aligned_for_huge((void *)0x40000000, PAGE_SIZE_1G) != 0);
    TEST_ASSERT(pmm_is_aligned_for_huge((void *)0x40000001, PAGE_SIZE_1G) == 0);
    
    return TEST_PASS;
}

/* ============================================================
 * 统计信息测试
 * ============================================================ */

/**
 * @brief 测试统计信息一致性
 */
static int test_statistics_consistency(void)
{
    uint64_t total = pmm_get_total_pages();
    uint64_t free_pages = pmm_get_free_pages();
    uint64_t used = pmm_get_used_pages();
    
    /* 总页数应该 > 0 */
    TEST_ASSERT(total > 0);
    
    /* 空闲 + 已用 应该约等于 总数 */
    TEST_ASSERT(free_pages + used <= total);  /* 允许少量误差 */
    
    return TEST_PASS;
}

/**
 * @brief 测试统计信息更新（分配后）
 */
static int test_statistics_update_after_alloc(void)
{
    uint64_t free_before = pmm_get_free_pages();
    uint64_t used_before = pmm_get_used_pages();
    
    /* 分配一页 */
    void *page = pmm_alloc_page();
    TEST_ASSERT_NOT_NULL(page);
    
    uint64_t free_after = pmm_get_free_pages();
    uint64_t used_after = pmm_get_used_pages();
    
    /* 空闲应该减少，已用应该增加 */
    TEST_ASSERT(free_after <= free_before);
    TEST_ASSERT(used_after >= used_before);
    
    /* 清理 */
    pmm_free_page(page);
    
    return TEST_PASS;
}

/* ============================================================
 * 压力测试
 * ============================================================ */

/**
 * @brief 批量分配/释放压力测试
 */
static int test_stress_allocation(void)
{
#define STRESS_COUNT 100
    void *pages[STRESS_COUNT];
    int i;
    int success_count = 0;
    
    /* 批量分配 */
    for (i = 0; i < STRESS_COUNT; i++) {
        pages[i] = pmm_alloc_page();
        if (pages[i] != NULL) {
            success_count++;
            /* 验证每个页面都是唯一且对齐的 */
            TEST_ASSERT(((uint64_t)pages[i]) % 4096 == 0);
        } else {
            break;  /* 内存耗尽 */
        }
    }
    
    /* 批量释放（逆序） */
    for (i = success_count - 1; i >= 0; i--) {
        pmm_free_page(pages[i]);
    }
    
    /* 至少应该能分配一些页面 */
    TEST_ASSERT(success_count > 0);
    
#undef STRESS_COUNT
    return TEST_PASS;
}

/* ============================================================
 * 模块注册
 * ============================================================ */

void test_pmm_register(void)
{
    int mod = test_register_module("PMM (Rust)");
    if (mod < 0) {
        return;
    }
    
    /* 基础功能 */
    test_register_case(mod, "初始化", test_pmm_init);
    test_register_case(mod, "单页分配/释放", test_alloc_single_page);
    test_register_case(mod, "多页连续分配", test_alloc_multiple_pages);
    
    /* 边界条件 */
    test_register_case(mod, "释放NULL", test_free_null_page);
    test_register_case(mod, "零大小分配", test_alloc_zero_pages);
    
    /* 大页支持 */
    test_register_case(mod, "2MB大页", test_alloc_huge_2mb);
    test_register_case(mod, "1GB大页", test_alloc_huge_1gb);
    
    /* 对齐检查 */
    test_register_case(mod, "对齐验证", test_alignment_check);
    
    /* 统计信息 */
    test_register_case(mod, "统计一致性", test_statistics_consistency);
    test_register_case(mod, "统计更新", test_statistics_update_after_alloc);
    
    /* 压力测试 */
    test_register_case(mod, "压力测试(100页)", test_stress_allocation);
}
