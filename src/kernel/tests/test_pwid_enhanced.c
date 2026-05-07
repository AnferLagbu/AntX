#include "kernel_test.h"
#include "pwid.h"
#include "syscall.h"
#include "string.h"

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

static int test_pwid_syscall_numbers(void) {
    TEST_ASSERT_EQ(SYS_AUTH_TOKEN_CREATE, 51);
    TEST_ASSERT_EQ(SYS_AUTH_TOKEN_USE, 52);
    TEST_ASSERT_EQ(SYS_AUTH_TOKEN_REVOKE, 53);
    TEST_ASSERT_EQ(SYS_AUTH_TRUST_ADD, 54);
    TEST_ASSERT_EQ(SYS_AUTH_TRUST_REMOVE, 55);
    TEST_ASSERT_EQ(SYS_AUTH_CHECK, 56);
    
    return TEST_PASS;
}

static int test_pwid_enhanced_check_no_session(void) {
    uint64_t pwid = pwid_get_current();
    if (pwid != 0) {
        int result = pwid_enhanced_check(pwid, pwid, FS_CAP_READ, CAP_DOMAIN_FS);
        TEST_ASSERT_EQ(result, 1);
        return TEST_PASS;
    }
    
    return TEST_SKIP;
}

static int test_pwid_first_identity(void) {
    if (pwid_any_identity_exists()) {
        return TEST_SKIP;
    }
    
    int result = pwid_create_first_identity("test_root_pw");
    TEST_ASSERT_EQ(result, 0);
    TEST_ASSERT(pwid_any_identity_exists());
    
    return TEST_PASS;
}

static int test_pwid_level_assignment(void) {
    if (!pwid_any_identity_exists()) {
        return TEST_SKIP;
    }
    
    struct pwid_entry *entry = pwid_find_by_note("root");
    if (entry == NULL) {
        return TEST_SKIP;
    }
    
    uint8_t level = entry->level;
    TEST_ASSERT_EQ(level, PWID_LEVEL_ROOT);
    
    return TEST_PASS;
}

void test_pwid_enhanced_register(void) {
    int module = test_register_module("PWID Enhanced");
    if (module < 0) return;
    
    test_register_case(module, "Capability constants", test_pwid_capability_constants);
    test_register_case(module, "Trust constants", test_pwid_trust_constants);
    test_register_case(module, "Token constants", test_pwid_token_constants);
    test_register_case(module, "Syscall numbers", test_pwid_syscall_numbers);
    test_register_case(module, "Enhanced check", test_pwid_enhanced_check_no_session);
    test_register_case(module, "First identity creation", test_pwid_first_identity);
    test_register_case(module, "Level assignment", test_pwid_level_assignment);
}
