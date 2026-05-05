#include "kernel_test.h"
#include "kmalloc.h"
#include "mm.h"
#include "vfs.h"
#include "timer.h"
#include "klog.h"
#include "string.h"

extern int32_t vfs_open_internal(const char *path, uint32_t flags, uint64_t pwid);
extern int32_t vfs_close_internal(uint32_t fd);
extern int32_t vfs_read_internal(uint32_t fd, void *buf, uint32_t count);
extern int32_t vfs_write_internal(uint32_t fd, const void *buf, uint32_t count);
extern int vfs_unlink(const char *path, uint64_t pwid);

static int test_perf_kmalloc_throughput(void) {
    const int iterations = 100;
    void *pointers[iterations];
    uint64_t start, end;
    int count = 0;
    
    start = timer_get_ticks();
    
    for (int i = 0; i < iterations; i++) {
        pointers[i] = kmalloc(1024);
        if (pointers[i] != NULL) {
            count++;
        }
    }
    
    end = timer_get_ticks();
    
    for (int i = 0; i < count; i++) {
        kfree(pointers[i]);
    }
    
    TEST_ASSERT_GE(count, iterations * 80 / 100);
    
    klog_kern("[PERF] kmalloc: %d/100 allocs in %d ticks", count, (uint32_t);
    
    return TEST_PASS;
}

static int test_perf_vfs_file_create_delete(void) {
    const int files = 10;
    uint64_t start, end;
    
    start = timer_get_ticks();
    
    for (int i = 0; i < files; i++) {
        char path[32];
        strcpy(path, "/perf_");
        
        if (i < 10) {
            path[6] = '0' + i;
            path[7] = '\0';
        } else {
            path[6] = '0' + (i / 10);
            path[7] = '0' + (i % 10);
            path[8] = '\0';
        }
        strcat(path, ".bin");
        
        int fd = vfs_open_internal(path, VFS_O_CREAT | VFS_O_WRONLY, 0);
        if (fd >= 0) {
            vfs_close_internal(fd);
        }
    }
    
    for (int i = 0; i < files; i++) {
        char path[32];
        strcpy(path, "/perf_");
        
        if (i < 10) {
            path[6] = '0' + i;
            path[7] = '\0';
        } else {
            path[6] = '0' + (i / 10);
            path[7] = '0' + (i % 10);
            path[8] = '\0';
        }
        strcat(path, ".bin");
        
        vfs_unlink(path, 0);
    }
    
    end = timer_get_ticks();
    
    klog_kern("[PERF] File create/delete: %d files in %d ticks", files, (uint32_t);
    
    return TEST_PASS;
}

static int test_perf_vfs_sequential_write(void) {
    int fd = vfs_open_internal("/perf_write.bin", VFS_O_CREAT | VFS_O_WRONLY, 0);
    if (fd < 0) return TEST_SKIP;
    
    const int size = 4096;
    char buffer[size];
    memset(buffer, 'X', size);
    
    uint64_t start = timer_get_ticks();
    
    int total_written = 0;
    for (int i = 0; i < 5; i++) {
        int written = vfs_write_internal(fd, (const uint8_t *)buffer, size);
        if (written > 0) {
            total_written += written;
        }
    }
    
    uint64_t end = timer_get_ticks();
    
    vfs_close_internal(fd);
    
    TEST_ASSERT_GT(total_written, 0);
    
    klog_kern("[PERF] Sequential write: %d KB in %d ticks", total_written / 1024, (uint32_t);
    
    return TEST_PASS;
}

static int test_perf_vfs_sequential_read(void) {
    int fd = vfs_open_internal("/perf_write.bin", VFS_O_RDONLY, 0);
    if (fd < 0) return TEST_SKIP;
    
    const int buf_size = 1024;
    char buffer[buf_size];
    
    uint64_t start = timer_get_ticks();
    
    int total_read = 0;
    while (true) {
        int read = vfs_read_internal(fd, (uint8_t *)buffer, buf_size);
        if (read <= 0) break;
        total_read += read;
    }
    
    uint64_t end = timer_get_ticks();
    
    vfs_close_internal(fd);
    
    klog_kern("[PERF] Sequential read: %d KB in %d ticks", total_read / 1024, (uint32_t);
    
    return TEST_PASS;
}

static int test_perf_memory_fragmentation(void) {
    const int allocs = 30;
    void *pointers[allocs];
    int allocated = 0;
    
    for (int i = 0; i < allocs; i++) {
        pointers[i] = kmalloc((i % 3 + 1) * 256);
        if (pointers[i] != NULL) {
            allocated++;
        }
    }
    
    for (int i = 0; i < allocated; i += 2) {
        kfree(pointers[i]);
        pointers[i] = NULL;
    }
    
    for (int i = 1; i < allocated; i += 2) {
        if (pointers[i]) {
            kfree(pointers[i]);
            pointers[i] = NULL;
        }
    }
    
    for (int i = 0; i < allocated; i += 2) {
        if (!pointers[i]) {
            pointers[i] = kmalloc(512);
            if (pointers[i]) {
                memset(pointers[i], 0xCC, 512);
            }
        }
    }
    
    for (int i = 0; i < allocated; i++) {
        if (pointers[i]) {
            kfree(pointers[i]);
        }
    }
    
    klog_kern("[PERF] Fragmentation test: %d allocations completed", allocated);
    
    return TEST_PASS;
}

static int test_perf_string_operations(void) {
    const int iterations = 500;
    char src[] = "performance test string";
    char dest[50];
    uint64_t start, end;
    
    start = timer_get_ticks();
    
    for (int i = 0; i < iterations; i++) {
        strcpy(dest, src);
        strlen(dest);
        (void)strcmp(dest, src);
    }
    
    end = timer_get_ticks();
    
    klog_kern("[PERF] String ops: %d iterations in %d ticks", iterations, (uint32_t);
    
    return TEST_PASS;
}

void test_performance_register(void) {
    int mod = test_register_module("Performance");
    if (mod < 0) return;
    
    test_register_case(mod, "Kmalloc throughput", test_perf_kmalloc_throughput);
    test_register_case(mod, "File create/delete", test_perf_vfs_file_create_delete);
    test_register_case(mod, "Sequential write", test_perf_vfs_sequential_write);
    test_register_case(mod, "Sequential read", test_perf_vfs_sequential_read);
    test_register_case(mod, "Memory fragmentation", test_perf_memory_fragmentation);
    test_register_case(mod, "String operations", test_perf_string_operations);
}
