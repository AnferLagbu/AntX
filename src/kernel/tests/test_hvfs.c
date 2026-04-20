#include "kernel_test.h"
#include "hvfs_rust.h"
#include "serial.h"
#include "string.h"

static int test_hvfs_init(void) {
    TEST_ASSERT_EQ(rust_hvfs_check_disk(), -1);
    return TEST_PASS;
}

static int test_hvfs_format(void) {
    int result = rust_hvfs_format();
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_hvfs_create_file(void) {
    int fd = rust_hvfs_open("/test_file.txt", 0x0100 | 0x0002, 0);
    TEST_ASSERT_GE(fd, 0);
    
    rust_hvfs_close(fd);
    
    return TEST_PASS;
}

static int test_hvfs_write_read(void) {
    int fd = rust_hvfs_open("/rw_test.txt", 0x0100 | 0x0002, 0);
    TEST_ASSERT_GE(fd, 0);
    
    const char *data = "HvFS test data";
    int len = strlen(data);
    
    int written = rust_hvfs_write(fd, (const uint8_t*)data, len);
    TEST_ASSERT_EQ(written, len);
    
    rust_hvfs_close(fd);
    
    fd = rust_hvfs_open("/rw_test.txt", 0x0001, 0);
    TEST_ASSERT_GE(fd, 0);
    
    char buffer[64] = {0};
    int read_bytes = rust_hvfs_read(fd, (uint8_t*)buffer, sizeof(buffer));
    TEST_ASSERT_EQ(read_bytes, len);
    TEST_ASSERT_EQ(memcmp(buffer, data, len), 0);
    
    rust_hvfs_close(fd);
    
    return TEST_PASS;
}

static int test_hvfs_mkdir(void) {
    int result = rust_hvfs_mkdir("/test_dir", 0);
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_hvfs_stats(void) {
    uint32_t total_blocks, free_blocks, total_inodes, free_inodes;
    rust_hvfs_get_stats(&total_blocks, &free_blocks, &total_inodes, &free_inodes);
    
    TEST_ASSERT_GT(total_blocks, 0);
    TEST_ASSERT_GT(total_inodes, 0);
    
    return TEST_PASS;
}

static int test_hvfs_sync(void) {
    int result = rust_hvfs_sync();
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_hvfs_current_dir(void) {
    uint32_t dir = rust_hvfs_get_current_dir();
    TEST_ASSERT_GT(dir, 0);
    
    rust_hvfs_set_current_dir(dir);
    TEST_ASSERT_EQ(rust_hvfs_get_current_dir(), dir);
    
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
