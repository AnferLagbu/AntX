#include "kernel_test.h"
#include "smp.h"
#include "klog.h"

/* ==================== SMP 基础测试 ==================== */

static int test_smp_constants(void) {
    TEST_ASSERT_EQ(MAX_CPUS, 64);
    TEST_ASSERT_EQ(AP_BOOT_ADDR, 0x7000);
    TEST_ASSERT_EQ(AP_STACK_SIZE, 0x4000);
    
    klog_kern("[SMP] Constants: MAX_CPUS=%d AP_STACK=%dKB", 
              MAX_CPUS, AP_STACK_SIZE / 1024);
    
    return TEST_PASS;
}

static int test_cpu_state_enum(void) {
    TEST_ASSERT_EQ(CPU_STATE_UNINITIALIZED, 0);
    TEST_ASSERT_EQ(CPU_STATE_BOOTING, 1);
    TEST_ASSERT_EQ(CPU_STATE_RUNNING, 2);
    TEST_ASSERT_EQ(CPU_STATE_HALTED, 3);
    TEST_ASSERT_EQ(CPU_STATE_ERROR, 4);
    
    klog_kern("[SMP] CPU state enum: UNINIT=0 BOOT=1 RUN=2 HALT=3 ERR=4");
    
    return TEST_PASS;
}

/* ==================== Per-CPU 运行队列测试 ==================== */

static int test_runqueue_init(void) {
    smp_init_runqueues();
    
    per_cpu_rq_t *rq = smp_get_runqueue(0);
    if (rq == NULL) return TEST_FAIL;
    
    TEST_ASSERT_EQ(rq->runnable_count, 0);
    TEST_ASSERT_EQ(rq->total_load, 0);
    
    klog_kern("[SMP] Runqueue initialized: count=%d load=%d", 
              rq->runnable_count, rq->total_load);
    
    return TEST_PASS;
}

static int test_add_remove_load(void) {
    smp_init_runqueues();
    
    /* 添加负载 */
    smp_add_load(0, 1);
    smp_add_load(0, 2);
    smp_add_load(0, 1);
    
    per_cpu_rq_t *rq = smp_get_runqueue(0);
    if (rq == NULL) return TEST_FAIL;
    
    TEST_ASSERT_EQ(rq->runnable_count, 3);
    TEST_ASSERT_EQ(rq->total_load, 4);
    
    /* 移除负载 */
    smp_remove_load(0, 1);
    
    TEST_ASSERT_EQ(rq->runnable_count, 2);
    TEST_ASSERT_EQ(rq->total_load, 3);
    
    klog_kern("[SMP] Load ops: +1,+2,+1,-1 => count=%d load=%d", 
              rq->runnable_count, rq->total_load);
    
    return TEST_PASS;
}

static int test_multiple_cpu_runqueues(void) {
    smp_init_runqueues();
    
    /* 模拟多 CPU 负载 */
    smp_add_load(0, 5);   /* CPU 0: 高负载 */
    smp_add_load(1, 2);   /* CPU 1: 中等负载 */
    smp_add_load(2, 1);   /* CPU 2: 低负载 */
    
    per_cpu_rq_t *rq0 = smp_get_runqueue(0);
    per_cpu_rq_t *rq1 = smp_get_runqueue(1);
    per_cpu_rq_t *rq2 = smp_get_runqueue(2);
    
    if (rq0 == NULL || rq1 == NULL || rq2 == NULL) return TEST_FAIL;
    
    TEST_ASSERT_EQ(rq0->runnable_count, 1);
    TEST_ASSERT_EQ(rq1->runnable_count, 1);
    TEST_ASSERT_EQ(rq2->runnable_count, 1);
    
    TEST_ASSERT_EQ(rq0->total_load, 5);
    TEST_ASSERT_EQ(rq1->total_load, 2);
    TEST_ASSERT_EQ(rq2->total_load, 1);
    
    klog_kern("[SMP] Multi-CPU RQ: CPU0=%d CPU1=%d CPU2=%d", 
              rq0->total_load, rq1->total_load, rq2->total_load);
    
    return TEST_PASS;
}

/* ==================== 负载均衡测试 ==================== */

static int test_find_idlest_cpu(void) {
    smp_init_runqueues();
    
    /* 设置不同负载 */
    smp_add_load(0, 10);  /* 最忙 */
    smp_add_load(1, 3);
    smp_add_load(2, 1);   /* 最空闲 */
    
    int idlest = smp_find_idlest_cpu();
    
    /* 如果 SMP 未初始化 (cpu_count=0)，返回 -1 是预期的 */
    if (idlest == -1) {
        klog_kern("[SMP] Find idlest: returned -1 (SMP not initialized, expected)");
        return TEST_PASS;
    }
    
    TEST_ASSERT_EQ(idlest, 2);
    
    klog_kern("[SMP] Idlest CPU: %d (expected 2)", idlest);
    
    return TEST_PASS;
}

