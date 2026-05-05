/**
 * @file test_dma.c
 * @brief DMA 引擎单元测试
 *
 * 测试 DMA 内存分配、映射、同步和传输功能。
 */

#include "tests/kernel_test.h"
#include "dma.h"
#include "kmalloc.h"
#include "klog.h"

/* ============================================================
 * 初始化测试
 * ============================================================ */

/**
 * @brief 测试 DMA 子系统初始化
 */
static int test_dma_init(void)
{
    int result = dma_init();

    TEST_ASSERT_EQ(result, 0);

    return TEST_PASS;
}

/* ============================================================
 * 一致性内存管理测试
 * ============================================================ */

/**
 * @brief 测试一致性内存分配和释放
 */
static int test_coherent_alloc_free(void)
{
    void *ptr1 = dma_alloc_coherent(1024, 16);
    TEST_ASSERT_NOT_NULL(ptr1);

    void *ptr2 = dma_alloc_coherent(4096, 4096);
    TEST_ASSERT_NOT_NULL(ptr2);

    /* 验证对齐 */
    uintptr_t addr1 = (uintptr_t)ptr1;
    uintptr_t addr2 = (uintptr_t)ptr2;
    TEST_ASSERT((addr1 % 16) == 0);
    TEST_ASSERT((addr2 % 4096) == 0);

    /* 验证可写入 */
    memset(ptr1, 0xAA, 1024);
    memset(ptr2, 0xBB, 4096);

    uint8_t *p1 = (uint8_t *)ptr1;
    uint8_t *p2 = (uint8_t *)ptr2;
    TEST_ASSERT_EQ(p1[0], 0xAA);
    TEST_ASSERT_EQ(p1[1023], 0xAA);
    TEST_ASSERT_EQ(p2[0], 0xBB);
    TEST_ASSERT_EQ(p2[4095], 0xBB);

    dma_free_coherent(ptr1, 1024);
    dma_free_coherent(ptr2, 4096);

    return TEST_PASS;
}

/**
 * @brief 测试零大小分配
 */
static int test_zero_size_alloc(void)
{
    void *ptr = dma_alloc_coherent(0, 16);
    TEST_ASSERT_NULL(ptr);

    return TEST_PASS;
}

/**
 * @brief 测试设备地址获取
 */
static int test_device_address(void)
{
    void *cpu_addr = dma_alloc_coherent(256, 16);
    TEST_ASSERT_NOT_NULL(cpu_addr);

    uint64_t dev_addr = dma_get_device_address(cpu_addr);
    TEST_ASSERT(dev_addr != 0);

    dma_free_coherent(cpu_addr, 256);

    return TEST_PASS;
}

/* ============================================================
 * 流式 DMA 映射测试
 * ============================================================ */

/**
 * @brief 测试单页映射和取消映射
 */
static int test_single_map_unmap(void)
{
    void *buffer = kmalloc(2048);
    TEST_ASSERT_NOT_NULL(buffer);

    memset(buffer, 0xCC, 2048);

    dma_mapping_t *mapping = dma_map_single(buffer, 2048, DMA_TO_DEVICE);
    TEST_ASSERT_NOT_NULL(mapping);
    TEST_ASSERT(mapping->is_mapped == 1);
    TEST_ASSERT_EQ(mapping->size, 2048);
    TEST_ASSERT_EQ(mapping->direction, DMA_TO_DEVICE);

    dma_unmap_single(mapping);

    kfree(buffer);

    return TEST_PASS;
}

/**
 * @brief 测试不同方向的映射
 */
static int test_different_directions(void)
{
    void *buffer = kmalloc(512);
    TEST_ASSERT_NOT_NULL(buffer);

    /* To Device */
    dma_mapping_t *map1 = dma_map_single(buffer, 512, DMA_TO_DEVICE);
    TEST_ASSERT_NOT_NULL(map1);
    TEST_ASSERT_EQ(map1->direction, DMA_TO_DEVICE);
    dma_unmap_single(map1);

    /* From Device */
    dma_mapping_t *map2 = dma_map_single(buffer, 512, DMA_FROM_DEVICE);
    TEST_ASSERT_NOT_NULL(map2);
    TEST_ASSERT_EQ(map2->direction, DMA_FROM_DEVICE);
    dma_unmap_single(map2);

    /* Bidirectional */
    dma_mapping_t *map3 = dma_map_single(buffer, 512, DMA_BIDIRECTIONAL);
    TEST_ASSERT_NOT_NULL(map3);
    TEST_ASSERT_EQ(map3->direction, DMA_BIDIRECTIONAL);
    dma_unmap_single(map3);

    kfree(buffer);

    return TEST_PASS;
}

