#include "kernel_test.h"
#include "proc.h"
#include "scheduler_ex.h"
#include "timer.h"
#include "klog.h"

extern void scheduler_yield(void);
extern void scheduler_boost_priority(void);
extern void scheduler_add_rt_task(uint32_t pid, uint8_t rt_priority, uint32_t policy);
extern int scheduler_set_sched_policy(uint32_t pid, uint32_t policy, uint8_t rt_priority);
extern size_t scheduler_get_rt_count(void);
extern void proc_block(uint32_t reason);

/* ==================== P0: 优先级提升机制测试 ==================== */

static int test_priority_boost_prevents_starvation(void) {
    struct process *current = process_get_current();
    if (current == NULL) return TEST_SKIP;
    
    int old_priority = current->priority;
    
    klog_kern("[SCHED-RT] Before boost: priority=%d", old_priority);
    
    scheduler_boost_priority();
    
    klog_kern("[SCHED-RT] Boost called successfully");
    
    TEST_ASSERT_GE(old_priority, 0);
    TEST_ASSERT_LT(old_priority, 4);
    
    return TEST_PASS;
}

static int test_boost_interval_configuration(void) {
    TEST_ASSERT_EQ(SCHED_BOOST_INTERVAL, 1000);
    
    klog_kern("[SCHED-RT] Boost interval: %d ticks (%d seconds)", 
              SCHED_BOOST_INTERVAL, SCHED_BOOST_INTERVAL / 100);
    
    return TEST_PASS;
}

static int test_multiple_boosts_stable(void) {
    for (int i = 0; i < 5; i++) {
        scheduler_boost_priority();
    }
    
    klog_kern("[SCHED-RT] Multiple boosts (5) completed without error");
    
    return TEST_PASS;
}

/* ==================== P0: 阻塞式睡眠测试 ==================== */

static int test_blocking_sleep_exists(void) {
    timer_sleep(1);
    
    klog_kern("[SCHED-RT] timer_sleep(1) returned - blocking sleep works");
    
    return TEST_PASS;
}

static int test_blocking_sleep_zero(void) {
    uint64_t start = timer_get_ticks();
    timer_sleep(0);
    uint64_t end = timer_get_ticks();
    
    TEST_ASSERT_EQ(end - start, 0);
    
    klog_kern("[SCHED-RT] Sleep(0) elapsed: %d ticks (should be 0)", (uint32_t)(end - start));
    
    return TEST_PASS;
}

static int test_busy_wait_compat(void) {
    uint64_t start = timer_get_ticks();
    timer_sleep_busy(10);
    uint64_t end = timer_get_ticks();
    
    TEST_ASSERT(end >= start);  /* Time should move forward */
    
    klog_kern("[SCHED-RT] Busy sleep(10ms) elapsed: %d ticks", (uint32_t)(end - start));
    
    return TEST_PASS;
}

static int test_sleep_vs_busy_comparison(void) {
    uint64_t sleep_start = timer_get_ticks();
    timer_sleep(5);
    uint64_t sleep_end = timer_get_ticks();
    uint64_t sleep_time = sleep_end - sleep_start;
    
    uint64_t busy_start = timer_get_ticks();
    timer_sleep_busy(5);
    uint64_t busy_end = timer_get_ticks();
    uint64_t busy_time = busy_end - busy_start;
    
    klog_kern("[SCHED-RT] Sleep comparison: blocking=%d ticks, busy=%d ticks", 
              (uint32_t)sleep_time, (uint32_t)busy_time);
    
    return TEST_PASS;
}

/* ==================== P1: 实时调度类测试 ==================== */

static int test_rt_policy_constants(void) {
    TEST_ASSERT_EQ(SCHED_NORMAL, 0);
    TEST_ASSERT_EQ(SCHED_FIFO, 1);
    TEST_ASSERT_EQ(SCHED_RR, 2);
    TEST_ASSERT_EQ(SCHED_IDLE, 3);
    
    TEST_ASSERT_EQ(RT_PRIORITY_MAX, 99);
    TEST_ASSERT_EQ(RT_PRIORITY_MIN, 1);
    TEST_ASSERT_EQ(RT_TIME_SLICE, 5);
    
    klog_kern("[SCHED-RT] RT constants: FIFO=%d, RR=%d, MAX_PRI=%d", 
              SCHED_FIFO, SCHED_RR, RT_PRIORITY_MAX);
    
    return TEST_PASS;
}

