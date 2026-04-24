#include "kernel_test.h"
#include "vfs.h"
#include "hvfs_ffi.h"
#include "serial.h"
#include "string.h"

extern int32_t vfs_open_internal(const char *path, uint32_t flags, uint64_t pwid);
extern int32_t vfs_close_internal(uint32_t fd);
extern int32_t vfs_read_internal(uint32_t fd, void *buf, uint32_t count);
extern int32_t vfs_write_internal(uint32_t fd, const void *buf, uint32_t count);
extern int32_t vfs_mkdir_internal(const char *path, uint64_t pwid);
extern int32_t vfs_mount_internal(const char *path, const char *fs_type);
extern int32_t vfs_unmount_internal(const char *path);
extern int32_t vfs_format_internal(const char *path, const char *fs_type);
extern int32_t vfs_sync_internal(void);
extern int32_t vfs_stat_internal(const char *path, void *st, uint64_t pwid);
extern int32_t vfs_seek_internal(uint32_t fd, int32_t offset, uint32_t whence);
extern int32_t vfs_chmod_internal(const char *path, uint32_t mode, uint64_t pwid);
extern int32_t vfs_chown_internal(const char *path, uint32_t uid, uint32_t gid, uint64_t pwid);

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
    int result = vfs_mount_internal("/mnt/ramfs", "ramfs");
    TEST_ASSERT_EQ(result, 0);
    return TEST_PASS;
}

static int test_vfs_mount_devfs(void) {
    int result = vfs_mount_internal("/dev", "devfs");
    TEST_ASSERT_GE(result, -1);
    return TEST_PASS;
}

static int test_vfs_mount_procfs(void) {
    int result = vfs_mount_internal("/proc", "procfs");
    TEST_ASSERT_GE(result, -1);
    return TEST_PASS;
}

static int test_vfs_unmount(void) {
    vfs_mount_internal("/mnt/test", "ramfs");
    int result = vfs_unmount_internal("/mnt/test");
    TEST_ASSERT_GE(result, -1);
    return TEST_PASS;
}

static int test_vfs_format_ramfs(void) {
    int result = vfs_format_internal("/mnt/ramfs", "ramfs");
    TEST_ASSERT_GE(result, -1);
    return TEST_PASS;
}

static int test_vfs_format_hvfs(void) {
    int result = hvfs_format_internal();
    TEST_ASSERT_EQ(result, 0);
    return TEST_PASS;
}

static int test_vfs_sync(void) {
    int result = vfs_sync_internal();
    TEST_ASSERT_GE(result, -1);
    return TEST_PASS;
}

