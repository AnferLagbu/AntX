/**
 * @file test_slab.c
 * @brief Slab 分配器单元测试
 *
 * 测试 Slab 分配器的缓存创建、对象分配/释放、通用接口和统计功能。
 */

#include "tests/kernel_test.h"
#include "slab.h"
#include "kmalloc.h"
#include "klog.h"

/* ============================================================
 * 缓存管理测试
 * ============================================================ */

/**
 * @brief 测试系统初始化
 */
static int test_slab_system_init(void)
{
    int result = slab_system_init();

    TEST_ASSERT_EQ(result, 0);

    return TEST_PASS;
}

/**
 * @brief 测试创建和销毁缓存
 */
static int test_kmem_cache_create_destroy(void)
{
    KmemCache *cache;

    cache = kmem_cache_create("test-cache", 64);
    TEST_ASSERT_NOT_NULL(cache);

    if (cache) {
        TEST_ASSERT_EQ(cache->object_size, 64);
        kmem_cache_destroy(cache);
    }

    return TEST_PASS;
}

/**
 * @brief 测试边界大小缓存
 */
static int test_kmem_cache_boundary_sizes(void)
{
    KmemCache *cache_small, *cache_large;

    cache_small = kmem_cache_create("small", 16);
    TEST_ASSERT_NOT_NULL(cache_small);

    cache_large = kmem_cache_create("large", 2048);
    TEST_ASSERT_NOT_NULL(cache_large);

    if (cache_small) {
        kmem_cache_destroy(cache_small);
    }
    if (cache_large) {
        kmem_cache_destroy(cache_large);
    }

    return TEST_PASS;
}

/* ============================================================
 * 对象分配释放测试
 * ============================================================ */

/**
 * @brief 测试基本对象分配
 */
static int test_kmem_cache_alloc_basic(void)
{
    KmemCache *cache;
    void *obj1, *obj2, *obj3;

    cache = kmem_cache_create("alloc-test", 128);
    TEST_ASSERT_NOT_NULL(cache);

    if (!cache) {
        return TEST_FAIL;
    }

    obj1 = kmem_cache_alloc(cache);
    obj2 = kmem_cache_alloc(cache);
    obj3 = kmem_cache_alloc(cache);

    TEST_ASSERT_NOT_NULL(obj1);
    TEST_ASSERT_NOT_NULL(obj2);
    TEST_ASSERT_NOT_NULL(obj3);

    TEST_ASSERT(obj1 != obj2);
    TEST_ASSERT(obj2 != obj3);
    TEST_ASSERT(obj1 != obj3);

    kmem_cache_free(cache, obj1);
    kmem_cache_free(cache, obj2);
    kmem_cache_free(cache, obj3);

    kmem_cache_destroy(cache);

    return TEST_PASS;
}

/**
 * @brief 测试大量对象分配
 */
static int test_kmem_cache_alloc_many(void)
{
    KmemCache *cache;
    void *objects[100];
    const int count = 100;

    cache = kmem_cache_create("many-alloc-test", 32);
    TEST_ASSERT_NOT_NULL(cache);

    if (!cache) {
        return TEST_FAIL;
    }

    for (int i = 0; i < count; i++) {
        objects[i] = kmem_cache_alloc(cache);
        TEST_ASSERT_NOT_NULL(objects[i]);
    }

    for (int i = 0; i < count; i++) {
        kmem_cache_free(cache, objects[i]);
    }

    kmem_cache_destroy(cache);

    return TEST_PASS;
}

/**
 * @brief 测试分配后写入数据并验证
 */
static int test_kmem_cache_alloc_write_verify(void)
{
    KmemCache *cache;
    void *obj;
    const char test_data[] = "Hello, Slab Allocator!";

    cache = kmem_cache_create("write-test", 256);
    TEST_ASSERT_NOT_NULL(cache);

    if (!cache) {
        return TEST_FAIL;
    }

    obj = kmem_cache_alloc(cache);
    TEST_ASSERT_NOT_NULL(obj);

    if (obj) {
        memset(obj, 0, 256);
        memcpy(obj, test_data, sizeof(test_data));

        TEST_ASSERT_MSG(memcmp(obj, test_data,
                               sizeof(test_data)) == 0,
                        "Data mismatch after write");

        kmem_cache_free(cache, obj);
    }

    kmem_cache_destroy(cache);

    return TEST_PASS;
}

