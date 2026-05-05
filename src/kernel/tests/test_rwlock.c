/**
 * @file test_rwlock.c
 * @brief 读写锁单元测试
 *
 * 测试读写锁的基本功能、读者-写者语义和正确性。
 */

#include "tests/kernel_test.h"
#include "rwlock.h"
#include "klog.h"

/* ============================================================
 * 基础功能测试
 * ============================================================ */

/**
 * @brief 测试读写锁初始化
 */
static int test_rwlock_basic(void)
{
    rwlock_t rw = RWLOCK_INIT;

    read_lock(&rw);
    read_unlock(&rw);

    write_lock(&rw);
    write_unlock(&rw);

    return TEST_PASS;
}

/* ============================================================
 * 读锁测试
 * ============================================================ */

/**
 * @brief 测试基本读锁操作
 */
static int test_read_lock_basic(void)
{
    rwlock_t rw = RWLOCK_INIT;

    read_lock(&rw);
    read_unlock(&rw);

    return TEST_PASS;
}

/**
 * @brief 测试多次获取读锁（应允许）
 */
static int test_read_lock_multiple(void)
{
    rwlock_t rw = RWLOCK_INIT;
    const int readers = 10;

    for (int i = 0; i < readers; i++) {
        read_lock(&rw);
    }

    for (int i = 0; i < readers; i++) {
        read_unlock(&rw);
    }

    return TEST_PASS;
}

/**
 * @brief 测试读 trylock
 */
static int test_read_trylock(void)
{
    rwlock_t rw = RWLOCK_INIT;

    TEST_ASSERT(read_trylock(&rw) != 0);
    TEST_ASSERT(read_trylock(&rw) != 0);  /* 多读者允许 */

    read_unlock(&rw);
    read_unlock(&rw);

    return TEST_PASS;
}

/* ============================================================
 * 写锁测试
 * ============================================================ */

/**
 * @brief 测试基本写锁操作
 */
static int test_write_lock_basic(void)
{
    rwlock_t rw = RWLOCK_INIT;

    write_lock(&rw);
    write_unlock(&rw);

    return TEST_PASS;
}

/**
 * @brief 测试写 trylock
 */
static int test_write_trylock(void)
{
    rwlock_t rw = RWLOCK_INIT;

    TEST_ASSERT(write_trylock(&rw) != 0);
    write_unlock(&rw);

    return TEST_PASS;
}

/**
 * @brief 测试写锁互斥性（单线程下只能获取一次）
 */
static int test_write_lock_exclusive(void)
{
    rwlock_t rw = RWLOCK_INIT;

    write_lock(&rw);
    /* 单线程下再次获取会死锁，这里只验证状态 */
    write_unlock(&rw);

    return TEST_PASS;
}

/* ============================================================
 * 读者-写者互斥测试
 * ============================================================ */

/**
 * @brief 测试写者阻塞读者
 */
static int test_writer_blocks_reader(void)
{
    rwlock_t rw = RWLOCK_INIT;

    write_lock(&rw);

    /* 在持有写锁的情况下，读者应该被阻塞 */
    /* 但在单线程环境下无法真正测试阻塞行为 */
    /* 这里仅验证接口可用性 */

    write_unlock(&rw);

    read_lock(&rw);
    read_unlock(&rw);

    return TEST_PASS;
}

/**
 * @brief 测试读者阻塞写者
 */
static int test_reader_blocks_writer(void)
{
    rwlock_t rw = RWLOCK_INIT;

    read_lock(&rw);

    /* 在持有读锁的情况下，写者应该被阻塞 */
    /* 单线程环境无法真正测试 */

    read_unlock(&rw);

    write_lock(&rw);
    write_unlock(&rw);

    return TEST_PASS;
}

/* ============================================================
 * 中断安全版本测试
 * ============================================================ */

/**
 * @brief 测试读锁 irqsave/irqrestore
 */
static int test_read_lock_irqsave_restore(void)
{
    rwlock_t rw = RWLOCK_INIT;
    unsigned long flags;

    for (int i = 0; i < 100; i++) {
        read_lock_irqsave(&rw, &flags);
        read_unlock_irqrestore(&rw, flags);
    }

    return TEST_PASS;
}

/**
 * @brief 测试写锁 irqsave/irqrestore
 */
static int test_write_lock_irqsave_restore(void)
{
    rwlock_t rw = RWLOCK_INIT;
    unsigned long flags;

    for (int i = 0; i < 100; i++) {
        write_lock_irqsave(&rw, &flags);
        write_unlock_irqrestore(&rw, flags);
    }

    return TEST_PASS;
}

