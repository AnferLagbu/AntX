/**
 * @file test_mutex.c
 * @brief 睡眠锁 (Mutex) 单元测试
 *
 * 测试 Mutex 的基本功能、超时机制和条件变量。
 */

#include "tests/kernel_test.h"
#include "mutex.h"
#include "thread.h"
#include "klog.h"

/* ============================================================
 * 基础功能测试
 * ============================================================ */

/**
 * @brief 测试 Mutex 初始化和基本状态查询
 */
static int test_mutex_basic(void)
{
    mutex_t m = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};

    TEST_ASSERT(!mutex_is_locked(&m));
    TEST_ASSERT_EQ(mutex_owner(&m), -1);

    mutex_lock(&m);

    TEST_ASSERT(mutex_is_locked(&m));
    TEST_ASSERT_EQ(mutex_owner(&m), process_get_current_pid());

    mutex_unlock(&m);

    TEST_ASSERT(!mutex_is_locked(&m));
    TEST_ASSERT_EQ(mutex_owner(&m), -1);

    return TEST_PASS;
}

/**
 * @brief 测试 trylock 非阻塞获取
 */
static int test_mutex_trylock(void)
{
    mutex_t m = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};

    mutex_lock(&m);
    TEST_ASSERT(mutex_is_locked(&m));
    TEST_ASSERT(mutex_trylock(&m) == 0);
    mutex_unlock(&m);

    TEST_ASSERT(!mutex_is_locked(&m));
    TEST_ASSERT(mutex_trylock(&m) != 0);
    TEST_ASSERT(mutex_is_locked(&m));
    mutex_unlock(&m);

    return TEST_PASS;
}

/**
 * @brief 测试多次 lock/unlock 循环
 */
static int test_mutex_multiple_cycles(void)
{
    mutex_t m = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};
    const int cycles = 1000;

    for (int i = 0; i < cycles; i++) {
        mutex_lock(&m);
        TEST_ASSERT(mutex_is_locked(&m));
        mutex_unlock(&m);
        TEST_ASSERT(!mutex_is_locked(&m));
    }

    return TEST_PASS;
}

/* ============================================================
 * 超时机制测试
 * ============================================================ */

/**
 * @brief 测试带超时的 lock (立即成功)
 */
static int test_mutex_lock_timeout_immediate(void)
{
    mutex_t m = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};

    int result = mutex_lock_timeout(&m, 0);  /* 无限等待，但应该立即成功 */
    TEST_ASSERT(result != 0);
    TEST_ASSERT(mutex_is_locked(&m));

    mutex_unlock(&m);

    return TEST_PASS;
}

/**
 * @brief 测试 trylock 作为零超时的替代
 */
static int test_mutex_trylock_as_timeout(void)
{
    mutex_t m = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};

    mutex_lock(&m);

    /* trylock 应该失败（已锁定）*/
    int result = mutex_trylock(&m);
    TEST_ASSERT(result == 0);

    mutex_unlock(&m);

    /* trylock 应该成功（未锁定）*/
    result = mutex_trylock(&m);
    TEST_ASSERT(result != 0);
    mutex_unlock(&m);

    return TEST_PASS;
}

/* ============================================================
 * 边界条件测试
 * ============================================================ */

/**
 * @brief 测试重复 unlock（应安全）
 */
static int test_mutex_double_unlock(void)
{
    mutex_t m = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};

    mutex_lock(&m);
    mutex_unlock(&m);
    mutex_unlock(&m);  /* 额外释放 */

    TEST_ASSERT(!mutex_is_locked(&m));

    return TEST_PASS;
}

/**
 * @brief 测试多个独立 Mutex 可以同时持有
 */
static int test_mutex_multiple_mutexes(void)
{
    mutex_t m1 = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};
    mutex_t m2 = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};
    mutex_t m3 = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};

    mutex_lock(&m1);
    mutex_lock(&m2);
    mutex_lock(&m3);

    TEST_ASSERT(mutex_is_locked(&m1));
    TEST_ASSERT(mutex_is_locked(&m2));
    TEST_ASSERT(mutex_is_locked(&m3));

    mutex_unlock(&m1);
    mutex_unlock(&m2);
    mutex_unlock(&m3);

    TEST_ASSERT(!mutex_is_locked(&m1));
    TEST_ASSERT(!mutex_is_locked(&m2));
    TEST_ASSERT(!mutex_is_locked(&m3));

    return TEST_PASS;
}

/* ============================================================
 * 条件变量测试
 * ============================================================ */

/**
 * @brief 测试条件变量初始化和基本操作
 */
static int test_cond_var_basic(void)
{
    cond_var_t cv;
    mutex_t m = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};

    cond_init(&cv);

    mutex_lock(&m);
    /* 在单线程环境下无法真正测试 wait/signal */
    mutex_unlock(&m);

    return TEST_PASS;
}

/**
 * @brief 测试多次 signal/broadcast（不应崩溃）
 */
static int test_cond_var_signal_broadcast(void)
{
    cond_var_t cv;
    mutex_t m = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};

    cond_init(&cv);

    for (int i = 0; i < 10; i++) {
        cond_signal(&cv);
        cond_broadcast(&cv);
    }

    mutex_lock(&m);
    mutex_unlock(&m);

    return TEST_PASS;
}

/* ============================================================
 * 性能基准测试
 * ============================================================ */

/**
 * @brief 测试 Mutex 性能
 */
static int test_mutex_performance(void)
{
    mutex_t m = {.locked = 0, .owner = -1, .acquire_time = 0, .depth = 0};
    const int iterations = 50000;
    uint64_t start, end, elapsed;

    __asm__ volatile("rdtsc" : "=A"(start));

    for (int i = 0; i < iterations; i++) {
        mutex_lock(&m);
        mutex_unlock(&m);
    }

    __asm__ volatile("rdtsc" : "=A"(end));

    elapsed = end - start;

    klog_kern("[性能] Mutex: %d 次 lock/unlock，耗时 %d cycles/次", iterations, (uint32_t);

    TEST_ASSERT(elapsed > 0);
    TEST_ASSERT(elapsed < iterations * 10000UL);  /* 合理上限 */

    return TEST_PASS;
}

/* ============================================================
 * 模块注册
 * ============================================================ */

void test_mutex_register(void)
{
    int mod = test_register_module("Mutex");
    if (mod < 0) {
        return;
    }

    test_register_case(mod, "基本功能", test_mutex_basic);
    test_register_case(mod, "Trylock", test_mutex_trylock);
    test_register_case(mod, "多次循环", test_mutex_multiple_cycles);
    test_register_case(mod, "超时-立即成功", test_mutex_lock_timeout_immediate);
    test_register_case(mod, "Trylock作为超时", test_mutex_trylock_as_timeout);
    test_register_case(mod, "重复Unlock", test_mutex_double_unlock);
    test_register_case(mod, "多Mutex独立", test_mutex_multiple_mutexes);
    test_register_case(mod, "条件变量基础", test_cond_var_basic);
    test_register_case(mod, "Signal/Broadcast", test_cond_var_signal_broadcast);
    test_register_case(mod, "性能基准", test_mutex_performance);
}
