#include "kernel_test.h"
#include "vfs.h"
#include "syscall.h"
#include "string.h"

extern int32_t vfs_open_internal(const char *path, uint32_t flags, uint64_t pwid);
extern int32_t vfs_close_internal(uint32_t fd);
extern int32_t vfs_read_internal(uint32_t fd, void *buf, uint32_t count);
extern int32_t vfs_write_internal(uint32_t fd, const void *buf, uint32_t count);
extern int32_t vfs_seek_internal(uint32_t fd, int32_t offset, int whence);
extern int32_t vfs_stat_internal(const char *path, void *st, uint64_t pwid);

static int test_vfs_empty_path(void) {
    int fd = vfs_open_internal("", VFS_O_RDONLY, 0);
    TEST_ASSERT_LT(fd, 0);
    return TEST_PASS;
}

static int test_vfs_very_long_path(void) {
    char long_path[256];
    memset(long_path, 'a', 255);
    long_path[255] = '\0';
    
    int fd = vfs_open_internal(long_path, VFS_O_RDONLY, 0);
    TEST_ASSERT_LT(fd, 0);
    return TEST_PASS;
}

static int test_vfs_special_chars_path(void) {
    const char *special_paths[] = {
        "/test/../etc/passwd",
        "/test/./hidden",
        "/test//double/slash",
        NULL
    };
    
    for (int i = 0; special_paths[i] != NULL; i++) {
        int fd = vfs_open_internal(special_paths[i], VFS_O_RDONLY, 0);
        if (fd >= 0) {
            vfs_close_internal(fd);
        }
    }
    
    return TEST_PASS;
}

static int test_syscall_invalid_fd_range(void) {
    char buffer[16];
    
    int64_t result1 = vfs_read_internal(0xFFFFFFFF, buffer, sizeof(buffer));
    TEST_ASSERT_LT(result1, 0);
    
    int64_t result2 = vfs_close_internal(0xFFFFFFFF);
    TEST_ASSERT_LT(result2, 0);
    
    return TEST_PASS;
}

static int test_vfs_multiple_opens_same_file(void) {
    const char *path = "/edge_test.txt";
    
    int fd1 = vfs_open_internal(path, VFS_O_CREAT | VFS_O_RDWR, 0);
    if (fd1 < 0) return TEST_SKIP;
    
    int fd2 = vfs_open_internal(path, VFS_O_RDONLY, 0);
    if (fd2 < 0) {
        vfs_close_internal(fd1);
        return TEST_PASS;
    }
    
    const char *data = "test data";
    vfs_write_internal(fd1, data, strlen(data));
    
    char buf[20];
    int read = vfs_read_internal(fd2, buf, sizeof(buf) - 1);
    TEST_ASSERT_GT(read, 0);
    
    vfs_close_internal(fd1);
    vfs_close_internal(fd2);
    
    return TEST_PASS;
}

static int test_vfs_write_zero_bytes(void) {
    int fd = vfs_open_internal("/zero_write.bin", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) return TEST_SKIP;
    
    int written = vfs_write_internal(fd, (const uint8_t *)"", 0);
    TEST_ASSERT_GE(written, 0);
    
    vfs_close_internal(fd);
    return TEST_PASS;
}

static int test_vfs_seek_beyond_eof(void) {
    int fd = vfs_open_internal("/seek_beyond.bin", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) return TEST_SKIP;
    
    const char *data = "short";
    vfs_write_internal(fd, data, 5);
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/seek_beyond.bin", VFS_O_RDONLY, 0);
    if (fd < 0) return TEST_SKIP;
    
    int seek_result = vfs_seek_internal(fd, 10000, 0);
    TEST_ASSERT_GE(seek_result, 0);
    
    char buf[10];
    int read = vfs_read_internal(fd, buf, sizeof(buf));
    TEST_ASSERT_EQ(read, 0);
    
    vfs_close_internal(fd);
    return TEST_PASS;
}

static int test_vfs_truncate_to_zero(void) {
    int fd = vfs_open_internal("/trunc_zero.bin", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) return TEST_SKIP;
    
    const char *data = "important data that will be removed";
    vfs_write_internal(fd, data, strlen(data));
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/trunc_zero.bin", VFS_O_WRONLY | VFS_O_TRUNC, 0);
    if (fd < 0) return TEST_SKIP;
    
    vfs_close_internal(fd);
    
    fd = vfs_open_internal("/trunc_zero.bin", VFS_O_RDONLY, 0);
    if (fd < 0) return TEST_SKIP;
    
    struct vfs_stat st;
    int stat_result = vfs_stat_internal("/trunc_zero.bin", &st, 0);
    if (stat_result >= 0) {
        TEST_ASSERT_EQ(st.size, 0);
    }
    
    vfs_close_internal(fd);
    return TEST_PASS;
}

static int test_string_edge_cases(void) {
    char buf1[10];
    memset(buf1, 0, sizeof(buf1));
    
    strcpy(buf1, "");
    TEST_ASSERT_EQ(strlen(buf1), 0);
    
    char buf2[5];
    strncpy(buf2, "hello world", 4);
    buf2[4] = '\0';
    TEST_ASSERT_EQ(strlen(buf2), 4);
    TEST_ASSERT_EQ(strcmp(buf2, "hell"), 0);
    
    return TEST_PASS;
}

void test_edge_cases_register(void) {
    int mod = test_register_module("Edge Cases");
    if (mod < 0) return;
    
    test_register_case(mod, "Empty path handling", test_vfs_empty_path);
    test_register_case(mod, "Very long path (>255 chars)", test_vfs_very_long_path);
    test_register_case(mod, "Special characters in path", test_vfs_special_chars_path);
    test_register_case(mod, "Invalid FD range", test_syscall_invalid_fd_range);
    test_register_case(mod, "Multiple opens same file", test_vfs_multiple_opens_same_file);
    test_register_case(mod, "Write zero bytes", test_vfs_write_zero_bytes);
    test_register_case(mod, "Seek beyond EOF", test_vfs_seek_beyond_eof);
    test_register_case(mod, "Truncate to zero size", test_vfs_truncate_to_zero);
    test_register_case(mod, "String edge cases", test_string_edge_cases);
}
