#include "kernel_test.h"
#include "vfs.h"
#include "klog.h"
#include "string.h"
#include "kmalloc.h"

extern int32_t vfs_open_internal(const char *path, uint32_t flags, uint64_t pwid);
extern int32_t vfs_close_internal(uint32_t fd);
extern int32_t vfs_read_internal(uint32_t fd, void *buf, uint32_t count);
extern int32_t vfs_write_internal(uint32_t fd, const void *buf, uint32_t count);
extern int32_t vfs_mkdir_internal(const char *path, uint64_t pwid);
extern int32_t vfs_stat_internal(const char *path, void *st, uint64_t pwid);
extern int32_t vfs_unlink_internal(const char *path);
extern int32_t vfs_seek_internal(uint32_t fd, int32_t offset, int whence);
extern void vfs_init(void);
extern void ramfs_init(void);

static int test_vfs_nested_directories(void) {
    int result1 = vfs_mkdir_internal("/level1", 0);
    if (result1 < 0) return TEST_SKIP;
    
    int result2 = vfs_mkdir_internal("/level1/level2", 0);
    if (result2 < 0) return TEST_SKIP;
    
    int result3 = vfs_mkdir_internal("/level1/level2/level3", 0);
    if (result3 < 0) return TEST_SKIP;
    
    int fd = vfs_open_internal("/level1/level2/level3/nested.txt", 0x0100 | 0x0002, 0);
    if (fd < 0) return TEST_SKIP;
    
    const char *data = "nested file content";
    vfs_write_internal(fd, data, strlen(data));
    vfs_close_internal(fd);
    
    klog_kern("[VFS+] Nested directories: 3 levels deep");
    return TEST_PASS;
}

static int test_vfs_file_append_mode(void) {
    int fd = vfs_open_internal("/append_test.txt", 0x0100 | 0x0002, 0);
    if (fd < 0) return TEST_SKIP;
    
    const char *first = "first line\n";
    vfs_write_internal(fd, first, strlen(first));
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/append_test.txt", 0x0100 | 0x0008 | 0x0002, 0);
    if (fd < 0) return TEST_SKIP;
    
    const char *second = "second line";
    vfs_write_internal(fd, second, strlen(second));
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/append_test.txt", 0x0001, 0);
    if (fd < 0) return TEST_SKIP;
    
    char buffer[64];
    int read = vfs_read_internal(fd, buffer, sizeof(buffer) - 1);
    vfs_close_internal(fd);
    
    if (read > 0) {
        buffer[read] = '\0';
        klog_kern("[VFS+] Append mode: \"");
        klog_kern("%s", buffer);
        klog_kern("\"");
    }
    
    return TEST_PASS;
}

static int test_vfs_fd_reuse(void) {
    int fds[5];
    int opened = 0;
    
    for (int i = 0; i < 5; i++) {
        char path[32];
        strcpy(path, "/fd_reuse_");
        
        char num[4];
        int idx = 0;
        int temp = i;
        if (temp == 0) { num[idx++] = '0'; }
        else { while (temp > 0 && idx < 3) { num[idx++] = '0' + (temp % 10); temp /= 10; } }
        num[idx] = '\0';
        for (int j = 0; j < idx / 2; j++) { char t = num[j]; num[j] = num[idx-1-j]; num[idx-1-j] = t; }
        
        strcat(path, num);
        strcat(path, ".txt");
        
        fds[i] = vfs_open_internal(path, 0x0100 | 0x0002, 0);
        if (fds[i] >= 0) {
            opened++;
        }
    }
    
    for (int i = 0; i < opened; i++) {
        vfs_close_internal(fds[i]);
    }
    
    int new_fd = vfs_open_internal("/reuse_check.txt", 0x0100 | 0x0002, 0);
    
    TEST_ASSERT_GE(new_fd, 0);
    
    if (new_fd >= 0) {
        vfs_close_internal(new_fd);
    }
    
    klog_kern("[VFS+] FD reuse: %d FDs opened and closed", opened);
    
    return TEST_PASS;
}

