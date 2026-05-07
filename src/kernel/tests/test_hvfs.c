#include "kernel_test.h"
#include "pwid.h"
#include "hvfs_ffi.h"
#include "string.h"

static void ensure_test_session(void) {
    if (pwid_get_current() != 0) return;
    if (!pwid_any_identity_exists()) {
        pwid_create_first_identity("test_session_pw");
    }
    pwid_login("root", "test_session_pw");
}

static int test_hvfs_init(void) {
    TEST_ASSERT_EQ(hvfs_check_disk_internal(), -1);
    return TEST_PASS;
}

static int test_hvfs_format(void) {
    int result = hvfs_format_internal();
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_hvfs_create_file(void) {
    ensure_test_session();
    int fd = hvfs_open_internal("/test_file.txt", 0x0100 | 0x0002, pwid_get_current());
    TEST_ASSERT_GE(fd, 0);
    
    hvfs_close_internal(fd);
    
    return TEST_PASS;
}

static int test_hvfs_write_read(void) {
    ensure_test_session();
    int fd = hvfs_open_internal("/rw_test.txt", 0x0100 | 0x0002, pwid_get_current());
    TEST_ASSERT_GE(fd, 0);
    
    const char *data = "HvFS test data";
    int len = strlen(data);
    
    int written = hvfs_write_internal(fd, (const uint8_t*)data, len);
    TEST_ASSERT_EQ(written, len);
    
    hvfs_close_internal(fd);
    
    fd = hvfs_open_internal("/rw_test.txt", 0x0001, pwid_get_current());
    TEST_ASSERT_GE(fd, 0);
    
    char buffer[64] = {0};
    int read_bytes = hvfs_read_internal(fd, (uint8_t*)buffer, sizeof(buffer));
    TEST_ASSERT_EQ(read_bytes, len);
    TEST_ASSERT_EQ(memcmp(buffer, data, len), 0);
    
    hvfs_close_internal(fd);
    
    return TEST_PASS;
}

static int test_hvfs_mkdir(void) {
    ensure_test_session();
    int result = hvfs_mkdir_internal("/test_dir", pwid_get_current());
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_hvfs_stats(void) {
    uint32_t total_blocks, free_blocks, total_inodes, free_inodes;
    hvfs_get_stats_internal(&total_blocks, &free_blocks, &total_inodes, &free_inodes);
    
    TEST_ASSERT_GT(total_blocks, 0);
    TEST_ASSERT_GT(total_inodes, 0);
    
    return TEST_PASS;
}

static int test_hvfs_sync(void) {
    int result = hvfs_sync_internal();
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_hvfs_current_dir(void) {
    uint32_t dir = hvfs_get_current_dir_internal();
    TEST_ASSERT_GT(dir, 0);
    
    hvfs_set_current_dir_internal(dir);
    TEST_ASSERT_EQ(hvfs_get_current_dir_internal(), dir);
    
    return TEST_PASS;
}

void test_hvfs_register(void) {
    int mod = test_register_module("HvFS (Hybrid Virtual File System)");
    
    test_register_case(mod, "HvFS initialization", test_hvfs_init);
    test_register_case(mod, "Format filesystem", test_hvfs_format);
    test_register_case(mod, "Create file", test_hvfs_create_file);
    test_register_case(mod, "Write and read", test_hvfs_write_read);
    test_register_case(mod, "Create directory", test_hvfs_mkdir);
    test_register_case(mod, "Filesystem stats", test_hvfs_stats);
    test_register_case(mod, "Sync filesystem", test_hvfs_sync);
    test_register_case(mod, "Current directory", test_hvfs_current_dir);
}