/**
 * @brief 测试无效参数的映射
 */
static int test_invalid_mapping_params(void)
{
    /* NULL 缓冲区 */
    dma_mapping_t *map1 = dma_map_single(NULL, 100, DMA_TO_DEVICE);
    TEST_ASSERT_NULL(map1);

    /* 零大小 */
    void *buffer = kmalloc(100);
    dma_mapping_t *map2 = dma_map_single(buffer, 0, DMA_TO_DEVICE);
    TEST_ASSERT_NULL(map2);

    /* 取消 NULL 映射 */
    dma_unmap_single(NULL);

    kfree(buffer);

    return TEST_PASS;
}

/* ============================================================
 * 散射-聚集列表测试
 * ============================================================ */

/**
 * @brief 测试散射-聚集列表初始化和添加条目
 */
static int test_sg_list_operations(void)
{
    dma_scatter_list_t sglist;
    char buf1[256];
    char buf2[512];

    dma_sg_init(&sglist);

    TEST_ASSERT_EQ(sglist.entry_count, 0);
    TEST_ASSERT_EQ(sglist.total_length, 0);

    int result;

    result = dma_sg_add_entry(&sglist, buf1, sizeof(buf1));
    TEST_ASSERT_EQ(result, 0);
    TEST_ASSERT_EQ(sglist.entry_count, 1);
    TEST_ASSERT_EQ(sglist.total_length, 256);

    result = dma_sg_add_entry(&sglist, buf2, sizeof(buf2));
    TEST_ASSERT_EQ(result, 0);
    TEST_ASSERT_EQ(sglist.entry_count, 2);
    TEST_ASSERT_EQ(sglist.total_length, 768);

    size_t total = dma_sg_total_length(&sglist);
    TEST_ASSERT_EQ(total, 768);

    return TEST_PASS;
}

/**
 * @brief 测试散射-聚集列表满的情况
 */
static int test_sg_list_full(void)
{
    dma_scatter_list_t sglist;
    char buf[64];

    dma_sg_init(&sglist);

    for (int i = 0; i < DMA_MAX_SCATTER_ENTRIES; i++) {
        int result = dma_sg_add_entry(&sglist, buf, sizeof(buf));
        TEST_ASSERT_EQ(result, 0);
    }

    /* 再添加应该失败 */
    int result = dma_sg_add_entry(&sglist, buf, sizeof(buf));
    TEST_ASSERT(result != 0);

    TEST_ASSERT_EQ(sglist.entry_count, DMA_MAX_SCATTER_ENTRIES);

    return TEST_PASS;
}

/**
 * @brief 测试散射-聚集映射
 */
static int test_sg_map_unmap(void)
{
    dma_scatter_list_t sglist;
    char buf1[128];
    char buf2[256];

    dma_sg_init(&sglist);
    dma_sg_add_entry(&sglist, buf1, sizeof(buf1));
    dma_sg_add_entry(&sglist, buf2, sizeof(buf2));

    dma_mapping_t *mapping = dma_map_sg(&sglist, DMA_TO_DEVICE);
    TEST_ASSERT_NOT_NULL(mapping);
    TEST_ASSERT(mapping->is_mapped == 1);

    dma_unmap_sg(mapping);

    return TEST_PASS;
}

/* ============================================================
 * 同步操作测试
 * ============================================================ */

/**
 * @brief 测试同步操作（不崩溃）
 */
static int test_sync_operations(void)
{
    void *buffer = kmalloc(1024);
    TEST_ASSERT_NOT_NULL(buffer);

    memset(buffer, 0xDD, 1024);

    dma_mapping_t *mapping = dma_map_single(buffer, 1024,
                                              DMA_BIDIRECTIONAL);
    TEST_ASSERT_NOT_NULL(mapping);

    /* 执行各种同步操作，不应崩溃 */
    dma_sync_for_device(mapping, 0, 1024);
    dma_sync_for_cpu(mapping, 0, 1024);
    dma_sync_for_cpu(mapping, 10, 500);
    dma_sync_both(mapping, 0, 1024);
    dma_sync_both(mapping, 100, 200);

    dma_unmap_single(mapping);
    kfree(buffer);

    return TEST_PASS;
}

/**
 * @brief 测试 NULL 映射的同步操作
 */
static int test_sync_null_mapping(void)
{
    /* 不应崩溃 */
    dma_sync_for_device(NULL, 0, 100);
    dma_sync_for_cpu(NULL, 0, 100);
    dma_sync_both(NULL, 0, 100);

    return TEST_PASS;
}

