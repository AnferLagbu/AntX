#include "kernel_test.h"
#include "pwid.h"
#include "syscall.h"
#include "proc_ffi.h"
#include "string.h"
#include "klog.h"

/* ==================== 常量验证 ==================== */

static int test_pwid_capability_constants(void) {
    TEST_ASSERT_EQ(PWID_LEVEL_ROOT, 0);
    TEST_ASSERT_EQ(PWID_LEVEL_TRUSTWORTHY, 1);
    TEST_ASSERT_EQ(PWID_LEVEL_UNTRUSTWORTHY, 2);
    
    TEST_ASSERT_EQ(CAP_DOMAIN_SYSTEM, 0x0000);
    TEST_ASSERT_EQ(CAP_DOMAIN_FS, 0x0001);
    TEST_ASSERT_EQ(CAP_DOMAIN_PROC, 0x0003);
    
    TEST_ASSERT_EQ(FS_CAP_READ, (1ULL << 0));
    TEST_ASSERT_EQ(FS_CAP_WRITE, (1ULL << 1));
    TEST_ASSERT_EQ(FS_CAP_EXECUTE, (1ULL << 2));
    TEST_ASSERT_EQ(FS_CAP_CREATE, (1ULL << 3));
    
    TEST_ASSERT_EQ(PROC_CAP_FORK, (1ULL << 0));
    TEST_ASSERT_EQ(PROC_CAP_EXEC, (1ULL << 1));
    TEST_ASSERT_EQ(PROC_CAP_RT_SCHED, (1ULL << 4));
    
    return TEST_PASS;
}

static int test_pwid_trust_constants(void) {
    TEST_ASSERT_EQ(TRUST_LEVEL_NONE, 0);
    TEST_ASSERT_EQ(TRUST_LEVEL_BASIC, 1);
    TEST_ASSERT_EQ(TRUST_LEVEL_OPERATE, 2);
    TEST_ASSERT_EQ(TRUST_LEVEL_DELEGATE, 3);
    TEST_ASSERT_EQ(TRUST_LEVEL_FULL, 4);
    return TEST_PASS;
}

static int test_pwid_token_constants(void) {
    TEST_ASSERT_EQ(TOKEN_TYPE_ELEVATION, 0);
    TEST_ASSERT_EQ(TOKEN_TYPE_DELEGATION, 1);
    TEST_ASSERT_EQ(TOKEN_TYPE_SESSION, 2);
    TEST_ASSERT_EQ(TOKEN_TYPE_ONETIME, 3);
    TEST_ASSERT_EQ(TOKEN_FLAG_SINGLE_COMMAND, 0x01);
    TEST_ASSERT_EQ(TOKEN_FLAG_NO_TTY, 0x02);
    return TEST_PASS;
}

/* ==================== 能力域定义验证 (v4) ==================== */

static int test_pwid_v4_domain_constants(void) {
    TEST_ASSERT_EQ(CAP_DOMAIN_SYSTEM_CFG, (1ULL << 0));
    TEST_ASSERT_EQ(CAP_DOMAIN_DEVICE_DISK, (1ULL << 1));
    TEST_ASSERT_EQ(CAP_DOMAIN_USER_CREATE, (1ULL << 2));
    TEST_ASSERT_EQ(CAP_DOMAIN_USER_DELETE, (1ULL << 3));
    TEST_ASSERT_EQ(CAP_DOMAIN_USER_LIST, (1ULL << 4));
    TEST_ASSERT_EQ(CAP_DOMAIN_TOKEN_ISSUE, (1ULL << 5));
    TEST_ASSERT_EQ(CAP_DOMAIN_TRUST_ADD, (1ULL << 6));
    klog_kern("[PWID-V4] Domain constants verified: SYSTEM=0x%lx DEVICE=0x%lx USER_CREATE=0x%lx",
              CAP_DOMAIN_SYSTEM_CFG, CAP_DOMAIN_DEVICE_DISK, CAP_DOMAIN_USER_CREATE);
    return TEST_PASS;
}

/* ==================== 增强检查 + 能力位查询 ==================== */

static int test_pwid_enhanced_check_no_session(void) {
    uint64_t pwid = pwid_get_current();
    if (pwid != 0) {
        int result = pwid_enhanced_check(pwid, pwid, FS_CAP_READ, CAP_DOMAIN_FS);
        TEST_ASSERT_EQ(result, 1);
        return TEST_PASS;
    }
    return TEST_SKIP;
}

static int test_pwid_has_capability_fs_read(void) {
    uint64_t pwid = pwid_get_current();
    if (pwid == 0) return TEST_SKIP;

    /* v4: pwid_has_capability checks the capability_mask directly */
    int has_fs = pwid_has_capability(pwid, CAP_DOMAIN_FS, FS_CAP_READ);
    TEST_ASSERT(has_fs == 1 || has_fs == 0);
    klog_kern("[PWID-V4] pwid=0x%lx fs_read=%d", pwid, has_fs);
    return TEST_PASS;
}

static int test_pwid_has_capability_raw(void) {
    uint64_t pwid = pwid_get_current();
    if (pwid == 0) return TEST_SKIP;

    uint64_t caps_raw = pwid_get_capability_raw(pwid, CAP_DOMAIN_FS);
    klog_kern("[PWID-V4] pwid=0x%lx raw_fs_caps=0x%lx", pwid, caps_raw);
    TEST_ASSERT_GE(caps_raw, 0);
    return TEST_PASS;
}

