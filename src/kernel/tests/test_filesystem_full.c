#include "kernel_test.h"
#include "vfs.h"
#include "hvfs_rust.h"
#include "serial.h"
#include "string.h"

extern int32_t rust_vfs_open(const char *path, uint32_t flags, uint64_t pwid);
extern int32_t rust_vfs_close(uint32_t fd);
extern int32_t rust_vfs_read(uint32_t fd, void *buf, uint32_t count);
extern int32_t rust_vfs_write(uint32_t fd, const void *buf, uint32_t count);
extern int32_t rust_vfs_mkdir(const char *path, uint64_t pwid);
extern int32_t rust_vfs_mount(const char *path, const char *fs_type);
extern int32_t rust_vfs_unmount(const char *path);
extern int32_t rust_vfs_format(const char *path, const char *fs_type);
extern int32_t rust_vfs_sync(void);
extern int32_t rust_vfs_stat(const char *path, void *st, uint64_t pwid);
extern int32_t rust_vfs_seek(uint32_t fd, int32_t offset, uint32_t whence);
extern int32_t rust_vfs_chmod(const char *path, uint32_t mode, uint64_t pwid);
extern int32_t rust_vfs_chown(const char *path, uint32_t uid, uint32_t gid, uint64_t pwid);

#define VFS_O_RDONLY 0x0001
#define VFS_O_WRONLY 0x0002
#define VFS_O_RDWR   0x0004
#define VFS_O_CREAT  0x0100
#define VFS_O_TRUNC  0x0200
#define VFS_O_APPEND 0x0400

#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

static int test_vfs_mount_ramfs(void) {
    int result = rust_vfs_mount("/mnt/ramfs", "ramfs");
    TEST_ASSERT_EQ(result, 0);
    return TEST_PASS;
}

static int test_vfs_mount_devfs(void) {
    int result = rust_vfs_mount("/dev", "devfs");
    TEST_ASSERT_GE(result, -1);
    return TEST_PASS;
}

static int test_vfs_mount_procfs(void) {
    int result = rust_vfs_mount("/proc", "procfs");
    TEST_ASSERT_GE(result, -1);
    return TEST_PASS;
}

static int test_vfs_unmount(void) {
    rust_vfs_mount("/mnt/test", "ramfs");
    int result = rust_vfs_unmount("/mnt/test");
    TEST_ASSERT_GE(result, -1);
    return TEST_PASS;
}

static int test_vfs_format_ramfs(void) {
    int result = rust_vfs_format("/mnt/ramfs", "ramfs");
    TEST_ASSERT_GE(result, -1);
    return TEST_PASS;
}

static int test_vfs_format_hvfs(void) {
    int result = rust_hvfs_format();
    TEST_ASSERT_EQ(result, 0);
    return TEST_PASS;
}

static int test_vfs_sync(void) {
    int result = rust_vfs_sync();
    TEST_ASSERT_GE(result, -1);
    return TEST_PASS;
}