/* ============================================================
 * DMA 传输测试
 * ============================================================ */

/**
 * @brief 测试同步内存拷贝
 */
static int test_memcpy(void)
{
    void *src = dma_alloc_coherent(1024, 16);
    void *dst = dma_alloc_coherent(1024, 16);

    TEST_ASSERT_NOT_NULL(src);
    TEST_ASSERT_NOT_NULL(dst);

    memset(src, 0x12, 1024);
    memset(dst, 0x00, 1024);

    uint64_t src_phys = dma_get_device_address(src);
    uint64_t dst_phys = dma_get_device_address(dst);

    int result = dma_memcpy(dst_phys, src_phys, 1024, DMA_TO_DEVICE);
    TEST_ASSERT_EQ(result, 0);

    /* 验证数据正确性 */
    uint8_t *s = (uint8_t *)src;
    uint8_t *d = (uint8_t *)dst;
    for (int i = 0; i < 1024; i++) {
        if (d[i] != s[i]) {
            break;  /* 数据不匹配 */
        }
    }
    TEST_ASSERT(memcmp(dst, src, 1024) == 0);

    dma_free_coherent(src, 1024);
    dma_free_coherent(dst, 1024);

    return TEST_PASS;
}

/**
 * @brief 测试不同方向的内存拷贝
 */
static int test_memcpy_directions(void)
{
    void *buf1 = dma_alloc_coherent(256, 16);
    void *buf2 = dma_alloc_coherent(256, 16);

    TEST_ASSERT_NOT_NULL(buf1);
    TEST_ASSERT_NOT_NULL(buf2);

    uint64_t phys1 = dma_get_device_address(buf1);
    uint64_t phys2 = dma_get_device_address(buf2);

    /* To Device */
    memset(buf1, 0xAB, 256);
    int r1 = dma_memcpy(phys2, phys1, 256, DMA_TO_DEVICE);
    TEST_ASSERT_EQ(r1, 0);

    /* From Device */
    memset(buf1, 0xCD, 256);
    int r2 = dma_memcpy(phys2, phys1, 256, DMA_FROM_DEVICE);
    TEST_ASSERT_EQ(r2, 0);

    /* Bidirectional */
    memset(buf1, 0xEF, 256);
    int r3 = dma_memcpy(phys2, phys1, 256, DMA_BIDIRECTIONAL);
    TEST_ASSERT_EQ(r3, 0);

    dma_free_coherent(buf1, 256);
    dma_free_coherent(buf2, 256);

    return TEST_PASS;
}

/**
 * @brief 测试异步传输创建和销毁
 */
static int test_async_transfer_create_destroy(void)
{
    void *src = dma_alloc_coherent(512, 16);
    void *dst = dma_alloc_coherent(512, 16);

    TEST_ASSERT_NOT_NULL(src);
    TEST_ASSERT_NOT_NULL(dst);

    uint64_t src_phys = dma_get_device_address(src);
    uint64_t dst_phys = dma_get_device_address(dst);

    /* 创建同步传输请求 */
    dma_transfer_t *t1 = dma_create_transfer(src_phys, dst_phys,
                                                512, DMA_TO_DEVICE,
                                                NULL, NULL);
    TEST_ASSERT_NOT_NULL(t1);
    TEST_ASSERT(t1->synchronous == 1);

    dma_destroy_transfer(t1);

    /* 创建异步传输请求 */
    dma_transfer_t *t2 = dma_create_transfer(src_phys, dst_phys,
                                                512, DMA_FROM_DEVICE,
                                                NULL, (void*)0xDEAD);
    TEST_ASSERT_NOT_NULL(t2);
    TEST_ASSERT(t2->synchronous == 0);

    dma_destroy_transfer(t2);

    dma_free_coherent(src, 512);
    dma_free_coherent(dst, 512);

    return TEST_PASS;
}

/**
 * @brief 测试异步传输执行和等待
 */
static int test_async_transfer_execute_wait(void)
{
    void *src = dma_alloc_coherent(256, 16);
    void *dst = dma_alloc_coherent(256, 16);

    TEST_ASSERT_NOT_NULL(src);
    TEST_ASSERT_NOT_NULL(dst);

    memset(src, 0x55, 256);
    memset(dst, 0x00, 256);

    uint64_t src_phys = dma_get_device_address(src);
    uint64_t dst_phys = dma_get_device_address(dst);

    dma_transfer_t *transfer = dma_create_transfer(src_phys, dst_phys,
                                                     256, DMA_TO_DEVICE,
                                                     NULL, NULL);
    TEST_ASSERT_NOT_NULL(transfer);

    /* 启动异步传输 */
    int start_result = dma_async_memcpy(transfer);
    TEST_ASSERT_EQ(start_result, 0);

    /* 等待完成 */
    int wait_result = dma_wait_for_completion(transfer, 1000);
    TEST_ASSERT_EQ(wait_result, 0);
    TEST_ASSERT(transfer->completed == 1);
    TEST_ASSERT_EQ(transfer->result, 0);

    /* 验证数据 */
    TEST_ASSERT(memcmp(dst, src, 256) == 0);

    dma_destroy_transfer(transfer);
    dma_free_coherent(src, 256);
    dma_free_coherent(dst, 256);

    return TEST_PASS;
}