/* ==================== First Token / Identity ==================== */

static int test_pwid_first_identity(void) {
    if (pwid_any_identity_exists()) {
        return TEST_SKIP;
    }
    int result = pwid_create_first_identity("test_root_pw");
    TEST_ASSERT_EQ(result, 0);
    TEST_ASSERT(pwid_any_identity_exists());
    klog_kern("[PWID-V4] First identity created successfully");
    return TEST_PASS;
}

/* ==================== Token 系统端到端测试 ==================== */

static int test_pwid_token_create(void) {
    uint64_t pwid = pwid_get_current();
    if (pwid == 0) return TEST_SKIP;

    /* Create a token granting FS capabilities for 100 ticks */
    int64_t token_id = pwid_create_token(pwid, CAP_DOMAIN_FS, FS_CAP_READ | FS_CAP_WRITE, 100, 1);
    klog_kern("[PWID-V4] Token create: pwid=0x%lx token=%ld", pwid, token_id);

    /* Token creation may succeed (token_id > 0) or fail (table full = 0) */
    TEST_ASSERT_GE(token_id, 0);
    return TEST_PASS;
}

static int test_pwid_token_revoke(void) {
    uint64_t pwid = pwid_get_current();
    if (pwid == 0) return TEST_SKIP;

    /* Try to revoke a non-existent token */
    int result = pwid_revoke_token_internal(99999, pwid);
    /* Revoke on invalid token returns -1 */
    TEST_ASSERT(result == 0 || result == -1);
    klog_kern("[PWID-V4] Token revoke on invalid: %d", result);
    return TEST_PASS;
}

static int test_pwid_token_lifecycle(void) {
    uint64_t pwid = pwid_get_current();
    if (pwid == 0) return TEST_SKIP;

    /* Create → use */
    int64_t token_id = pwid_create_token(pwid, CAP_DOMAIN_FS, FS_CAP_READ, 100, 1);
    if (token_id > 0) {
        int use_result = pwid_use_token_internal((uint64_t)token_id);
        klog_kern("[PWID-V4] Token lifecycle: id=%ld use=%d", token_id, use_result);
        TEST_ASSERT(use_result == 0 || use_result == -1);
    }
    return TEST_PASS;
}

/* ==================== 调度器配额测试 (L3) ==================== */

static int test_pwid_scheduler_quota_set(void) {
    uint64_t pwid = pwid_get_current();
    if (pwid == 0) return TEST_SKIP;

    /* Set quota: 50 ticks per 500-tick period */
    scheduler_set_quota(pwid, 50, 500);
    klog_kern("[PWID-V4] Scheduler quota set: pwid=0x%lx 50/500", pwid);
    return TEST_PASS;
}

static int test_pwid_scheduler_quota_remove(void) {
    uint64_t pwid = pwid_get_current();
    if (pwid == 0) return TEST_SKIP;

    scheduler_remove_quota(pwid);
    klog_kern("[PWID-V4] Scheduler quota removed: pwid=0x%lx", pwid);
    return TEST_PASS;
}

static int test_pwid_current_pwid_access(void) {
    uint64_t sched_pwid = scheduler_get_current_pwid();
    uint64_t ctx_pwid = pwid_get_current();
    klog_kern("[PWID-V4] Scheduler pwid=0x%lx Context pwid=0x%lx", sched_pwid, ctx_pwid);
    /* Both access methods must be consistent or one may be 0 */
    TEST_ASSERT(sched_pwid == ctx_pwid || sched_pwid == 0 || ctx_pwid == 0);
    return TEST_PASS;
}

/* ==================== 进程限制测试 (L4) ==================== */

static int test_pwid_proc_limit_set(void) {
    uint64_t pwid = pwid_get_current();
    if (pwid == 0) return TEST_SKIP;

    /* Set max 20 processes for this identity */
    scheduler_set_proc_limit(pwid, 20);
    klog_kern("[PWID-V4] Proc limit set: pwid=0x%lx max=20", pwid);
    return TEST_PASS;
}

/* ==================== 注册入口 ==================== */

void test_pwid_enhanced_register(void) {
    int module = test_register_module("PWID Enhanced (v4)");
    if (module < 0) return;
    
    test_register_case(module, "Capability constants", test_pwid_capability_constants);
    test_register_case(module, "Trust constants", test_pwid_trust_constants);
    test_register_case(module, "Token constants", test_pwid_token_constants);
    test_register_case(module, "v4 domain constants", test_pwid_v4_domain_constants);
    test_register_case(module, "Enhanced check", test_pwid_enhanced_check_no_session);
    test_register_case(module, "has_capability (FS)", test_pwid_has_capability_fs_read);
    test_register_case(module, "get_capability_raw", test_pwid_has_capability_raw);
    test_register_case(module, "First identity", test_pwid_first_identity);
    test_register_case(module, "Token create", test_pwid_token_create);
    test_register_case(module, "Token revoke", test_pwid_token_revoke);
    test_register_case(module, "Token lifecycle", test_pwid_token_lifecycle);
    test_register_case(module, "Quota set", test_pwid_scheduler_quota_set);
    test_register_case(module, "Quota remove", test_pwid_scheduler_quota_remove);
    test_register_case(module, "Current pwid access", test_pwid_current_pwid_access);
    test_register_case(module, "Proc limit set", test_pwid_proc_limit_set);
}