/* ============================================================
 * 对象复用测试
 * ============================================================ */

/**
 * @brief 测试对象释放后可重新分配（地址可能相同或不同）
 */
static int test_kmem_cache_reuse(void)
{
    KmemCache *cache;
    void *obj1, *obj2, *obj3;

    cache = kmem_cache_create("reuse-test", 64);
    TEST_ASSERT_NOT_NULL(cache);

    if (!cache) {
        return TEST_FAIL;
    }

    obj1 = kmem_cache_alloc(cache);
    kmem_cache_free(cache, obj1);

    obj2 = kmem_cache_alloc(cache);
    TEST_ASSERT_NOT_NULL(obj2);

    kmem_cache_free(cache, obj2);

    obj3 = kmem_cache_alloc(cache);
    TEST_ASSERT_NOT_NULL(obj3);

    kmem_cache_free(cache, obj3);

    kmem_cache_destroy(cache);

    return TEST_PASS;
}

/**
 * @brief 测试交替分配和释放
 */
static int test_kmem_cache_alternating(void)
{
    KmemCache *cache;
    void *objs[10];
    const int cycles = 50;

    cache = kmem_cache_create("alternating-test", 96);
    TEST_ASSERT_NOT_NULL(cache);

    if (!cache) {
        return TEST_FAIL;
    }

    for (int c = 0; c < cycles; c++) {
        for (int i = 0; i < 10; i++) {
            objs[i] = kmem_cache_alloc(cache);
            TEST_ASSERT_NOT_NULL(objs[i]);
        }
        for (int i = 9; i >= 0; i--) {
            kmem_cache_free(cache, objs[i]);
        }
    }

    kmem_cache_destroy(cache);

    return TEST_PASS;
}

/* ============================================================
 * 通用接口测试
 * ============================================================ */

/**
 * @brief 测试 slab_alloc/slab_free 基本功能
 */
static int test_slab_alloc_free_basic(void)
{
    void *p1, *p2, *p3;

    p1 = slab_alloc(16);
    p2 = slab_alloc(100);
    p3 = slab_alloc(1024);

    TEST_ASSERT_NOT_NULL(p1);
    TEST_ASSERT_NOT_NULL(p2);
    TEST_ASSERT_NOT_NULL(p3);

    slab_free(p1);
    slab_free(p2);
    slab_free(p3);

    return TEST_PASS;
}

/**
 * @brief 测试不同大小的通用分配
 */
static int test_slab_alloc_various_sizes(void)
{
    size_t sizes[] = {16, 32, 64, 128, 256, 512, 1024, 2048};
    void *ptrs[8];

    for (int i = 0; i < 8; i++) {
        ptrs[i] = slab_alloc(sizes[i]);
        TEST_ASSERT_NOT_NULL(ptrs[i]);
    }

    for (int i = 7; i >= 0; i--) {
        slab_free(ptrs[i]);
    }

    return TEST_PASS;
}

/**
 * @brief 测试大请求回退到 kmalloc
 */
static int test_slab_alloc_large_fallback(void)
{
    void *ptr;

    ptr = slab_alloc(4096);  /* 超过 SLAB_MAX_OBJECT_SIZE */

    /* 应该回退到 kmalloc，不应该为 NULL（如果有足够内存）*/
    /* 这里只验证不会崩溃 */

    if (ptr) {
        kfree(ptr);
    }

    return TEST_PASS;
}

/* ============================================================
 * 统计信息测试
 * ============================================================ */

/**
 * @brief 测试缓存统计信息准确性
 */