/**
 * @brief 测试超时等待
 */
static int test_timeout_wait(void)
{
    void *src = dma_alloc_coherent(64, 16);
    void *dst = dma_alloc_coherent(64, 16);

    TEST_ASSERT_NOT_NULL(src);
    TEST_ASSERT_NOT_NULL(dst);

    uint64_t src_phys = dma_get_device_address(src);
    uint64_t dst_phys = dma_get_device_address(dst);

    dma_transfer_t *transfer = dma_create_transfer(src_phys, dst_phys,
                                                     64, DMA_BIDIRECTIONAL,
                                                     NULL, NULL);
    TEST_ASSERT_NOT_NULL(transfer);

    /* 立即完成的传输不应超时 */
    dma_async_memcpy(transfer);
    int wait_result = dma_wait_for_completion(transfer, 1000);
    TEST_ASSERT_EQ(wait_result, 0);

    dma_destroy_transfer(transfer);
    dma_free_coherent(src, 64);
    dma_free_coherent(dst, 64);

    return TEST_PASS;
}

/* ============================================================
 * 统计信息测试
 * ============================================================ */

/**
 * @brief 测试统计信息接口
 */
static int test_statistics(void)
{
    dma_pool_stats_t stats_before, stats_after;

    dma_get_stats(&stats_before);

    /* 执行一些操作以改变统计 */
    void *ptr1 = dma_alloc_coherent(1024, 16);
    void *ptr2 = dma_alloc_coherent(2048, 32);

    void *buf = kmalloc(512);
    dma_mapping_t *m1 = dma_map_single(buf, 512, DMA_TO_DEVICE);
    dma_unmap_single(m1);

    dma_free_coherent(ptr1, 1024);
    dma_free_coherent(ptr2, 2048);
    kfree(buf);

    dma_get_stats(&stats_after);

    /* 验证统计变化合理 */
    TEST_ASSERT(stats_after.total_allocations >= stats_before.total_allocations);
    TEST_ASSERT(stats_after.total_frees >= stats_before.total_frees);
    TEST_ASSERT(stats_after.total_mappings >= stats_before.total_mappings);
    TEST_ASSERT(stats_after.total_unmappings >= stats_before.total_unmappings);

    return TEST_PASS;
}

/**
 * @brief 测试统计重置
 */
static int test_stats_reset(void)
{
    dma_pool_stats_t before, after;

    dma_get_stats(&before);

    dma_reset_stats();

    dma_get_stats(&after);

    TEST_ASSERT_EQ(after.total_allocations, 0);
    TEST_ASSERT_EQ(after.total_frees, 0);
    TEST_ASSERT_EQ(after.total_mappings, 0);
    TEST_ASSERT_EQ(after.total_bytes_allocated, 0);

    return TEST_PASS;
}

/* ============================================================
 * 调试输出测试
 * ============================================================ */

/**
 * @brief 测试调试输出（不崩溃）
 */
static int test_debug_output(void)
{
    /* 不应崩溃 */
    dma_dump_stats();
    dump_active_mappings();

    return TEST_PASS;
}

/* ============================================================
 * 性能基准测试
 * ============================================================ */

/**
 * @brief 测试一致性分配性能
 */
static int test_coherent_alloc_performance(void)
{
    const int iterations = 10000;
    uint64_t start, end, elapsed;
    void **ptrs;

    ptrs = (void **)kmalloc(sizeof(void *) * iterations);
    TEST_ASSERT_NOT_NULL(ptrs);

    __asm__ volatile("rdtsc" : "=A"(start));

    for (int i = 0; i < iterations; i++) {
        ptrs[i] = dma_alloc_coherent(256, 16);
    }

    __asm__ volatile("rdtsc" : "=A"(end));

    elapsed = end - start;

    klog_kern("[性能] Coherent Alloc: %d 次，耗时 %d cycles/次", iterations, (uint32_t)(elapsed / iterations));

    for (int i = 0; i < iterations; i++) {
        if (ptrs[i]) {
            dma_free_coherent(ptrs[i], 256);
        }
    }

    kfree(ptrs);

    TEST_ASSERT(elapsed > 0);

    return TEST_PASS;
}