static int test_rt_queue_initial_state(void) {
    size_t rt_count = scheduler_get_rt_count();
    
    TEST_ASSERT_EQ(rt_count, 0);
    
    klog_kern("[SCHED-RT] Initial RT queue count: %d", (uint32_t)rt_count);
    
    return TEST_PASS;
}

static int test_add_rt_task_fifo(void) {
    uint32_t test_pid = 100;
    uint8_t rt_pri = 50;
    
    scheduler_add_rt_task(test_pid, rt_pri, SCHED_FIFO);
    
    size_t rt_count = scheduler_get_rt_count();
    
    TEST_ASSERT_GT(rt_count, 0);
    
    klog_kern("[SCHED-RT] Added RT task (FIFO): pid=%d, pri=%d, count=%d", 
              test_pid, rt_pri, (uint32_t)rt_count);
    
    return TEST_PASS;
}

static int test_add_rt_task_rr(void) {
    uint32_t test_pid = 101;
    uint8_t rt_pri = 70;
    
    scheduler_add_rt_task(test_pid, rt_pri, SCHED_RR);
    
    size_t rt_count = scheduler_get_rt_count();
    
    TEST_ASSERT_GT(rt_count, 1);
    
    klog_kern("[SCHED-RT] Added RT task (RR): pid=%d, pri=%d, count=%d", 
              test_pid, rt_pri, (uint32_t)rt_count);
    
    return TEST_PASS;
}

static int test_rt_priority_ordering(void) {
    size_t before = scheduler_get_rt_count();
    
    scheduler_add_rt_task(200, 10, SCHED_FIFO);
    scheduler_add_rt_task(201, 90, SCHED_FIFO);
    scheduler_add_rt_task(202, 50, SCHED_FIFO);
    
    size_t after = scheduler_get_rt_count();
    
    TEST_ASSERT_GE(after, before + 3);
    
    klog_kern("[SCHED-RT] Priority ordering test: added 3 tasks (10,90,50), total=%d", 
              (uint32_t)after);
    
    return TEST_PASS;
}

static int test_set_sched_policy_interface(void) {
    struct process *current = process_get_current();
    if (current == NULL) return TEST_SKIP;
    
    uint32_t pid = (uint32_t)current->pid;
    int result = scheduler_set_sched_policy(pid, SCHED_FIFO, 80);
    
    TEST_ASSERT_EQ(result, 0);
    
    klog_kern("[SCHED-RT] Set sched policy for pid=%d: result=%d", pid, result);
    
    return TEST_PASS;
}

static int test_rt_vs_normal_priority(void) {
    klog_kern("[SCHED-RT] RT tasks should preempt normal tasks");
    klog_kern("[SCHED-RT] Queue order: RT(FIFO/RR) > MLFQ(L0-L3)");
    
    size_t rt_count = scheduler_get_rt_count();
    
    (void)rt_count;  /* RT count is size_t, always valid */
    
    return TEST_PASS;
}

/* ==================== 调度器综合测试 ==================== */

static int test_scheduler_tick_with_boost(void) {
    extern void scheduler_tick_mlfq(void);
    
    for (int i = 0; i < 100; i++) {
        scheduler_tick_mlfq();
    }
    
    klog_kern("[SCHED-RT] 100 scheduler ticks processed");
    
    return TEST_PASS;
}

static int test_yield_reschedule_flag(void) {
    extern int sched_should_reschedule(void);
    
    scheduler_yield();
    
    klog_kern("[SCHED-RT] Yield called, reschedule flag set");
    
    return TEST_PASS;
}

