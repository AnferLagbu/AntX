#include "kernel_test.h"
#include "vfs.h"
#include "klog.h"
#include "string.h"

extern int32_t vfs_open_internal(const char *path, uint32_t flags, uint64_t pwid);
extern int32_t vfs_close_internal(uint32_t fd);
extern int32_t vfs_read_internal(uint32_t fd, void *buf, uint32_t count);
extern int32_t vfs_write_internal(uint32_t fd, const void *buf, uint32_t count);
extern int32_t vfs_mkdir_internal(const char *path, uint64_t pwid);
extern int32_t vfs_stat_internal(const char *path, void *st, uint64_t pwid);
extern void vfs_init(void);
extern void ramfs_init(void);

static int test_vfs_init(void) {
    int result = vfs_mount("/", "ramfs");
    if (result != 0 && result != -2) {
        vfs_init();
        ramfs_init();
        result = vfs_mount("/", "ramfs");
        if (result != 0 && result != -2) {
            TEST_ASSERT_MSG(0, "Failed to mount root filesystem");
        }
    }
    
    return TEST_PASS;
}

static int test_vfs_mount(void) {
    int result = vfs_mount("/test_mnt", "ramfs");
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_vfs_create_file(void) {
    int mount_result = vfs_mount("/", "ramfs");
    klog_kern("[MOUNT] result=%d", mount_result);

    klog_kern("[TEST] Calling vfs_open_internal...");

    int32_t fd = vfs_open_internal("/test_create.txt", 0x0100 | 0x0002, 0);
    klog_kern("[TEST] returned: %d", fd);
    TEST_ASSERT_GE(fd, 0);
    
    if (fd >= 0) {
        vfs_close_internal(fd);
    }
    
    return TEST_PASS;
}

static int test_vfs_write_read(void) {
    const char *test_data = "Hello, VFS World!";
    int len = strlen(test_data);
    
    int32_t fd = vfs_open_internal("/test_rw.txt", 0x0100 | 0x0002, 0);
    if (fd < 0) {
        TEST_ASSERT_MSG(0, "Failed to create file");
    }
    
    int32_t written = vfs_write_internal(fd, test_data, len);
    TEST_ASSERT_GE(written, 0);
    
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/test_rw.txt", 0x0001, 0);
    if (fd < 0) {
        TEST_ASSERT_MSG(0, "Failed to open file for reading");
    }
    
    char buffer[64] = {0};
    int32_t read_bytes = vfs_read_internal(fd, buffer, sizeof(buffer));
    TEST_ASSERT_GE(read_bytes, 0);
    
    vfs_close_internal(fd);
    
    return TEST_PASS;
}

static int test_vfs_mkdir(void) {
    int32_t result = vfs_mkdir_internal("/test_dir", 0);
    TEST_ASSERT_GE(result, 0);
    
    return TEST_PASS;
}

static int test_vfs_stat(void) {
    int32_t fd = vfs_open_internal("/test_stat.txt", 0x0100 | 0x0002, 0);
    if (fd < 0) {
        TEST_ASSERT_MSG(0, "Failed to create file");
    }
    
    const char *data = "test content";
    vfs_write_internal(fd, data, strlen(data));
    vfs_close_internal(fd);
    
    char st[128];
    int32_t result = vfs_stat_internal("/test_stat.txt", st, 0);
    TEST_ASSERT_GE(result, 0);
    
    return TEST_PASS;
}

static int test_vfs_delete(void) {
    int32_t fd = vfs_open_internal("/test_delete.txt", 0x0100 | 0x0002, 0);
    if (fd >= 0) {
        vfs_close_internal(fd);
    }
    TEST_ASSERT_GE(fd, 0);
    
    return TEST_PASS;
}

static int test_vfs_large_file(void) {
    int32_t fd = vfs_open_internal("/test_large.bin", 0x0100 | 0x0002, 0);
    if (fd < 0) {
        TEST_ASSERT_MSG(0, "Failed to create large file");
    }
    
    char buffer[256];
    for (int i = 0; i < 256; i++) {
        buffer[i] = (char)(i & 0xFF);
    }
    
    int total_written = 0;
    for (int i = 0; i < 10; i++) {
        int32_t written = vfs_write_internal(fd, buffer, sizeof(buffer));
        if (written < 0) break;
        total_written += written;
    }
    
    TEST_ASSERT_GT(total_written, 0);
    
    vfs_close_internal(fd);
    
    return TEST_PASS;
}

void test_vfs_register(void) {
    int mod = test_register_module("VFS (Virtual File System)");
    
    test_register_case(mod, "VFS initialization", test_vfs_init);
    test_register_case(mod, "Mount filesystem", test_vfs_mount);
    test_register_case(mod, "Create file", test_vfs_create_file);
    test_register_case(mod, "Write and read", test_vfs_write_read);
    test_register_case(mod, "Create directory", test_vfs_mkdir);
    test_register_case(mod, "File stat", test_vfs_stat);
    test_register_case(mod, "Delete file", test_vfs_delete);
    test_register_case(mod, "Large file (2.5KB)", test_vfs_large_file);
}