/**
 * @brief 测试 DMA 拷贝性能
 */
static int test_dma_copy_performance(void)
{
    void *src = dma_alloc_coherent(4096, 16);
    void *dst = dma_alloc_coherent(4096, 16);
    const int iterations = 5000;
    uint64_t start, end, elapsed;

    TEST_ASSERT_NOT_NULL(src);
    TEST_ASSERT_NOT_NULL(dst);

    uint64_t src_phys = dma_get_device_address(src);
    uint64_t dst_phys = dma_get_device_address(dst);

    memset(src, 0x42, 4096);

    __asm__ volatile("rdtsc" : "=A"(start));

    for (int i = 0; i < iterations; i++) {
        dma_memcpy(dst_phys, src_phys, 4096, DMA_TO_DEVICE);
    }

    __asm__ volatile("rdtsc" : "=A"(end));

    elapsed = end - start;

    klog_kern("[性能] DMA Memcpy: %d 次 (4KB)，耗时 %d cycles/次", iterations, (uint32_t)(elapsed / iterations));

    dma_free_coherent(src, 4096);
    dma_free_coherent(dst, 4096);

    TEST_ASSERT(elapsed > 0);

    return TEST_PASS;
}

/* ============================================================
 * 边界条件测试
 * ============================================================ */

/**
 * @brief 测试关闭状态下的操作
 */
static int test_shutdown_state(void)
{
    /* 先初始化 */
    dma_init();

    /* 关闭 */
    dma_shutdown();

    /* 关闭后操作应该安全但无效果 */
    void *ptr = dma_alloc_coherent(100, 16);
    /* 可能返回 NULL 或有效值，取决于实现 */

    if (ptr) {
        dma_free_coherent(ptr, 100);
    }

    /* 重新初始化以便后续测试 */
    dma_init();

    return TEST_PASS;
}

/**
 * @brief 测试大块内存分配
 */
static int test_large_allocation(void)
{
    size_t large_size = 65536;  /* 64KB */

    void *ptr = dma_alloc_coherent(large_size, 4096);
    TEST_ASSERT_NOT_NULL(ptr);

    if (ptr) {
        memset(ptr, 0x77, large_size);

        uint8_t *p = (uint8_t *)ptr;
        TEST_ASSERT_EQ(p[0], 0x77);
        TEST_ASSERT_EQ(p[large_size - 1], 0x77);

        dma_free_coherent(ptr, large_size);
    }

    return TEST_PASS;
}

/* ============================================================
 * 模块注册
 * ============================================================ */

void test_dma_register(void)
{
    int mod = test_register_module("DMA");
    if (mod < 0) {
        return;
    }

    test_register_case(mod, "初始化", test_dma_init);
    test_register_case(mod, "一致性分配释放", test_coherent_alloc_free);
    test_register_case(mod, "零大小分配", test_zero_size_alloc);
    test_register_case(mod, "设备地址", test_device_address);
    test_register_case(mod, "单页映射取消", test_single_map_unmap);
    test_register_case(mod, "不同方向映射", test_different_directions);
    test_register_case(mod, "无效参数映射", test_invalid_mapping_params);
    test_register_case(mod, "SG列表操作", test_sg_list_operations);
    test_register_case(mod, "SG列表满", test_sg_list_full);
    test_register_case(mod, "SG映射取消", test_sg_map_unmap);
    test_register_case(mod, "同步操作", test_sync_operations);
    test_register_case(mod, "NULL同步", test_sync_null_mapping);
    test_register_case(mod, "内存拷贝", test_memcpy);
    test_register_case(mod, "多方向拷贝", test_memcpy_directions);
    test_register_case(mod, "异步传输创建销毁", test_async_transfer_create_destroy);
    test_register_case(mod, "异步传输执行等待", test_async_transfer_execute_wait);
    test_register_case(mod, "超时等待", test_timeout_wait);
    test_register_case(mod, "统计信息", test_statistics);
    test_register_case(mod, "统计重置", test_stats_reset);
    test_register_case(mod, "调试输出", test_debug_output);
    test_register_case(mod, "一致性分配性能", test_coherent_alloc_performance);
    test_register_case(mod, "DMA拷贝性能", test_dma_copy_performance);
    test_register_case(mod, "关闭状态", test_shutdown_state);
    test_register_case(mod, "大块分配", test_large_allocation);
}