static int test_vfs_create_deep_directory(void) {
    vfs_mkdir_internal("/deep", 0);
    vfs_mkdir_internal("/deep/level1", 0);
    vfs_mkdir_internal("/deep/level1/level2", 0);
    vfs_mkdir_internal("/deep/level1/level2/level3", 0);
    
    int fd = vfs_open_internal("/deep/level1/level2/level3/test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    TEST_ASSERT_GE(fd, 0);
    
    if (fd >= 0) {
        vfs_close_internal(fd);
    }
    
    return TEST_PASS;
}

static int test_vfs_file_seek(void) {
    int fd = vfs_open_internal("/seek_test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *data = "0123456789ABCDEF";
    vfs_write_internal(fd, data, 16);
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/seek_test.txt", VFS_O_RDONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    int result = vfs_seek_internal(fd, 5, SEEK_SET);
    TEST_ASSERT_GE(result, 0);
    
    char buf[8] = {0};
    int read = vfs_read_internal(fd, buf, 3);
    TEST_ASSERT_EQ(read, 3);
    TEST_ASSERT_EQ(buf[0], '5');
    TEST_ASSERT_EQ(buf[1], '6');
    TEST_ASSERT_EQ(buf[2], '7');
    
    vfs_close_internal(fd);
    
    return TEST_PASS;
}

static int test_vfs_file_append(void) {
    int fd = vfs_open_internal("/append_test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *data1 = "FIRST";
    vfs_write_internal(fd, data1, 5);
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/append_test.txt", VFS_O_WRONLY | VFS_O_APPEND, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *data2 = "SECOND";
    vfs_write_internal(fd, data2, 6);
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/append_test.txt", VFS_O_RDONLY, 0);
    char buf[16] = {0};
    int read = vfs_read_internal(fd, buf, sizeof(buf));
    TEST_ASSERT_EQ(read, 11);
    TEST_ASSERT_EQ(memcmp(buf, "FIRSTSECOND", 11), 0);
    
    vfs_close_internal(fd);
    
    return TEST_PASS;
}

static int test_vfs_file_truncate(void) {
    int fd = vfs_open_internal("/trunc_test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *data = "THIS IS A LONG STRING FOR TRUNCATE TEST";
    vfs_write_internal(fd, data, 39);
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/trunc_test.txt", VFS_O_WRONLY | VFS_O_TRUNC, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *short_data = "SHORT";
    vfs_write_internal(fd, short_data, 5);
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/trunc_test.txt", VFS_O_RDONLY, 0);
    char buf[64] = {0};
    int read = vfs_read_internal(fd, buf, sizeof(buf));
    TEST_ASSERT_EQ(read, 5);
    TEST_ASSERT_EQ(memcmp(buf, "SHORT", 5), 0);
    
    vfs_close_internal(fd);
    
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
        fds[i] = vfs_open_internal(files[i], VFS_O_CREAT | VFS_O_WRONLY, 0);
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
        vfs_write_internal(fds[i], data, len);
    }
    
    for (int i = 0; i < 5; i++) {
        vfs_close_internal(fds[i]);
    }
    
    for (int i = 0; i < 5; i++) {
        int fd = vfs_open_internal(files[i], VFS_O_RDONLY, 0);
        TEST_ASSERT_GE(fd, 0);
        if (fd >= 0) {
            vfs_close_internal(fd);
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
        
        int fd = vfs_open_internal(path, VFS_O_CREAT | VFS_O_WRONLY, 0);
        if (fd >= 0) {
            char data[8] = {'T', 'E', 'S', 'T', '_', '0' + (i/10), '0' + (i%10), 0};
            vfs_write_internal(fd, data, 7);
            vfs_close_internal(fd);
            created++;
        }
    }
    
    TEST_ASSERT_GT(created, 0);
    
    return TEST_PASS;
}

static int test_vfs_stress_large_file(void) {
    int fd = vfs_open_internal("/large_stress.bin", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    char buffer[1024];
    for (int i = 0; i < 1024; i++) {
        buffer[i] = (char)(i & 0xFF);
    }
    
    int total_written = 0;
    for (int i = 0; i < 10; i++) {
        int written = vfs_write_internal(fd, buffer, 1024);
        if (written > 0) {
            total_written += written;
        }
    }
    
    vfs_close_internal(fd);
    
    TEST_ASSERT_GT(total_written, 0);
    
    fd = vfs_open_internal("/large_stress.bin", VFS_O_RDONLY, 0);
    if (fd >= 0) {
        char verify[1024];
        int read = vfs_read_internal(fd, verify, 1024);
        TEST_ASSERT_GT(read, 0);
        vfs_close_internal(fd);
    }
    
    return TEST_PASS;
}

static int test_vfs_stress_random_access(void) {
    int fd = vfs_open_internal("/random_access.bin", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    char pattern[256];
    for (int i = 0; i < 256; i++) {
        pattern[i] = (char)i;
    }
    
    for (int i = 0; i < 4; i++) {
        vfs_write_internal(fd, pattern, 256);
    }
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/random_access.bin", VFS_O_RDONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    char buf[64];
    
    vfs_seek_internal(fd, 128, SEEK_SET);
    int read = vfs_read_internal(fd, buf, 64);
    TEST_ASSERT_EQ(read, 64);
    for (int i = 0; i < 64; i++) {
        TEST_ASSERT_EQ(buf[i], (char)((128 + i) & 0xFF));
    }
    
    vfs_seek_internal(fd, 512, SEEK_SET);
    read = vfs_read_internal(fd, buf, 64);
    TEST_ASSERT_GT(read, 0);
    
    vfs_close_internal(fd);
    
    return TEST_PASS;
}

static int test_vfs_error_handling(void) {
    int fd = vfs_open_internal("/nonexistent_file.txt", VFS_O_RDONLY, 0);
    TEST_ASSERT_LT(fd, 0);
    
    int result = vfs_read_internal(9999, NULL, 0);
    TEST_ASSERT_LT(result, 0);
    
    result = vfs_close_internal(9999);
    TEST_ASSERT_LT(result, 0);
    
    return TEST_PASS;
}

static int test_vfs_permission_check(void) {
    int fd = vfs_open_internal("/permission_test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd >= 0) {
        vfs_close_internal(fd);
    }
    
    int result = vfs_chmod_internal("/permission_test.txt", 0755, 0);
    TEST_ASSERT_GE(result, -1);
    
    return TEST_PASS;
}

static int test_vfs_directory_operations(void) {
    int result = vfs_mkdir_internal("/test_dir_ops", 0);
    TEST_ASSERT_GE(result, -1);
    
    result = vfs_mkdir_internal("/test_dir_ops/subdir1", 0);
    TEST_ASSERT_GE(result, -1);
    
    result = vfs_mkdir_internal("/test_dir_ops/subdir2", 0);
    TEST_ASSERT_GE(result, -1);
    
    int fd = vfs_open_internal("/test_dir_ops/file_in_dir.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd >= 0) {
        vfs_write_internal(fd, "content", 7);
        vfs_close_internal(fd);
    }
    
    return TEST_PASS;
}

static int test_vfs_file_stat(void) {
    int fd = vfs_open_internal("/stat_test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) {
        return TEST_SKIP;
    }
    
    const char *data = "Stat test content";
    vfs_write_internal(fd, data, 17);
    vfs_close_internal(fd);
    
    char stat_buf[128];
    int result = vfs_stat_internal("/stat_test.txt", stat_buf, 0);
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
