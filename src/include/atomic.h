/**
 * @file atomic.h
 * @brief 原子操作接口定义
 *
 * 提供 x86_64 架构下的原子操作原语，包括原子读写、算术运算、
 * 位操作和 CAS (Compare-And-Swap) 操作，作为并发编程的基础设施。
 */

#ifndef ATOMIC_H
#define ATOMIC_H

#include "types.h"

/* ============================================================
 * 原子类型定义
 * ============================================================ */

/**
 * @brief 32位原子整数类型
 */
typedef struct {
    volatile int counter;
} atomic_t;

/**
 * @brief 64位原子长整数类型
 */
typedef struct {
    volatile long counter;
} atomic_long_t;

/**
 * @brief 原子初始化宏
 */
#define ATOMIC_INIT(i)  { (i) }
#define ATOMIC_LONG_INIT(l) { (l) }

/* ============================================================
 * 原子读写操作
 * ============================================================ */

/**
 * @brief 原子读取整数值
 *
 * @param v 原子变量指针
 * @return 当前值
 */
static inline int atomic_read(atomic_t *v)
{
    return v->counter;
}

/**
 * @brief 原子设置整数值
 *
 * @param v 原子变量指针
 * @param i 要设置的值
 */
static inline void atomic_set(atomic_t *v, int i)
{
    v->counter = i;
}

/**
 * @brief 原子读取长整数值
 *
 * @param v 原子变量指针
 * @return 当前值
 */
static inline long atomic_long_read(atomic_long_t *v)
{
    return v->counter;
}

/**
 * @brief 原子设置长整数值
 *
 * @param v 原子变量指针
 * @param l 要设置的值
 */
static inline void atomic_long_set(atomic_long_t *v, long l)
{
    v->counter = l;
}

/* ============================================================
 * 原子算术运算
 * ============================================================ */

/**
 * @brief 原子加一操作
 *
 * @param v 原子变量指针
 * @return 操作后的新值
 */
static inline int atomic_inc(atomic_t *v)
{
    int result;
    __asm__ volatile(
        "lock xaddl %1, %0"
        : "+m"(v->counter), "=r"(result)
        : "1"(1)
        : "memory", "cc"
    );
    return result + 1;
}

/**
 * @brief 原子减一操作
 *
 * @param v 原子变量指针
 * @return 操作后的新值
 */
static inline int atomic_dec(atomic_t *v)
{
    int result;
    __asm__ volatile(
        "lock xaddl %1, %0"
        : "+m"(v->counter), "=r"(result)
        : "1"(-1)
        : "memory", "cc"
    );
    return result - 1;
}

/**
 * @brief 原子加法操作
 *
 * @param i 要加的值
 * @param v 原子变量指针
 * @return 操作前的旧值
 */
static inline int atomic_add(int i, atomic_t *v)
{
    int result;
    __asm__ volatile(
        "lock xaddl %1, %0"
        : "+m"(v->counter), "=r"(result)
        : "1"(i)
        : "memory", "cc"
    );
    return result;
}

/**
 * @brief 原子减法操作
 *
 * @param i 要减的值
 * @param v 原子变量指针
 * @return 操作前的旧值
 */
static inline int atomic_sub(int i, atomic_t *v)
{
    return atomic_add(-i, v);
}

/**
 * @brief 原子加法并返回新值
 *
 * @param i 要加的值
 * @param v 原子变量指针
 * @return 操作后的新值
 */
static inline int atomic_add_return(int i, atomic_t *v)
{
    return atomic_add(i, v) + i;
}

/**
 * @brief 原子减法并返回新值
 *
 * @param i 要减的值
 * @param v 原子变量指针
 * @return 操作后的新值
 */
static inline int atomic_sub_return(int i, atomic_t *v)
{
    return atomic_sub(i, v) - i;
}

/* ============================================================
 * 原子位操作
 * ============================================================ */

/**
 * @brief 原子或操作
 *
 * @param mask 位掩码
 * @param v 原子变量指针
 */