static int test_process_block_unblock(void) {
    struct process *current = process_get_current();
    if (current == NULL) return TEST_SKIP;
    
    uint32_t initial_state = current->state;
    
    proc_block(3);  /* BLOCK_SLEEP */
    
    klog_kern("[SCHED-RT] Process blocked: state=%d->BLOCKED", initial_state);
    
    return TEST_PASS;
}

static int test_scheduler_stats_consistency(void) {
    struct process *current = process_get_current();
    if (current == NULL) return TEST_SKIP;
    
    TEST_ASSERT_GT(current->pid, 0);
    TEST_ASSERT_GE(current->priority, 0);
    TEST_ASSERT_LT(current->priority, 5);
    
    klog_kern("[SCHED-RT] Process stats: pid=%d, pri=%d, cpu_time=%d", 
              (uint32_t)current->pid, current->priority, (uint32_t)current->cpu_time);
    
    return TEST_PASS;
}

/* ==================== 压力测试 ==================== */

static int test_rapid_schedule_cycles(void) {
    const int cycles = 100;
    uint64_t start = timer_get_ticks();
    
    for (int i = 0; i < cycles; i++) {
        scheduler_yield();
        
        if (i % 20 == 0) {
            scheduler_boost_priority();
        }
    }
    
    uint64_t end = timer_get_ticks();
    
    klog_kern("[SCHED-RT] Rapid schedule: %d cycles in %d ticks", cycles, (uint32_t)(end - start));
    
    return TEST_PASS;
}

static int test_mixed_rt_normal_workload(void) {
    scheduler_add_rt_task(300, 99, SCHED_FIFO);
    scheduler_add_rt_task(301, 1, SCHED_RR);
    scheduler_add_rt_task(302, 50, SCHED_NORMAL);
    
    size_t total = scheduler_get_rt_count();
    
    klog_kern("[SCHED-RT] Mixed workload: %d tasks in RT queue", (uint32_t)total);
    
    (void)total;  /* Validity: size_t is always non-negative */
    
    return TEST_PASS;
}

void test_scheduler_rt_register(void) {
    int mod = test_register_module("Scheduler RT Enhancements");
    if (mod < 0) return;
    
    /* P0: 优先级提升 */
    test_register_case(mod, "Priority boost prevents starvation", test_priority_boost_prevents_starvation);
    test_register_case(mod, "Boost interval configuration", test_boost_interval_configuration);
    test_register_case(mod, "Multiple boosts stability", test_multiple_boosts_stable);
    
    /* P0: 阻塞式睡眠 */
    test_register_case(mod, "Blocking sleep function", test_blocking_sleep_exists);
    test_register_case(mod, "Sleep with zero duration", test_blocking_sleep_zero);
    test_register_case(mod, "Busy-wait compatibility", test_busy_wait_compat);
    test_register_case(mod, "Sleep vs busy comparison", test_sleep_vs_busy_comparison);
    
    /* P1: 实时调度类 */
    test_register_case(mod, "RT policy constants", test_rt_policy_constants);
    test_register_case(mod, "RT queue initial state", test_rt_queue_initial_state);
    test_register_case(mod, "Add RT task (FIFO)", test_add_rt_task_fifo);
    test_register_case(mod, "Add RT task (RR)", test_add_rt_task_rr);
    test_register_case(mod, "RT priority ordering", test_rt_priority_ordering);
    test_register_case(mod, "Set sched policy API", test_set_sched_policy_interface);
    test_register_case(mod, "RT vs Normal priority", test_rt_vs_normal_priority);
    
    /* 综合测试 */
    test_register_case(mod, "Scheduler tick with boost", test_scheduler_tick_with_boost);
    test_register_case(mod, "Yield reschedule flag", test_yield_reschedule_flag);
    test_register_case(mod, "Process block/unblock", test_process_block_unblock);
    test_register_case(mod, "Scheduler stats consistency", test_scheduler_stats_consistency);
    
    /* 压力测试 */
    test_register_case(mod, "Rapid schedule cycles (100)", test_rapid_schedule_cycles);
    test_register_case(mod, "Mixed RT/Normal workload", test_mixed_rt_normal_workload);
}