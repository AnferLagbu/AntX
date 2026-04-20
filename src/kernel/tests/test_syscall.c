#include "kernel_test.h"
#include "syscall.h"
#include "vfs.h"
#include "serial.h"
#include "string.h"

static int test_syscall_getpid(void) {
    int64_t pid = sys_proc_getid();
    if (pid == 0) {
        return TEST_SKIP;
    }
    TEST_ASSERT_GT(pid, 0);
    
    return TEST_PASS;
}

static int test_syscall_write_read(void) {
    const char *msg = "Syscall test";
    int len = strlen(msg);
    
    int64_t written = sys_fs_write(1, msg, len);
    TEST_ASSERT_GE(written, 0);
    
    return TEST_PASS;
}

static int test_syscall_open_close(void) {
    int64_t fd = sys_fs_open("/syscall_test.txt", VFS_O_CREAT | VFS_O_WRONLY, 0);
    TEST_ASSERT_GE(fd, 0);
    
    int64_t result = sys_fs_close(fd);
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_syscall_invalid_fd(void) {
    char buffer[16];
    int64_t result = sys_fs_read(9999, buffer, sizeof(buffer));
    TEST_ASSERT_LT(result, 0);
    
    return TEST_PASS;
}

static int test_syscall_invalid_path(void) {
    int64_t fd = sys_fs_open("/nonexistent/path/file.txt", VFS_O_RDONLY, 0);
    TEST_ASSERT_LT(fd, 0);
    
    return TEST_PASS;
}

static int test_syscall_mkdir(void) {
    int64_t result = sys_fs_mkdir("/syscall_dir", 0);
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_syscall_yield(void) {
    int64_t result = sys_proc_yield();
    TEST_ASSERT_GE(result, 0);
    
    return TEST_PASS;
}

void test_syscall_register(void) {
    int mod = test_register_module("System Calls");
    
    test_register_case(mod, "getpid syscall", test_syscall_getpid);
    test_register_case(mod, "write syscall", test_syscall_write_read);
    test_register_case(mod, "open/close syscall", test_syscall_open_close);
    test_register_case(mod, "Invalid FD handling", test_syscall_invalid_fd);
    test_register_case(mod, "Invalid path handling", test_syscall_invalid_path);
    test_register_case(mod, "mkdir syscall", test_syscall_mkdir);
    test_register_case(mod, "yield syscall", test_syscall_yield);
}
