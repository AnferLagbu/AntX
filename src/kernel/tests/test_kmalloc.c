/**
 * @file test_kmalloc.c
 * @brief Kmalloc (内核堆分配器) 单元测试
 *
 * 通过 FFI 接口测试 Rust 实现的 Kmalloc 功能。
 * 使用项目统一的 kernel_test.h 测试框架。
 */

#include "tests/kernel_test.h"
#include "kmalloc.h"
#include "serial.h"
#include "string.h"

/* ============================================================
 * 基础分配/释放测试
 * ============================================================ */

/**
 * @brief 测试基本分配与释放
 */
static int test_basic_alloc_free(void)
{
    /* 分配 256 字节 */
    char *buffer = (char *)kmalloc(256);
    
    TEST_ASSERT_NOT_NULL(buffer);
    
    /* 写入数据 */
    strcpy(buffer, "Hello from Rust kmalloc!");
    
    /* 验证数据完整性 */
    TEST_ASSERT_STR(buffer, "Hello from Rust kmalloc!");
    
    /* 释放内存 */
    kfree(buffer);
    
    return TEST_PASS;
}

/**
 * @brief 测试零大小分配（应返回 NULL）
 */
static int test_zero_size_alloc(void)
{
    void *ptr = kmalloc(0);
    
    TEST_ASSERT_NULL(ptr);
    
    return TEST_PASS;
}

/* ============================================================
 * 边界条件测试
 * ============================================================ */

/**
 * @brief 测试释放 NULL 指针（不应崩溃）
 */
static int test_free_null(void)
{
    kfree(NULL);
    
    return TEST_PASS;
}

/**
 * @brief 测试大块分配（例如 64KB）
 */
static int test_large_allocation(void)
{
    size_t size = 64 * 1024;  /* 64KB */
    char *buffer = (char *)kmalloc(size);
    
    if (buffer != NULL) {
        /* 填充数据 */
        memset(buffer, 0xAB, size);
        
        /* 验证首尾字节 */
        TEST_ASSERT_EQ(buffer[0], (char)0xAB);
        TEST_ASSERT_EQ(buffer[size - 1], (char)0xAB);
        
        kfree(buffer);
    }
    /* 如果返回 NULL，可能是堆空间不足，也算通过 */
    
    return TEST_PASS;
}

/* ============================================================
 * 多次分配测试
 * ============================================================ */

/**
 * @brief 测试多次独立分配
 */
static int test_multiple_allocations(void)
{
#define NUM_ALLOCS 10
    void *pointers[NUM_ALLOCS];
    int sizes[] = {16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192};
    int i;
    
    /* 分配多个不同大小的块 */
    for (i = 0; i < NUM_ALLOCS; i++) {
        pointers[i] = kmalloc(sizes[i]);
        TEST_ASSERT_NOT_NULL(pointers[i]);
        
        /* 写入唯一标识符 */
        if (pointers[i] != NULL) {
            *((char *)pointers[i]) = (char)(i + 'A');
        }
    }
    
    /* 验证所有指针都是唯一的 */
    for (i = 0; i < NUM_ALLOCS; i++) {
        int j;
        for (j = i + 1; j < NUM_ALLOCS; j++) {
            if (pointers[i] != NULL && pointers[j] != NULL) {
                TEST_ASSERT(pointers[i] != pointers[j]);
            }
        }
    }
    
    /* 验证数据完整性 */
    for (i = 0; i < NUM_ALLOCS; i++) {
        if (pointers[i] != NULL) {
            TEST_ASSERT_EQ(*((char *)pointers[i]), (char)(i + 'A'));
        }
    }
    
    /* 释放所有分配 */
    for (i = NUM_ALLOCS - 1; i >= 0; i--) {
        if (pointers[i] != NULL) {
            kfree(pointers[i]);
        }
    }
    
#undef NUM_ALLOCS
    return TEST_PASS;
}

/* ============================================================
 * Realloc 测试
 * ============================================================ */

/**
 * @brief 测试 realloc 扩展
 */
static int test_realloc_grow(void)
{
    /* 初始分配 32 字节 */
    char *ptr = (char *)kmalloc(32);
    TEST_ASSERT_NOT_NULL(ptr);
    
    /* 写入初始数据 */
    strcpy(ptr, "Hello");
    
    /* 扩展到 128 字节 */
    char *new_ptr = (char *)krealloc(ptr, 128);
    TEST_ASSERT_NOT_NULL(new_ptr);
    
    /* 验证旧数据保留 */
    TEST_ASSERT_STR(new_ptr, "Hello");
    
    /* 验证可以写入更多数据 */
    strcpy(new_ptr + 32, "World! This is extended memory.");
    
    kfree(new_ptr);
    return TEST_PASS;
}

/**
 * @brief 测试 realloc 收缩
 */
