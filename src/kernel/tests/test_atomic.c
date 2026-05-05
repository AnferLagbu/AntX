/**
 * @file test_atomic.c
 * @brief 原子操作单元测试
 *
 * 测试原子操作的正确性、并发安全性和性能。
 */

#include "tests/kernel_test.h"
#include "atomic.h"
#include "klog.h"

/* ============================================================
 * 原子读写测试
 * ============================================================ */

/**
 * @brief 测试原子读写的正确性
 */
static int test_atomic_read_write(void)
{
    atomic_t var = ATOMIC_INIT(42);

    TEST_ASSERT_EQ(atomic_read(&var), 42);

    atomic_set(&var, 100);
    TEST_ASSERT_EQ(atomic_read(&var), 100);

    atomic_set(&var, -1);
    TEST_ASSERT_EQ(atomic_read(&var), -1);

    return TEST_PASS;
}

/* ============================================================
 * 原子算术运算测试
 * ============================================================ */

/**
 * @brief 测试原子加一减一
 */
static int test_atomic_inc_dec(void)
{
    atomic_t counter = ATOMIC_INIT(0);

    for (int i = 0; i < 1000; i++) {
        atomic_inc(&counter);
    }

    TEST_ASSERT_EQ(atomic_read(&counter), 1000);

    for (int i = 0; i < 500; i++) {
        atomic_dec(&counter);
    }

    TEST_ASSERT_EQ(atomic_read(&counter), 500);

    return TEST_PASS;
}

/**
 * @brief 测试原子加减法
 */
static int test_atomic_add_sub(void)
{
    atomic_t var = ATOMIC_INIT(0);

    int old = atomic_add(100, &var);
    TEST_ASSERT_EQ(old, 0);
    TEST_ASSERT_EQ(atomic_read(&var), 100);

    old = atomic_add(50, &var);
    TEST_ASSERT_EQ(old, 100);
    TEST_ASSERT_EQ(atomic_read(&var), 150);

    old = atomic_sub(30, &var);
    TEST_ASSERT_EQ(old, 150);
    TEST_ASSERT_EQ(atomic_read(&var), 120);

    return TEST_PASS;
}

/**
 * @brief 测试原子加减返回新值
 */
static int test_atomic_add_sub_return(void)
{
    atomic_t var = ATOMIC_INIT(10);

    int new_val = atomic_add_return(5, &var);
    TEST_ASSERT_EQ(new_val, 15);
    TEST_ASSERT_EQ(atomic_read(&var), 15);

    new_val = atomic_sub_return(3, &var);
    TEST_ASSERT_EQ(new_val, 12);
    TEST_ASSERT_EQ(atomic_read(&var), 12);

    return TEST_PASS;
}

/* ============================================================
 * 原子位操作测试
 * ============================================================ */

/**
 * @brief 测试原子位或/与/异或操作
 */
static int test_atomic_bitwise(void)
{
    atomic_t var = ATOMIC_INIT(0xFF00);

    atomic_or(0x0F0F, &var);
    TEST_ASSERT_EQ(atomic_read(&var), 0xFF0F);

    atomic_and(0xF0F0, &var);
    TEST_ASSERT_EQ(atomic_read(&var), 0xF000);

    atomic_xor(0xFFFF, &var);
    TEST_ASSERT_EQ(atomic_read(&var), 0x0FFF);

    return TEST_PASS;
}

/* ============================================================
 * CAS 操作测试
 * ============================================================ */

/**
 * @brief 测试比较并交换成功场景
 */
static int test_atomic_cmpxchg_success(void)
{
    atomic_t var = ATOMIC_INIT(100);

    int old = atomic_cmpxchg(&var, 100, 200);
    TEST_ASSERT_EQ(old, 100);
    TEST_ASSERT_EQ(atomic_read(&var), 200);

    return TEST_PASS;
}

/**
 * @brief 测试比较并交换失败场景
 */
static int test_atomic_cmpxchg_fail(void)
{
    atomic_t var = ATOMIC_INIT(100);

    int old = atomic_cmpxchg(&var, 99, 200);
    TEST_ASSERT_EQ(old, 100);  /* 返回旧值 */
    TEST_ASSERT_EQ(atomic_read(&var), 100);  /* 值不变 */

    return TEST_PASS;
}

/**
 * @brief 测试 CAS 实现自旋等待模式
 */
static int test_atomic_cmpxchg_loop(void)
{
    atomic_t var = ATOMIC_INIT(0);
    const int target = 10000;

    for (int expected = 0; expected < target; ) {
        int old = atomic_cmpxchg(&var, expected, expected + 1);
        if (old == expected) {
            expected++;
        }
        /* 否则重试，说明有其他线程修改了值（单线程下不会发生）*/
    }

    TEST_ASSERT_EQ(atomic_read(&var), target);

    return TEST_PASS;
}

/* ============================================================
 * 内存屏障测试
 * ============================================================ */