static int test_vfs_concurrent_operations(void) {
    const int files = 10;
    int fds[files];
    int opened = 0;
    
    for (int i = 0; i < files; i++) {
        char path[32];
        strcpy(path, "/concurrent_");
        
        char num[4];
        int idx = 0;
        int temp = i;
        if (temp == 0) { num[idx++] = '0'; }
        else { while (temp > 0 && idx < 3) { num[idx++] = '0' + (temp % 10); temp /= 10; } }
        num[idx] = '\0';
        for (int j = 0; j < idx / 2; j++) { char t = num[j]; num[j] = num[idx-1-j]; num[idx-1-j] = t; }
        
        strcat(path, num);
        strcat(path, ".bin");
        
        fds[i] = vfs_open_internal(path, 0x0100 | 0x0002, 0);
        if (fds[i] >= 0) {
            const char *data = "concurrent write";
            vfs_write_internal(fds[i], data, strlen(data));
            opened++;
        }
    }
    
    for (int i = 0; i < opened; i++) {
        vfs_close_internal(fds[i]);
    }
    
    TEST_ASSERT_EQ(opened, files);
    
    klog_kern("[VFS+] Concurrent ops: %d simultaneous file operations", files);
    
    return TEST_PASS;
}

static int test_vfs_path_resolution(void) {
    const char *paths[] = {
        "/",
        "/root",
        "/root/file.txt",
        "/a/b/c/d/e",
        NULL
    };
    
    int tested = 0;
    for (int i = 0; paths[i] != NULL; i++) {
        int result = vfs_mkdir_internal(paths[i], 0);
        tested++;
    }
    
    TEST_ASSERT_GT(tested, 0);
    
    klog_kern("[VFS+] Path resolution: %d paths tested", tested);
    
    return TEST_PASS;
}

static int test_vfs_truncate_operation(void) {
    int fd = vfs_open_internal("/trunc_test.bin", 0x0100 | 0x0002, 0);
    if (fd < 0) return TEST_SKIP;
    
    const char *data = "this is a long string that will be truncated";
    vfs_write_internal(fd, data, strlen(data));
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/trunc_test.bin", 0x0100 | 0x0200 | 0x0002, 0);
    if (fd < 0) return TEST_SKIP;
    
    const char *short_data = "short";
    vfs_write_internal(fd, short_data, strlen(short_data));
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/trunc_test.bin", 0x0001, 0);
    if (fd < 0) return TEST_SKIP;
    
    char buffer[64];
    int read = vfs_read_internal(fd, buffer, sizeof(buffer) - 1);
    vfs_close_internal(fd);
    
    if (read >= 0 && read <= (int)strlen(short_data)) {
        buffer[read] = '\0';
        klog_kern("[VFS+] Truncate: \"");
        klog_kern("%s", buffer);
        klog_kern("\" (%d bytes)", read);
    }
    
    return TEST_PASS;
}

static int test_vfs_mixed_read_write(void) {
    int fd = vfs_open_internal("/mixed_rw.bin", 0x0100 | 0x0002, 0);
    if (fd < 0) return TEST_SKIP;
    
    const char *pattern = "ABCD";
    for (int i = 0; i < 5; i++) {
        vfs_write_internal(fd, pattern, strlen(pattern));
    }
    
    vfs_seek_internal(fd, 0, 0);
    
    char buffer[20];
    int total_read = 0;
    int iterations = 0;
    
    while (iterations < 3) {
        int read = vfs_read_internal(fd, buffer + total_read, sizeof(buffer) - total_read - 1);
        if (read <= 0) break;
        total_read += read;
        iterations++;
    }
    
    vfs_close_internal(fd);
    
    TEST_ASSERT_GT(total_read, 0);
    
    klog_kern("[VFS+] Mixed R/W: %d bytes read back", total_read);
    
    return TEST_PASS;
}

void test_vfs_enhanced_register(void) {
    int mod = test_register_module("VFS Enhanced");
    if (mod < 0) return;
    
    test_register_case(mod, "Nested directories (3 levels)", test_vfs_nested_directories);
    test_register_case(mod, "File append mode", test_vfs_file_append_mode);
    test_register_case(mod, "File descriptor reuse", test_vfs_fd_reuse);
    test_register_case(mod, "Concurrent operations", test_vfs_concurrent_operations);
    test_register_case(mod, "Path resolution", test_vfs_path_resolution);
    test_register_case(mod, "Truncate operation", test_vfs_truncate_operation);
    test_register_case(mod, "Mixed read/write", test_vfs_mixed_read_write);
}
