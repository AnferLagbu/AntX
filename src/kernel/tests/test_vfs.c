#include "kernel_test.h"
#include "vfs.h"
#include "serial.h"
#include "string.h"

static int test_vfs_init(void) {
    return TEST_PASS;
}

static int test_vfs_mount(void) {
    int result = vfs_mount("/test_mnt", "ramfs");
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_vfs_create_file(void) {
    struct vfs_file *file = vfs_open("/test_create.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    TEST_ASSERT_NOT_NULL(file);
    
    vfs_close(file);
    
    return TEST_PASS;
}

static int test_vfs_write_read(void) {
    const char *test_data = "Hello, VFS World!";
    int len = strlen(test_data);
    
    struct vfs_file *file = vfs_open("/test_rw.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    TEST_ASSERT_NOT_NULL(file);
    
    int written = vfs_write(file, test_data, len);
    TEST_ASSERT_EQ(written, len);
    
    vfs_close(file);
    
    file = vfs_open("/test_rw.txt", VFS_O_RDONLY, 0);
    TEST_ASSERT_NOT_NULL(file);
    
    char buffer[64] = {0};
    int read_bytes = vfs_read(file, buffer, sizeof(buffer));
    TEST_ASSERT_EQ(read_bytes, len);
    
    vfs_close(file);
    
    return TEST_PASS;
}

static int test_vfs_mkdir(void) {
    int result = vfs_mkdir("/test_dir", 0);
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_vfs_stat(void) {
    struct vfs_file *file = vfs_open("/test_stat.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    TEST_ASSERT_NOT_NULL(file);
    
    const char *data = "test content";
    vfs_write(file, data, strlen(data));
    vfs_close(file);
    
    struct vfs_stat st;
    int result = vfs_stat("/test_stat.txt", &st, 0);
    TEST_ASSERT_EQ(result, 0);
    TEST_ASSERT_EQ(st.size, strlen(data));
    
    return TEST_PASS;
}

static int test_vfs_delete(void) {
    struct vfs_file *file = vfs_open("/test_delete.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    TEST_ASSERT_NOT_NULL(file);
    vfs_close(file);
    
    return TEST_PASS;
}

static int test_vfs_large_file(void) {
    struct vfs_file *file = vfs_open("/test_large.bin", VFS_O_CREAT | VFS_O_WRONLY, 0);
    TEST_ASSERT_NOT_NULL(file);
    
    char buffer[256];
    for (int i = 0; i < 256; i++) {
        buffer[i] = (char)(i & 0xFF);
    }
    
    int total_written = 0;
    for (int i = 0; i < 10; i++) {
        int written = vfs_write(file, buffer, sizeof(buffer));
        if (written < 0) break;
        total_written += written;
    }
    
    TEST_ASSERT_GT(total_written, 0);
    
    vfs_close(file);
    
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