/* ============================================================
 * 边界条件测试
 * ============================================================ */

/**
 * @brief 测试重复 unlock（应安全）
 */
static int test_rwlock_double_unlock(void)
{
    rwlock_t rw = RWLOCK_INIT;

    read_lock(&rw);
    read_unlock(&rw);
    read_unlock(&rw);  /* 额外释放 */

    write_lock(&rw);
    write_unlock(&rw);
    write_unlock(&rw);  /* 额外释放 */

    return TEST_PASS;
}

/**
 * @brief 测试交替读写操作
 */
static int test_rwlock_alternating_rw(void)
{
    rwlock_t rw = RWLOCK_INIT;
    const int cycles = 100;

    for (int i = 0; i < cycles; i++) {
        read_lock(&rw);
        read_unlock(&rw);

        write_lock(&rw);
        write_unlock(&rw);
    }

    return TEST_PASS;
}

/**
 * @brief 测试多读者后写者
 */
static int test_rwlock_many_readers_then_writer(void)
{
    rwlock_t rw = RWLOCK_INIT;
    const int reader_count = 20;

    for (int i = 0; i < reader_count; i++) {
        read_lock(&rw);
    }

    for (int i = 0; i < reader_count; i++) {
        read_unlock(&rw);
    }

    write_lock(&rw);
    write_unlock(&rw);

    return TEST_PASS;
}

/* ============================================================
 * 性能基准测试
 * ============================================================ */

/**
 * @brief 测试读锁性能
 */
static int test_read_lock_performance(void)
{
    rwlock_t rw = RWLOCK_INIT;
    const int iterations = 50000;
    uint64_t start, end, elapsed;

    __asm__ volatile("rdtsc" : "=A"(start));

    for (int i = 0; i < iterations; i++) {
        read_lock(&rw);
        read_unlock(&rw);
    }

    __asm__ volatile("rdtsc" : "=A"(end));

    elapsed = end - start;

    klog_kern("[性能] ReadLock: %d 次操作，耗时 %d cycles/次", iterations, (uint32_t)(elapsed / iterations));

    TEST_ASSERT(elapsed > 0);

    return TEST_PASS;
}

/**
 * @brief 测试写锁性能
 */
static int test_write_lock_performance(void)
{
    rwlock_t rw = RWLOCK_INIT;
    const int iterations = 50000;
    uint64_t start, end, elapsed;

    __asm__ volatile("rdtsc" : "=A"(start));

    for (int i = 0; i < iterations; i++) {
        write_lock(&rw);
        write_unlock(&rw);
    }

    __asm__ volatile("rdtsc" : "=A"(end));

    elapsed = end - start;

    klog_kern("[性能] WriteLock: %d 次操作，耗时 %d cycles/次", iterations, (uint32_t)(elapsed / iterations));

    TEST_ASSERT(elapsed > 0);

    return TEST_PASS;
}

/* ============================================================
 * 模块注册
 * ============================================================ */

void test_rwlock_register(void)
{
    int mod = test_register_module("RWLock");
    if (mod < 0) {
        return;
    }

    test_register_case(mod, "基础功能", test_rwlock_basic);
    test_register_case(mod, "读锁基础", test_read_lock_basic);
    test_register_case(mod, "多读者", test_read_lock_multiple);
    test_register_case(mod, "读Trylock", test_read_trylock);
    test_register_case(mod, "写锁基础", test_write_lock_basic);
    test_register_case(mod, "写Trylock", test_write_trylock);
    test_register_case(mod, "写锁互斥", test_write_lock_exclusive);
    test_register_case(mod, "写者阻塞读者", test_writer_blocks_reader);
    test_register_case(mod, "读者阻塞写者", test_reader_blocks_writer);
    test_register_case(mod, "读IRQSave", test_read_lock_irqsave_restore);
    test_register_case(mod, "写IRQSave", test_write_lock_irqsave_restore);
    test_register_case(mod, "重复Unlock", test_rwlock_double_unlock);
    test_register_case(mod, "交替读写", test_rwlock_alternating_rw);
    test_register_case(mod, "多读者后写者", test_rwlock_many_readers_then_writer);
    test_register_case(mod, "读锁性能", test_read_lock_performance);
    test_register_case(mod, "写锁性能", test_write_lock_performance);
}