static inline void atomic_or(int mask, atomic_t *v)
{
    __asm__ volatile(
        "lock orl %1, %0"
        : "+m"(v->counter)
        : "r"((unsigned int)mask)
        : "memory", "cc"
    );
}

/**
 * @brief 原子与操作
 *
 * @param mask 位掩码
 * @param v 原子变量指针
 */
static inline void atomic_and(int mask, atomic_t *v)
{
    __asm__ volatile(
        "lock andl %1, %0"
        : "+m"(v->counter)
        : "r"((unsigned int)mask)
        : "memory", "cc"
    );
}

/**
 * @brief 原子异或操作
 *
 * @param mask 位掩码
 * @param v 原子变量指针
 */
static inline void atomic_xor(int mask, atomic_t *v)
{
    __asm__ volatile(
        "lock xorl %1, %0"
        : "+m"(v->counter)
        : "r"((unsigned int)mask)
        : "memory", "cc"
    );
}

/* ============================================================
 * CAS (Compare-And-Swap) 操作
 * ============================================================ */

/**
 * @brief 原子比较并交换操作 (32位)
 *
 * 如果当前值等于 old_val，则将其设置为 new_val。
 *
 * @param v 原子变量指针
 * @param old_val 期望的旧值
 * @param new_val 要设置的新值
 * @return 操作前的旧值（如果返回值等于 old_val，则交换成功）
 */
static inline int atomic_cmpxchg(atomic_t *v, int old_val, int new_val)
{
    int result;
    __asm__ volatile(
        "lock cmpxchgl %2, %0"
        : "+m"(v->counter), "=a"(result)
        : "r"(new_val), "1"((int)old_val)
        : "memory", "cc"
    );
    return result;
}

/**
 * @brief 原子尝试加法操作
 *
 * 如果加法不会导致溢出或下溢，则执行加法。
 *
 * @param v 原子变量指针
 * @param delta 要加的值
 * @return 0 成功，非零失败
 */
static inline int atomic_try_add(atomic_t *v, int delta)
{
    int old_val, new_val;

    do {
        old_val = atomic_read(v);
        new_val = old_val + delta;

        if (delta > 0 && new_val < old_val) {
            return -1;  /* 溢出 */
        }
        if (delta < 0 && new_val > old_val) {
            return -1;  /* 下溢 */
        }
    } while (atomic_cmpxchg(v, old_val, new_val) != old_val);

    return 0;
}

/* ============================================================
 * 内存屏障 (Memory Barriers)
 * ============================================================ */

/**
 * @brief 编译器屏障
 *
 * 防止编译器对内存访问进行重排序，
 * 但不生成 CPU 内存屏障指令。
 */
static inline void barrier(void)
{
    __asm__ volatile("" ::: "memory");
}

/**
 * @brief 完整内存屏障 (Full Memory Barrier)
 *
 * 防止 CPU 和编译器对所有内存操作进行重排序。
 * 等效于 mfence 指令。
 */
static inline void smp_mb(void)
{
    barrier();
    __asm__ volatile("mfence" ::: "memory");
}

/**
 * @brief 读内存屏障 (Load Barrier)
 *
 * 确保所有读操作在屏障前完成。
 * 等效于 lfence 指令。
 */
static inline void smp_rmb(void)
{
    barrier();
    __asm__ volatile("lfence" ::: "memory");
}

/**
 * @brief 写内存屏障 (Store Barrier)
 *
 * 确保所有写操作在屏障前完成。
 * 等效于 sfence 指令。
 */
static inline void smp_wmb(void)
{
    barrier();
    __asm__ volatile("sfence" ::: "memory");
}

/**
 * @brief 获取内存屏障 (Acquire Barrier)
 *
 * 确保后续的读/写操作不会被重排到屏障之前。
 */
static inline void smp_acquire(void)
{
    barrier();
    __asm__ volatile("lfence" ::: "memory");
}

/**
 * @brief 释放内存屏障 (Release Barrier)
 *
 * 确保之前的读/写操作不会被重排到屏障之后。
 */
static inline void smp_release(void)
{
    barrier();
    __asm__ volatile("sfence" ::: "memory");
}

#endif /* ATOMIC_H */
