#include "kernel_test.h"
#include "vfs.h"
#include "syscall.h"
#include "kmalloc.h"
#include "string.h"

extern int32_t vfs_open_internal(const char *path, uint32_t flags, uint64_t pwid);
extern int32_t vfs_close_internal(uint32_t fd);
extern int32_t vfs_read_internal(uint32_t fd, void *buf, uint32_t count);
extern int32_t vfs_write_internal(uint32_t fd, const void *buf, uint32_t count);
extern int32_t vfs_mkdir_internal(const char *path, uint64_t pwid);
extern int vfs_unlink(const char *path, uint64_t pwid);
extern int32_t vfs_stat_internal(const char *path, void *st, uint64_t pwid);
extern int32_t vfs_seek_internal(uint32_t fd, int32_t offset, int whence);

static int test_vfs_open_nonexistent(void) {
    int fd = vfs_open_internal("/nonexistent_file_12345.txt", VFS_O_RDONLY, 0);
    TEST_ASSERT_LT(fd, 0);
    return TEST_PASS;
}

static int test_vfs_read_closed_fd(void) {
    char buffer[16];
    
    int fd = vfs_open_internal("/temp_read_test.bin", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) return TEST_SKIP;
    
    vfs_close_internal(fd);
    
    int64_t result = vfs_read_internal(fd, buffer, sizeof(buffer));
    TEST_ASSERT_LT(result, 0);
    return TEST_PASS;
}

static int test_vfs_write_to_readonly(void) {
    const char *path = "/readonly_test.txt";
    
    int fd1 = vfs_open_internal(path, VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd1 < 0) return TEST_SKIP;
    
    const char *data = "original content";
    vfs_write_internal(fd1, data, strlen(data));
    vfs_close_internal(fd1);
    
    int fd2 = vfs_open_internal(path, VFS_O_RDONLY, 0);
    if (fd2 < 0) return TEST_SKIP;
    
    int written = vfs_write_internal(fd2, data, 5);
    TEST_ASSERT_LT(written, 0);
    
    vfs_close_internal(fd2);
    return TEST_PASS;
}

static int test_vfs_mkdir_duplicate(void) {
    int result1 = vfs_mkdir_internal("/error_dir", 0);
    if (result1 < 0) return TEST_SKIP;
    
    int result2 = vfs_mkdir_internal("/error_dir", 0);
    
    TEST_ASSERT_LT(result2, 0);
    return TEST_PASS;
}

static int test_vfs_unlink_nonexistent(void) {
    int result = vfs_unlink("/does_not_exist_xyz.txt", 0);
    TEST_ASSERT_LT(result, 0);
    return TEST_PASS;
}

static int test_syscall_invalid_params(void) {
    int64_t open_result = sys_fs_open(NULL, VFS_O_RDONLY, 0644);
    TEST_ASSERT_LT(open_result, 0);
    
    int64_t close_result = sys_fs_close(-1);
    TEST_ASSERT_LT(close_result, 0);
    
    int64_t mkdir_result = sys_fs_mkdir(NULL, 0755);
    TEST_ASSERT_LT(mkdir_result, 0);
    
    return TEST_PASS;
}

static int test_kmalloc_oversized_request(void) {
    void *ptr = kmalloc(1024 * 1024 * 1024ULL);
    
    if (ptr != NULL) {
        kfree(ptr);
    }
    
    return TEST_PASS;
}

static int test_vfs_stat_nonexistent(void) {
    struct vfs_stat st;
    int result = vfs_stat_internal("/nonexistent_for_stat.txt", &st, 0);
    TEST_ASSERT_LT(result, 0);
    return TEST_PASS;
}

static int test_vfs_seek_invalid_whence(void) {
    int fd = vfs_open_internal("/seek_invalid.bin", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) return TEST_SKIP;
    
    int seek_result = vfs_seek_internal(fd, 100, -1);
    TEST_ASSERT_LT(seek_result, 0);
    
    seek_result = vfs_seek_internal(fd, 100, 9999);
    TEST_ASSERT_LT(seek_result, 0);
    
    vfs_close_internal(fd);
    return TEST_PASS;
}

void test_error_handling_register(void) {
    int mod = test_register_module("Error Handling");
    if (mod < 0) return;
    
    test_register_case(mod, "Open nonexistent file", test_vfs_open_nonexistent);
    test_register_case(mod, "Read closed FD", test_vfs_read_closed_fd);
    test_register_case(mod, "Write to readonly file", test_vfs_write_to_readonly);
    test_register_case(mod, "Mkdir duplicate directory", test_vfs_mkdir_duplicate);
    test_register_case(mod, "Unlink nonexistent file", test_vfs_unlink_nonexistent);
    test_register_case(mod, "Syscall invalid params", test_syscall_invalid_params);
    test_register_case(mod, "Kmalloc oversized request", test_kmalloc_oversized_request);
    test_register_case(mod, "Stat nonexistent file", test_vfs_stat_nonexistent);
    test_register_case(mod, "Seek invalid whence", test_vfs_seek_invalid_whence);
}
