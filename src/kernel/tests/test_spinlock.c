/**
 * @file test_spinlock.c
 * @brief 自旋锁单元测试
 *
 * 测试自旋锁的基本功能、边界条件和正确性。
 */

#include "tests/kernel_test.h"
#include "spinlock.h"
#include "klog.h"

/* ============================================================
 * 基础功能测试
 * ============================================================ */

/**
 * @brief 测试自旋锁初始化和基本状态查询
 */
static int test_spinlock_basic(void)
{
    spinlock_t lock = {.locked = 0};

    TEST_ASSERT(!spin_is_locked(&lock));

    spin_lock(&lock);

    TEST_ASSERT(spin_is_locked(&lock));

    spin_unlock(&lock);

    TEST_ASSERT(!spin_is_locked(&lock));

    return TEST_PASS;
}

/**
 * @brief 测试 trylock 非阻塞获取
 */
static int test_spinlock_trylock(void)
{
    spinlock_t lock = {.locked = 0};

    spin_lock(&lock);
    TEST_ASSERT(spin_is_locked(&lock));
    TEST_ASSERT(spin_trylock(&lock) == 0);
    spin_unlock(&lock);

    TEST_ASSERT(!spin_is_locked(&lock));
    TEST_ASSERT(spin_trylock(&lock) != 0);
    TEST_ASSERT(spin_is_locked(&lock));
    spin_unlock(&lock);

    return TEST_PASS;
}

/**
 * @brief 测试多次 lock/unlock 循环
 */
static int test_spinlock_multiple_cycles(void)
{
    spinlock_t lock = {.locked = 0};
    const int cycles = 10000;

    for (int i = 0; i < cycles; i++) {
        spin_lock(&lock);
        TEST_ASSERT(spin_is_locked(&lock));
        spin_unlock(&lock);
        TEST_ASSERT(!spin_is_locked(&lock));
    }

    return TEST_PASS;
}

/* ============================================================
 * 中断安全版本测试
 * ============================================================ */

/**
 * @brief 测试 irqsave/irqrestore 配对
 */
static int test_spinlock_irqsave_restore(void)
{
    spinlock_t lock = {.locked = 0};
    unsigned long flags;

    for (int i = 0; i < 1000; i++) {
        spin_lock_irqsave(&lock, &flags);
        TEST_ASSERT(spin_is_locked(&lock));
        spin_unlock_irqrestore(&lock, flags);
        TEST_ASSERT(!spin_is_locked(&lock));
    }

    return TEST_PASS;
}

/**
 * @brief 测试 irq 版本
 */
static int test_spinlock_irq(void)
{
    spinlock_t lock = {.locked = 0};

    for (int i = 0; i < 1000; i++) {
        spin_lock_irq(&lock);
        TEST_ASSERT(spin_is_locked(&lock));
        spin_unlock_irq(&lock);
        TEST_ASSERT(!spin_is_locked(&lock));
    }

    return TEST_PASS;
}

/* ============================================================
 * 边界条件测试
 * ============================================================ */

/**
 * @brief 测试嵌套 trylock 失败
 */
static int test_spinlock_nested_trylock_fail(void)
{
    spinlock_t lock = {.locked = 0};

    spin_lock(&lock);
    TEST_ASSERT(spin_trylock(&lock) == 0);
    TEST_ASSERT(spin_trylock(&lock) == 0);
    TEST_ASSERT(spin_trylock(&lock) == 0);
    spin_unlock(&lock);

    return TEST_PASS;
}

/**
 * @brief 测试重复 unlock（应安全）
 */
static int test_spinlock_double_unlock(void)
{
    spinlock_t lock = {.locked = 0};

    spin_lock(&lock);
    spin_unlock(&lock);
    spin_unlock(&lock);

    TEST_ASSERT(!spin_is_locked(&lock));

    return TEST_PASS;
}

/* ============================================================
 * 多锁测试
 * ============================================================ */

/**
 * @brief 测试多个独立锁可以同时持有
 */
static int test_spinlock_multiple_locks(void)
{
    spinlock_t lock1 = {.locked = 0};
    spinlock_t lock2 = {.locked = 0};
    spinlock_t lock3 = {.locked = 0};

    spin_lock(&lock1);
    spin_lock(&lock2);
    spin_lock(&lock3);

    TEST_ASSERT(spin_is_locked(&lock1));
    TEST_ASSERT(spin_is_locked(&lock2));
    TEST_ASSERT(spin_is_locked(&lock3));

    spin_unlock(&lock1);
    spin_unlock(&lock2);
    spin_unlock(&lock3);

    TEST_ASSERT(!spin_is_locked(&lock1));
    TEST_ASSERT(!spin_is_locked(&lock2));
    TEST_ASSERT(!spin_is_locked(&lock3));

    return TEST_PASS;
}

/**
 * @brief 测试锁的顺序获取和释放
 */
static int test_spinlock_ordering(void)
{
    spinlock_t locks[10];

    for (int i = 0; i < 10; i++) {
        spin_init(&locks[i]);
    }

    for (int i = 0; i < 10; i++) {
        spin_lock(&locks[i]);
    }

    for (int i = 9; i >= 0; i--) {
        TEST_ASSERT(spin_is_locked(&locks[i]));
        spin_unlock(&locks[i]);
    }

    for (int i = 0; i < 10; i++) {
        TEST_ASSERT(!spin_is_locked(&locks[i]));
    }

    return TEST_PASS;
}

/* ============================================================
 * 性能基准测试
 * ============================================================ */

/**
 * @brief 测试自旋锁性能
 */
static int test_spinlock_performance(void)
{
    spinlock_t lock = {.locked = 0};
    const int iterations = 100000;
    uint64_t start, end, elapsed;

    __asm__ volatile("rdtsc" : "=A"(start));

    for (int i = 0; i < iterations; i++) {
        spin_lock(&lock);
        spin_unlock(&lock);
    }

    __asm__ volatile("rdtsc" : "=A"(end));

    elapsed = end - start;

    klog_kern("[性能] Spinlock: %d 次 lock/unlock，耗时 %d cycles/次", iterations, (uint32_t);

    TEST_ASSERT(elapsed > 0);
    TEST_ASSERT(elapsed < iterations * 1000UL);  /* 合理上限 */

    return TEST_PASS;
}

/* ============================================================
 * 模块注册
 * ============================================================ */

void test_spinlock_register(void)
{
    int mod = test_register_module("Spinlock");
    if (mod < 0) {
        return;
    }

    test_register_case(mod, "基本功能", test_spinlock_basic);
    test_register_case(mod, "Trylock", test_spinlock_trylock);
    test_register_case(mod, "多次循环", test_spinlock_multiple_cycles);
    test_register_case(mod, "IRQSave/Restore", test_spinlock_irqsave_restore);
    test_register_case(mod, "IRQ版本", test_spinlock_irq);
    test_register_case(mod, "嵌套Trylock失败", test_spinlock_nested_trylock_fail);
    test_register_case(mod, "重复Unlock", test_spinlock_double_unlock);
    test_register_case(mod, "多锁独立", test_spinlock_multiple_locks);
    test_register_case(mod, "顺序获取释放", test_spinlock_ordering);
    test_register_case(mod, "性能基准", test_spinlock_performance);
}