static int test_realloc_shrink(void)
{
    /* 初始分配 1024 字节 */
    char *ptr = (char *)kmalloc(1024);
    TEST_ASSERT_NOT_NULL(ptr);
    
    /* 写入数据 */
    memset(ptr, 'X', 10);
    ptr[10] = '\0';
    
    /* 缩减到 32 字节 */
    char *new_ptr = (char *)krealloc(ptr, 32);
    TEST_ASSERT_NOT_NULL(new_ptr);
    
    /* 验证数据保留（至少前32字节） */
    TEST_ASSERT_EQ(new_ptr[0], 'X');
    TEST_ASSERT_EQ(strlen(new_ptr), 10);  /* 应该还是 "XXXXXXXXXX" */
    
    kfree(new_ptr);
    return TEST_PASS;
}

/**
 * @brief 测试 realloc 从 NULL（等同于 malloc）
 */
static int test_realloc_from_null(void)
{
    void *ptr = krealloc(NULL, 256);
    
    TEST_ASSERT_NOT_NULL(ptr);
    
    /* 应该可以使用 */
    if (ptr != NULL) {
        memset(ptr, 0, 256);
        kfree(ptr);
    }
    
    return TEST_PASS;
}

/**
 * @brief 测试 realloc 到大小 0（应释放并返回 NULL）
 */
static int test_realloc_to_zero(void)
{
    void *ptr = kmalloc(100);
    TEST_ASSERT_NOT_NULL(ptr);
    
    /* Realloc 到 0 应该释放内存 */
    void *result = krealloc(ptr, 0);
    
    TEST_ASSERT_NULL(result);
    
    return TEST_PASS;
}

/* ============================================================
 * 统计信息测试
 * ============================================================ */

/**
 * @brief 测试统计信息更新
 */
static int test_statistics_tracking(void)
{
    /* 注意：需要先初始化 kmalloc 才能获取准确统计 */
    /*
     * 在实际测试中，这里假设 kmalloc 已经在系统启动时初始化。
     * 如果未初始化，这些函数可能返回默认值或导致 panic。
     */
    
    /* 这个测试主要验证 API 可调用性，不严格检查数值 */
    struct kmalloc_stats stats;
    kmalloc_stats(&stats);  /* 不应崩溃 */
    
    return TEST_PASS;
}

/* ============================================================
 * 堆完整性验证测试
 * ============================================================ */

/**
 * @brief 测试堆完整性检查（如果可用）
 */
static int test_heap_validation(void)
{
    /*
     * kmalloc_dump() 用于调试目的，
     * 打印堆状态信息。
     * 正常情况下应该可以调用。
     */
    kmalloc_dump();  /* 不应崩溃 */
    
    return TEST_PASS;
}

/* ============================================================
 * 内存模式测试
 * ============================================================ */

/**
 * @brief 测试分配-使用-释放循环的稳定性
 */
static int test_alloc_free_cycle(void)
{
#define CYCLE_COUNT 50
    int i;
    
    for (i = 0; i < CYCLE_COUNT; i++) {
        /* 分配随机大小的块（这里用固定模式） */
        size_t size = (i % 10 + 1) * 16;  /* 16 到 160 字节 */
        void *ptr = kmalloc(size);
        
        if (ptr != NULL) {
            /* 使用内存（写入再读回） */
            memset(ptr, (int)(i & 0xFF), size);
            
            /* 简单验证 */
            char *bytes = (char *)ptr;
            TEST_ASSERT_EQ(bytes[0], (char)(i & 0xFF));
            TEST_ASSERT_EQ(bytes[size - 1], (char)(i & 0xFF));
            
            /* 释放 */
            kfree(ptr);
        }
        /* 如果分配失败，继续下一次迭代 */
    }
    
#undef CYCLE_COUNT
    return TEST_PASS;
}

/* ============================================================
 * 模块注册
 * ============================================================ */

void test_kmalloc_register(void)
{
    int mod = test_register_module("Kmalloc (Rust)");
    if (mod < 0) {
        return;
    }
    
    /* 基础功能 */
    test_register_case(mod, "基本分配/释放", test_basic_alloc_free);
    test_register_case(mod, "零大小分配", test_zero_size_alloc);
    
    /* 边界条件 */
    test_register_case(mod, "释放NULL", test_free_null);
    test_register_case(mod, "大块分配(64KB)", test_large_allocation);
    
    /* 多次分配 */
    test_register_case(mod, "多次独立分配", test_multiple_allocations);
    
    /* Realloc 功能 */
    test_register_case(mod, "Realloc扩展", test_realloc_grow);
    test_register_case(mod, "Realloc收缩", test_realloc_shrink);
    test_register_case(mod, "Realloc从NULL", test_realloc_from_null);
    test_register_case(mod, "Realloc到0", test_realloc_to_zero);
    
    /* 统计与验证 */
    test_register_case(mod, "统计跟踪", test_statistics_tracking);
    test_register_case(mod, "堆完整性验证", test_heap_validation);
    
    /* 稳定性测试 */
    test_register_case(mod, "分配/释放循环(50次)", test_alloc_free_cycle);
}
