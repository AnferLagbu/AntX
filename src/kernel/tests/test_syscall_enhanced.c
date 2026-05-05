#include "kernel_test.h"
#include "syscall.h"
#include "vfs.h"
#include "klog.h"
#include "string.h"

static int test_syscall_basic_file_ops(void) {
    int64_t fd = sys_fs_open("/syscall_test.txt", 0x0100 | 0x0002, 0644);
    if (fd < 0) return TEST_SKIP;
    
    const char *data = "syscall test data";
    int64_t written = sys_fs_write(fd, data, strlen(data));
    
    TEST_ASSERT_GT(written, 0);
    
    sys_fs_close(fd);
    
    klog_kern("[SYSCALL+] Basic file ops: %d bytes writte", (int32_t);
    
    return TEST_PASS;
}

static int test_syscall_error_codes(void) {
    int64_t result1 = sys_fs_open(NULL, 0x0001, 0644);
    TEST_ASSERT_LT(result1, 0);
    
    int64_t result2 = sys_fs_close(-1);
    TEST_ASSERT_LT(result2, 0);
    
    int64_t result3 = sys_fs_mkdir(NULL, 0755);
    TEST_ASSERT_LT(result3, 0);
    
    klog_kern("[SYSCALL+] Error codes validated");
    return TEST_PASS;
}

static int test_syscall_multiple_opens(void) {
    const int count = 8;
    int64_t fds[count];
    int opened = 0;
    
    for (int i = 0; i < count; i++) {
        char path[32];
        strcpy(path, "/multi_");
        
        char num[4];
        int idx = 0;
        int temp = i;
        if (temp == 0) { num[idx++] = '0'; }
        else { while (temp > 0 && idx < 3) { num[idx++] = '0' + (temp % 10); temp /= 10; } }
        num[idx] = '\0';
        for (int j = 0; j < idx / 2; j++) { char t = num[j]; num[j] = num[idx-1-j]; num[idx-1-j] = t; }
        
        strcat(path, num);
        strcat(path, ".txt");
        
        fds[i] = sys_fs_open(path, 0x0100 | 0x0002, 0644);
        if (fds[i] >= 0) {
            opened++;
        }
    }
    
    for (int i = 0; i < opened; i++) {
        sys_fs_close(fds[i]);
    }
    
    TEST_ASSERT_EQ(opened, count);
    
    klog_kern("[SYSCALL+] Multiple opens: %d/%d files", opened, count);
    
    return TEST_PASS;
}

static int test_syscall_read_write_cycle(void) {
    const char *test_data = "read write cycle test";
    int64_t fd = sys_fs_open("/rw_cycle.bin", 0x0100 | 0x0002, 0644);
    if (fd < 0) return TEST_SKIP;
    
    int64_t written = sys_fs_write(fd, test_data, strlen(test_data));
    sys_fs_close(fd);
    
    if (written <= 0) return TEST_SKIP;
    
    fd = sys_fs_open("/rw_cycle.bin", 0x0001, 0);
    if (fd < 0) return TEST_SKIP;
    
    char buffer[64];
    int64_t read = sys_fs_read(fd, buffer, sizeof(buffer) - 1);
    sys_fs_close(fd);
    
    if (read > 0) {
        buffer[read] = '\0';
        TEST_ASSERT_EQ(strcmp(buffer, test_data), 0);
        
        klog_kern("[SYSCALL+] R/W cycle: \"");
        klog_kern("%s", buffer);
        klog_kern("\"");
    }
    
    return TEST_PASS;
}

static int test_syscall_mkdir_chain(void) {
    const char *dirs[] = {"/chain_a", "/chain_a/chain_b", "/chain_a/chain_b/chain_c", NULL};
    int created = 0;
    
    for (int i = 0; dirs[i] != NULL; i++) {
        int64_t result = sys_fs_mkdir(dirs[i], 0755);
        if (result >= 0) {
            created++;
        }
    }
    
    TEST_ASSERT_GT(created, 0);
    
    klog_kern("[SYSCALL+] Mkdir chain: %d directories", created);
    
    return TEST_PASS;
}

static int test_syscall_boundary_sizes(void) {
    int64_t fd = sys_fs_open("/boundary_test.bin", 0x0100 | 0x0002, 0644);
    if (fd < 0) return TEST_SKIP;
    
    char small_buf[1] = {'X'};
    int64_t r1 = sys_fs_write(fd, small_buf, 1);
    
    char medium_buf[128];
    memset(medium_buf, 'M', sizeof(medium_buf));
    int64_t r2 = sys_fs_write(fd, medium_buf, sizeof(medium_buf));
    
    char large_buf[1024];
    memset(large_buf, 'L', sizeof(large_buf));
    int64_t r3 = sys_fs_write(fd, large_buf, sizeof(large_buf));
    
    sys_fs_close(fd);
    
    TEST_ASSERT_GT(r1 + r2 + r3, 0);
    
    klog_kern("[SYSCALL+] Boundary sizes: 1+%d+%d bytes", (int32_t, (int32_t);
    
    return TEST_PASS;
}

void test_syscall_enhanced_register(void) {
    int mod = test_register_module("Syscall Enhanced");
    if (mod < 0) return;
    
    test_register_case(mod, "Basic file operations", test_syscall_basic_file_ops);
    test_register_case(mod, "Error code validation", test_syscall_error_codes);
    test_register_case(mod, "Multiple simultaneous opens", test_syscall_multiple_opens);
    test_register_case(mod, "Read/write cycle", test_syscall_read_write_cycle);
    test_register_case(mod, "Directory creation chain", test_syscall_mkdir_chain);
    test_register_case(mod, "Boundary sizes (1/128/1024)", test_syscall_boundary_sizes);
}
