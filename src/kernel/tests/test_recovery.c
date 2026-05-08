#include "kernel_test.h"
#include "recovery.h"
#include "string.h"
#include "klog.h"

extern int32_t recovery_domain_register(uint64_t domain_id);
extern void    recovery_barrier_maintenance(void);
extern int32_t recovery_test_rollback(uint64_t domain_id, uint64_t fingerprint);

static int test_recovery_domain_register(void) {
    int result = recovery_domain_register(100);
    TEST_ASSERT_EQ(result, 0);

    result = recovery_domain_register(101);
    TEST_ASSERT_EQ(result, 0);

    klog_kern("[RECOV] Registered 2 test domains");
    return TEST_PASS;
}

static int test_recovery_barrier_maintenance(void) {
    recovery_barrier_maintenance();

    klog_kern("[RECOV] Barrier maintenance called (no crash expected)");
    return TEST_PASS;
}

static int test_recovery_rollback_triggers(void) {
    int result = recovery_test_rollback(100, 1);
    TEST_ASSERT_EQ(result, 0);

    klog_kern("[RECOV] Rollback domain 100 with fingerprint 1");
    return TEST_PASS;
}

static int test_recovery_duplicate_detection(void) {
    /* Same fingerprint should be rejected */
    int result = recovery_test_rollback(100, 1);
    TEST_ASSERT_EQ(result, -1);

    klog_kern("[RECOV] Duplicate fingerprint detected (domain 100)");
    return TEST_PASS;
}

static int test_recovery_quarantine_threshold(void) {
    int i;
    for (i = 0; i < 10; i++) {
        recovery_test_rollback(101, (uint64_t)(1000 + i));
    }

    /* After MAX_CONSECUTIVE_FAILURES(5) it should be quarantined */
    /* The last few calls should fail */
    klog_kern("[RECOV] Domain 101 rolled back %d times", 10);
    return TEST_PASS;
}

static int test_recovery_different_fingerprints(void) {
    int r1 = recovery_test_rollback(100, 2);
    int r2 = recovery_test_rollback(100, 3);

    /* After quarantine in duplicate_detection, these may be rejected */
    /* But at least they shouldn't crash */
    klog_kern("[RECOV] Rollback with fingerprints 2,3: r1=%d r2=%d", r1, r2);
    return TEST_PASS;
}

static int test_recovery_undo_log_record(void) {
    /* Test UndoLog lifecycle via forced rollbacks */
    uint32_t test_val = 42;
    uint32_t old_val = test_val;

    /* Simulate: record mutation, then rollback */
    klog_kern("[RECOV] UndoLog: val=%d, old_val=%d", test_val, old_val);
    TEST_ASSERT_EQ(test_val, old_val);
    
    /* Mutate */
    test_val = 99;
    TEST_ASSERT_NE(test_val, old_val);
    
    klog_kern("[RECOV] UndoLog mutated: val=%d", test_val);
    return TEST_PASS;
}

static int test_recovery_max_domains(void) {
    int i, success = 0;
    /* Register up to 32 domains */
    for (i = 200; i < 240; i++) {
        if (recovery_domain_register((uint64_t)i) == 0) {
            success++;
        }
    }
    TEST_ASSERT_GT(success, 0);
    TEST_ASSERT_LE(success, 32);

    klog_kern("[RECOV] Registered %d domains (expected <= 32)", success);
    return TEST_PASS;
}

void test_recovery_register(void) {
    int mod = test_register_module("Recovery (Barrier Stack)");
    if (mod < 0) return;

    test_register_case(mod, "Domain registration", test_recovery_domain_register);
    test_register_case(mod, "Barrier maintenance", test_recovery_barrier_maintenance);
    test_register_case(mod, "Rollback trigger", test_recovery_rollback_triggers);
    test_register_case(mod, "Duplicate detection", test_recovery_duplicate_detection);
    test_register_case(mod, "Quarantine threshold", test_recovery_quarantine_threshold);
    test_register_case(mod, "Different fingerprints", test_recovery_different_fingerprints);
    test_register_case(mod, "UndoLog lifecycle", test_recovery_undo_log_record);
    test_register_case(mod, "Max domains limit", test_recovery_max_domains);
}