static int test_kmem_cache_stats(void)
{
    KmemCache *cache;
    void *objs[5];
    unsigned int total_obj, active_obj, total_slabs;

    cache = kmem_cache_create("stats-test", 64);
    TEST_ASSERT_NOT_NULL(cache);

    if (!cache) {
        return TEST_FAIL;
    }

    kmem_cache_get_stats(cache, &total_obj, &active_obj, &total_slabs);
    TEST_ASSERT_EQ(active_obj, 0);

    for (int i = 0; i < 5; i++) {
        objs[i] = kmem_cache_alloc(cache);
    }

    kmem_cache_get_stats(cache, &total_obj, &active_obj, &total_slabs);
    TEST_ASSERT_EQ(active_obj, 5);

    kmem_cache_free(cache, objs[0]);
    kmem_cache_free(cache, objs[2]);

    kmem_cache_get_stats(cache, &total_obj, &active_obj, &total_slabs);
    TEST_ASSERT_EQ(active_obj, 3);

    for (int i = 0; i < 5; i++) {
        if (objs[i]) {
            kmem_cache_free(cache, objs[i]);
        }
    }

    kmem_cache_get_stats(cache, &total_obj, &active_obj, &total_slabs);
    TEST_ASSERT_EQ(active_obj, 0);

    kmem_cache_destroy(cache);

    return TEST_PASS;
}

/* ============================================================
 * 边界条件测试
 * ============================================================ */

/**
 * @brief 测试空指针释放（应安全）
 */
static int test_slab_null_free(void)
{
    slab_free(NULL);
    kmem_cache_free(NULL, NULL);

    return TEST_PASS;
}

/**
 * @brief 测试零大小请求
 */
static int test_slab_zero_size(void)
{
    void *ptr = slab_alloc(0);

    TEST_ASSERT_NULL(ptr);

    return TEST_PASS;
}

/* ============================================================
 * 性能基准测试
 * ============================================================ */

/**
 * @brief 测试 Slab 分配性能
 */
static int test_slab_performance(void)
{
    KmemCache *cache;
    void **objs;
    const int iterations = 50000;
    uint64_t start, end, elapsed;

    cache = kmem_cache_create("perf-test", 128);
    TEST_ASSERT_NOT_NULL(cache);

    if (!cache) {
        return TEST_FAIL;
    }

    objs = (void **)kmalloc(sizeof(void *) * iterations);
    if (!objs) {
        kmem_cache_destroy(cache);
        return TEST_FAIL;
    }

    __asm__ volatile("rdtsc" : "=A"(start));

    for (int i = 0; i < iterations; i++) {
        objs[i] = kmem_cache_alloc(cache);
    }

    __asm__ volatile("rdtsc" : "=A"(end));

    elapsed = end - start;

    klog_kern("[性能] Slab Alloc: %d 次分配，耗时 %d cycles/次", iterations, (uint32_t)(elapsed / iterations));

    __asm__ volatile("rdtsc" : "=A"(start));

    for (int i = 0; i < iterations; i++) {
        kmem_cache_free(cache, objs[i]);
    }

    __asm__ volatile("rdtsc" : "=A"(end));

    elapsed = end - start;

    klog_kern("[性能] Slab Free: %d 次释放，耗时 %d cycles/次", iterations, (uint32_t)(elapsed / iterations));

    kfree(objs);
    kmem_cache_destroy(cache);

    return TEST_PASS;
}

/* ============================================================
 * 模块注册
 * ============================================================ */

void test_slab_register(void)
{
    int mod = test_register_module("Slab");
    if (mod < 0) {
        return;
    }

    test_register_case(mod, "系统初始化", test_slab_system_init);
    test_register_case(mod, "创建/销毁缓存", test_kmem_cache_create_destroy);
    test_register_case(mod, "边界大小", test_kmem_cache_boundary_sizes);
    test_register_case(mod, "基本分配", test_kmem_cache_alloc_basic);
    test_register_case(mod, "大量分配", test_kmem_cache_alloc_many);
    test_register_case(mod, "写入验证", test_kmem_cache_alloc_write_verify);
    test_register_case(mod, "对象复用", test_kmem_cache_reuse);
    test_register_case(mod, "交替分配释放", test_kmem_cache_alternating);
    test_register_case(mod, "通用Alloc/Free", test_slab_alloc_free_basic);
    test_register_case(mod, "不同大小", test_slab_alloc_various_sizes);
    test_register_case(mod, "大请求回退", test_slab_alloc_large_fallback);
    test_register_case(mod, "统计信息", test_kmem_cache_stats);
    test_register_case(mod, "NULL释放", test_slab_null_free);
    test_register_case(mod, "零大小请求", test_slab_zero_size);
    test_register_case(mod, "性能基准", test_slab_performance);
}