/**
 * @brief 测试内存屏障不崩溃
 */
static int test_memory_barriers(void)
{
    volatile int x = 0;
    volatile int y = 0;

    barrier();
    x = 1;

    smp_wmb();
    y = 2;

    smp_rmb();
    int local_y = y;

    smp_mb();
    x = 3;

    TEST_ASSERT_EQ(x, 3);
    TEST_ASSERT_EQ(local_y, 2);

    smp_acquire();
    smp_release();

    return TEST_PASS;
}

/* ============================================================
 * 原子长整数测试
 * ============================================================ */

/**
 * @brief 测试64位原子操作
 */
static int test_atomic_long_operations(void)
{
    atomic_long_t var = ATOMIC_LONG_INIT(0);

    atomic_long_set(&var, 0xFFFFFFFFFFFFFFFFULL);
    TEST_ASSERT_EQ(atomic_long_read(&var), 0xFFFFFFFFFFFFFFFFULL);

    atomic_long_set(&var, 0);
    TEST_ASSERT_EQ(atomic_long_read(&var), 0);

    return TEST_PASS;
}

/* ============================================================
 * 并发安全性模拟测试
 * ============================================================ */

/**
 * @brief 模拟多线程原子递增
 *
 * 使用 CAS 操作模拟无锁计数器递增。
 */
static int test_atomic_concurrent_increment(void)
{
    atomic_t counter = ATOMIC_INIT(0);
    const int iterations = 10000;

    for (int i = 0; i < iterations; i++) {
        int old_val, new_val;

        do {
            old_val = atomic_read(&counter);
            new_val = old_val + 1;
        } while (atomic_cmpxchg(&counter, old_val, new_val) != old_val);
    }

    TEST_ASSERT_EQ(atomic_read(&counter), iterations);

    return TEST_PASS;
}

/* ============================================================
 * 性能基准测试
 * ============================================================ */

/**
 * @brief 测试原子操作性能
 */
static int test_atomic_performance(void)
{
    atomic_t counter = ATOMIC_INIT(0);
    const int iterations = 100000;
    uint64_t start, end, elapsed;

    __asm__ volatile("rdtsc" : "=A"(start));

    for (int i = 0; i < iterations; i++) {
        atomic_inc(&counter);
    }

    __asm__ volatile("rdtsc" : "=A"(end));

    elapsed = end - start;

    klog_kern("[性能] Atomic Inc: %d 次操作，耗时 %d cycles/次", iterations, (uint32_t)(elapsed / iterations));

    TEST_ASSERT_EQ(atomic_read(&counter), iterations);
    TEST_ASSERT(elapsed > 0);

    return TEST_PASS;
}

/* ============================================================
 * 边界条件测试
 * ============================================================ */

/**
 * @brief 测试大数原子操作
 */
static int test_atomic_large_values(void)
{
    atomic_t var = ATOMIC_INIT(0x7FFFFFFF);

    atomic_inc(&var);
    TEST_ASSERT_EQ(atomic_read(&var), 0x80000000);

    atomic_dec(&var);
    TEST_ASSERT_EQ(atomic_read(&var), 0x7FFFFFFF);

    return TEST_PASS;
}

/**
 * @brief 测试零值边界
 */
static int test_atomic_zero_boundary(void)
{
    atomic_t var = ATOMIC_INIT(1);

    atomic_dec(&var);
    TEST_ASSERT_EQ(atomic_read(&var), 0);

    atomic_dec(&var);
    TEST_ASSERT_EQ(atomic_read(&var), -1);

    atomic_inc(&var);
    TEST_ASSERT_EQ(atomic_read(&var), 0);

    return TEST_PASS;
}

/* ============================================================
 * 模块注册
 * ============================================================ */

void test_atomic_register(void)
{
    int mod = test_register_module("Atomic");
    if (mod < 0) {
        return;
    }

    test_register_case(mod, "读写", test_atomic_read_write);
    test_register_case(mod, "加减一", test_atomic_inc_dec);
    test_register_case(mod, "加减法", test_atomic_add_sub);
    test_register_case(mod, "加减返回值", test_atomic_add_sub_return);
    test_register_case(mod, "位运算", test_atomic_bitwise);
    test_register_case(mod, "CAS成功", test_atomic_cmpxchg_success);
    test_register_case(mod, "CAS失败", test_atomic_cmpxchg_fail);
    test_register_case(mod, "CAS循环", test_atomic_cmpxchg_loop);
    test_register_case(mod, "内存屏障", test_memory_barriers);
    test_register_case(mod, "64位操作", test_atomic_long_operations);
    test_register_case(mod, "并发递增", test_atomic_concurrent_increment);
    test_register_case(mod, "性能基准", test_atomic_performance);
    test_register_case(mod, "大数值", test_atomic_large_values);
    test_register_case(mod, "零值边界", test_atomic_zero_boundary);
}