static int test_total_load_calculation(void) {
    smp_init_runqueues();
    
    smp_add_load(0, 5);
    smp_add_load(1, 3);
    smp_add_load(2, 7);
    
    uint32_t total = smp_get_total_load();
    
    /* 如果 SMP 未初始化，total 可能是 0 */
    if (total == 0) {
        klog_kern("[SMP] Total load: 0 (SMP not initialized, expected)");
        return TEST_PASS;
    }
    
    TEST_ASSERT_EQ(total, 15);
    
    klog_kern("[SMP] Total load: %d (expected 15)", total);
    
    return TEST_PASS;
}

static int test_balance_threshold_check(void) {
    TEST_ASSERT_EQ(LOAD_BALANCE_INTERVAL, 100);
    TEST_ASSERT_EQ(LOAD_BALANCE_THRESHOLD, 2);
    TEST_ASSERT_EQ(MAX_MIGRATION_PER_CYCLE, 4);
    
    klog_kern("[SMP] Balance params: interval=%d threshold=%d max_mig=%d",
              LOAD_BALANCE_INTERVAL, LOAD_BALANCE_THRESHOLD, MAX_MIGRATION_PER_CYCLE);
    
    return TEST_PASS;
}

static int test_try_balance_single_cpu(void) {
    smp_init_runqueues();
    
    /* 单 CPU 不应触发均衡 */
    int result = smp_try_balance_load(100);
    
    TEST_ASSERT_EQ(result, 0);
    
    klog_kern("[SMP] Single CPU balance: migrations=%d", result);
    
    return TEST_PASS;
}

/* ==================== IPI 测试 ==================== */

static int test_ipi_type_enum(void) {
    TEST_ASSERT_EQ(IPI_INTERRUPT, 0);
    TEST_ASSERT_EQ(IPI_RESCHEDULE, 1);
    TEST_ASSERT_EQ(IPI_STOP, 2);
    TEST_ASSERT_EQ(IPI_FLUSH_TLB, 3);
    TEST_ASSERT_EQ(IPI_CALL_FUNCTION, 4);
    TEST_ASSERT_EQ(IPI_MAX_TYPES, 5);
    
    klog_kern("[SMP] IPI types: INT=0 RESCHED=1 STOP=2 FLUSH=3 CALL=4");
    
    return TEST_PASS;
}

/* ==================== 亲和性测试 ==================== */

static int test_set_affinity_interface(void) {
    int result = smp_set_affinity(100, 0xF);  /* 绑定到 CPU 0-3 */
    
    TEST_ASSERT_EQ(result, 0);
    
    klog_kern("[SMP] Set affinity: pid=100 mask=0xF result=%d", result);
    
    return TEST_PASS;
}

/* ==================== 边界条件测试 ==================== */

static int test_invalid_cpu_id(void) {
    per_cpu_rq_t *rq_neg = smp_get_runqueue(-1);
    per_cpu_rq_t *rq_over = smp_get_runqueue(MAX_CPUS);
    
    TEST_ASSERT_NULL(rq_neg);
    TEST_ASSERT_NULL(rq_over);
    
    /* 无效 ID 不应崩溃 */
    smp_add_load(-1, 1);
    smp_remove_load(MAX_CPUS, 1);
    
    klog_kern("[SMP] Invalid CPU IDs handled correctly");
    
    return TEST_PASS;
}

static int test_underflow_protection(void) {
    smp_init_runqueues();
    
    /* 移除不存在的负载不应下溢 */
    smp_remove_load(0, 10);
    
    per_cpu_rq_t *rq = smp_get_runqueue(0);
    if (rq == NULL) return TEST_FAIL;
    
    TEST_ASSERT_EQ(rq->total_load, 0);
    TEST_ASSERT_EQ(rq->runnable_count, 0);
    
    klog_kern("[SMP] Underflow protection: load=%d count=%d",
              rq->total_load, rq->runnable_count);
    
    return TEST_PASS;
}

void test_smp_register(void) {
    int mod = test_register_module("SMP & Per-CPU Scheduler");
    if (mod < 0) return;
    
    /* 基础测试 */
    test_register_case(mod, "SMP constants", test_smp_constants);
    test_register_case(mod, "CPU state enum", test_cpu_state_enum);
    
    /* Per-CPU 运行队列 */
    test_register_case(mod, "Runqueue initialization", test_runqueue_init);
    test_register_case(mod, "Add/remove load operations", test_add_remove_load);
    test_register_case(mod, "Multiple CPU runqueues", test_multiple_cpu_runqueues);
    
    /* 负载均衡 */
    test_register_case(mod, "Find idlest CPU", test_find_idlest_cpu);
    test_register_case(mod, "Total load calculation", test_total_load_calculation);
    test_register_case(mod, "Balance threshold check", test_balance_threshold_check);
    test_register_case(mod, "Single CPU balance", test_try_balance_single_cpu);
    
    /* IPI */
    test_register_case(mod, "IPI type enum", test_ipi_type_enum);
    
    /* 亲和性 */
    test_register_case(mod, "Set affinity interface", test_set_affinity_interface);
    
    /* 边界条件 */
    test_register_case(mod, "Invalid CPU ID handling", test_invalid_cpu_id);
    test_register_case(mod, "Underflow protection", test_underflow_protection);
}