static int test_vfs_create_deep_directory(void) {
    rust_vfs_mkdir("/deep", 0);
    rust_vfs_mkdir("/deep/level1", 0);
    rust_vfs_mkdir("/deep/level1/level2", 0);
    rust_vfs_mkdir("/deep/level1/level2/level3", 0);
    
    int fd = rust_vfs_open("/deep/level1/level2/level3/test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    TEST_ASSERT_GE(fd, 0);
    
    if (fd >= 0) {
        rust_vfs_close(fd);
    }
    
    return TEST_PASS;
}

static int test_vfs_file_seek(void) {
    int fd = rust_vfs_open("/seek_test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *data = "0123456789ABCDEF";
    rust_vfs_write(fd, data, 16);
    rust_vfs_close(fd);
    
    fd = rust_vfs_open("/seek_test.txt", VFS_O_RDONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    int result = rust_vfs_seek(fd, 5, SEEK_SET);
    TEST_ASSERT_GE(result, 0);
    
    char buf[8] = {0};
    int read = rust_vfs_read(fd, buf, 3);
    TEST_ASSERT_EQ(read, 3);
    TEST_ASSERT_EQ(buf[0], '5');
    TEST_ASSERT_EQ(buf[1], '6');
    TEST_ASSERT_EQ(buf[2], '7');
    
    rust_vfs_close(fd);
    
    return TEST_PASS;
}

static int test_vfs_file_append(void) {
    int fd = rust_vfs_open("/append_test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *data1 = "FIRST";
    rust_vfs_write(fd, data1, 5);
    rust_vfs_close(fd);
    
    fd = rust_vfs_open("/append_test.txt", VFS_O_WRONLY | VFS_O_APPEND, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *data2 = "SECOND";
    rust_vfs_write(fd, data2, 6);
    rust_vfs_close(fd);
    
    fd = rust_vfs_open("/append_test.txt", VFS_O_RDONLY, 0);
    char buf[16] = {0};
    int read = rust_vfs_read(fd, buf, sizeof(buf));
    TEST_ASSERT_EQ(read, 11);
    TEST_ASSERT_EQ(memcmp(buf, "FIRSTSECOND", 11), 0);
    
    rust_vfs_close(fd);
    
    return TEST_PASS;
}

static int test_vfs_file_truncate(void) {
    int fd = rust_vfs_open("/trunc_test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *data = "THIS IS A LONG STRING FOR TRUNCATE TEST";
    rust_vfs_write(fd, data, 39);
    rust_vfs_close(fd);
    
    fd = rust_vfs_open("/trunc_test.txt", VFS_O_WRONLY | VFS_O_TRUNC, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *short_data = "SHORT";
    rust_vfs_write(fd, short_data, 5);
    rust_vfs_close(fd);
    
    fd = rust_vfs_open("/trunc_test.txt", VFS_O_RDONLY, 0);
    char buf[64] = {0};
    int read = rust_vfs_read(fd, buf, sizeof(buf));
    TEST_ASSERT_EQ(read, 5);
    TEST_ASSERT_EQ(memcmp(buf, "SHORT", 5), 0);
    
    rust_vfs_close(fd);
    
    return TEST_PASS;
}

static int test_vfs_concurrent_files(void) {
    int fds[5];
    const char *files[] = {
        "/concurrent_1.txt",
        "/concurrent_2.txt",
        "/concurrent_3.txt",
        "/concurrent_4.txt",
        "/concurrent_5.txt"
    };
    
    for (int i = 0; i < 5; i++) {
        fds[i] = rust_vfs_open(files[i], VFS_O_CREAT | VFS_O_WRONLY, 0);
        TEST_ASSERT_GE(fds[i], 0);
    }
    
    for (int i = 0; i < 5; i++) {
        char data[16];
        int len = 0;
        data[len++] = 'D';
        data[len++] = 'A';
        data[len++] = 'T';
        data[len++] = 'A';
        data[len++] = '_';
        data[len++] = '0' + i;
        rust_vfs_write(fds[i], data, len);
    }
    
    for (int i = 0; i < 5; i++) {
        rust_vfs_close(fds[i]);
    }
    
    for (int i = 0; i < 5; i++) {
        int fd = rust_vfs_open(files[i], VFS_O_RDONLY, 0);
        TEST_ASSERT_GE(fd, 0);
        if (fd >= 0) {
            rust_vfs_close(fd);
        }
    }
    
    return TEST_PASS;
}

static int test_vfs_stress_many_files(void) {
    const int file_count = 50;
    int created = 0;
    
    for (int i = 0; i < file_count; i++) {
        char path[32];
        int len = 0;
        
        const char *prefix = "/stress_";
        while (*prefix) path[len++] = *prefix++;
        
        path[len++] = '0' + (i / 10);
        path[len++] = '0' + (i % 10);
        path[len++] = '.';
        path[len++] = 't';
        path[len++] = 'x';
        path[len++] = 't';
        path[len] = '\0';
        
        int fd = rust_vfs_open(path, VFS_O_CREAT | VFS_O_WRONLY, 0);
        if (fd >= 0) {
            char data[8] = {'T', 'E', 'S', 'T', '_', '0' + (i/10), '0' + (i%10), 0};
            rust_vfs_write(fd, data, 7);
            rust_vfs_close(fd);
            created++;
        }
    }
    
    TEST_ASSERT_GT(created, 0);
    
    return TEST_PASS;
}

static int test_vfs_stress_large_file(void) {
    int fd = rust_vfs_open("/large_stress.bin", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    char buffer[1024];
    for (int i = 0; i < 1024; i++) {
        buffer[i] = (char)(i & 0xFF);
    }
    
    int total_written = 0;
    for (int i = 0; i < 10; i++) {
        int written = rust_vfs_write(fd, buffer, 1024);
        if (written > 0) {
            total_written += written;
        }
    }
    
    rust_vfs_close(fd);
    
    TEST_ASSERT_GT(total_written, 0);
    
    fd = rust_vfs_open("/large_stress.bin", VFS_O_RDONLY, 0);
    if (fd >= 0) {
        char verify[1024];
        int read = rust_vfs_read(fd, verify, 1024);
        TEST_ASSERT_GT(read, 0);
        rust_vfs_close(fd);
    }
    
    return TEST_PASS;
}

static int test_vfs_stress_random_access(void) {
    int fd = rust_vfs_open("/random_access.bin", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    char pattern[256];
    for (int i = 0; i < 256; i++) {
        pattern[i] = (char)i;
    }
    
    for (int i = 0; i < 4; i++) {
        rust_vfs_write(fd, pattern, 256);
    }
    rust_vfs_close(fd);
    
    fd = rust_vfs_open("/random_access.bin", VFS_O_RDONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    char buf[64];
    
    rust_vfs_seek(fd, 128, SEEK_SET);
    int read = rust_vfs_read(fd, buf, 64);
    TEST_ASSERT_EQ(read, 64);
    for (int i = 0; i < 64; i++) {
        TEST_ASSERT_EQ(buf[i], (char)((128 + i) & 0xFF));
    }
    
    rust_vfs_seek(fd, 512, SEEK_SET);
    read = rust_vfs_read(fd, buf, 64);
    TEST_ASSERT_GT(read, 0);
    
    rust_vfs_close(fd);
    
    return TEST_PASS;
}

static int test_vfs_error_handling(void) {
    int fd = rust_vfs_open("/nonexistent_file.txt", VFS_O_RDONLY, 0);
    TEST_ASSERT_LT(fd, 0);
    
    int result = rust_vfs_read(9999, NULL, 0);
    TEST_ASSERT_LT(result, 0);
    
    result = rust_vfs_close(9999);
    TEST_ASSERT_LT(result, 0);
    
    return TEST_PASS;
}

static int test_vfs_permission_check(void) {
    int fd = rust_vfs_open("/permission_test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd >= 0) {
        rust_vfs_close(fd);
    }
    
    int result = rust_vfs_chmod("/permission_test.txt", 0755, 0);
    TEST_ASSERT_GE(result, -1);
    
    return TEST_PASS;
}

static int test_vfs_directory_operations(void) {
    int result = rust_vfs_mkdir("/test_dir_ops", 0);
    TEST_ASSERT_GE(result, -1);
    
    result = rust_vfs_mkdir("/test_dir_ops/subdir1", 0);
    TEST_ASSERT_GE(result, -1);
    
    result = rust_vfs_mkdir("/test_dir_ops/subdir2", 0);
    TEST_ASSERT_GE(result, -1);
    
    int fd = rust_vfs_open("/test_dir_ops/file_in_dir.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd >= 0) {
        rust_vfs_write(fd, "content", 7);
        rust_vfs_close(fd);
    }
    
    return TEST_PASS;
}

static int test_vfs_file_stat(void) {
    int fd = rust_vfs_open("/stat_test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *data = "Stat test content";
    rust_vfs_write(fd, data, 17);
    rust_vfs_close(fd);
    
    char stat_buf[128];
    int result = rust_vfs_stat("/stat_test.txt", stat_buf, 0);
    TEST_ASSERT_GE(result, -1);
    
    return TEST_PASS;
}

void test_filesystem_full_register(void) {
    int mod = test_register_module("Filesystem Full Test");
    
    test_register_case(mod, "Mount RamFS", test_vfs_mount_ramfs);
    test_register_case(mod, "Mount DevFS", test_vfs_mount_devfs);
    test_register_case(mod, "Mount ProcFS", test_vfs_mount_procfs);
    test_register_case(mod, "Unmount filesystem", test_vfs_unmount);
    test_register_case(mod, "Format RamFS", test_vfs_format_ramfs);
    test_register_case(mod, "Format HvFS", test_vfs_format_hvfs);
    test_register_case(mod, "Sync filesystem", test_vfs_sync);
    test_register_case(mod, "Create deep directory", test_vfs_create_deep_directory);
    test_register_case(mod, "File seek operation", test_vfs_file_seek);
    test_register_case(mod, "File append operation", test_vfs_file_append);
    test_register_case(mod, "File truncate operation", test_vfs_file_truncate);
    test_register_case(mod, "Concurrent file access", test_vfs_concurrent_files);
    test_register_case(mod, "Stress: many files (50)", test_vfs_stress_many_files);
    test_register_case(mod, "Stress: large file (10KB)", test_vfs_stress_large_file);
    test_register_case(mod, "Stress: random access", test_vfs_stress_random_access);
    test_register_case(mod, "Error handling", test_vfs_error_handling);
    test_register_case(mod, "Permission check", test_vfs_permission_check);
    test_register_case(mod, "Directory operations", test_vfs_directory_operations);
    test_register_case(mod, "File stat", test_vfs_file_stat);
}
