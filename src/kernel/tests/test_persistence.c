#include "kernel_test.h"
#include "hvfs_rust.h"
#include "hvfs.h"
#include "pwid.h"
#include "string.h"
#include "serial.h"

static int test_persistence_module = -1;
static int hvfs_initialized = 0;
static int pwid_initialized = 0;
static uint64_t test_root_pwid = 0;
static int disk_mode_available = 0;

static int ensure_hvfs_initialized(void) {
    if (!hvfs_initialized) {
        if (!pwid_initialized) {
            pwid_init();
            pwid_initialized = 1;
        }
        
        pwid_create_original_root("test_root_password");
        
        struct pwid_entry *root = pwid_find_by_note("root");
        if (root) {
            test_root_pwid = root->pwid;
        } else {
            serial_puts(SERIAL_COM1, "[Persistence Test] Failed to create root PWID\n");
            return -1;
        }
        
        if (hvfs_disk_init() == 0) {
            disk_mode_available = 1;
            serial_puts(SERIAL_COM1, "[Persistence Test] Disk mode enabled\n");
        } else {
            hvfs_init();
            if (hvfs_format() != 0) {
                serial_puts(SERIAL_COM1, "[Persistence Test] HvFS format failed\n");
                return -1;
            }
            disk_mode_available = 0;
            serial_puts(SERIAL_COM1, "[Persistence Test] Memory mode (no disk)\n");
        }
        
        hvfs_mkdir("/etc", test_root_pwid);
        hvfs_mkdir("/tmp", test_root_pwid);
        hvfs_initialized = 1;
    }
    return 0;
}

static int test_hvfs_file_persistence(void) {
    if (ensure_hvfs_initialized() != 0) {
        return TEST_SKIP;
    }
    
    if (!disk_mode_available) {
        serial_puts(SERIAL_COM1, "[SKIP] No disk available for persistence test\n");
        return TEST_SKIP;
    }
    
    const char *test_file = "/test_persist.txt";
    const char *test_data = "PERSISTENCE_TEST_DATA_12345";
    char read_buf[64];
    
    int fd = hvfs_open(test_file, HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, test_root_pwid);
    if (fd < 0) {
        serial_puts(SERIAL_COM1, "[FAIL] Failed to open file for write\n");
    }
    TEST_ASSERT(fd >= 0);
    
    int written = hvfs_write(fd, test_data, strlen(test_data));
    if (written != (int)strlen(test_data)) {
        serial_puts(SERIAL_COM1, "[FAIL] Write failed: expected ");
        serial_put_dec(SERIAL_COM1, strlen(test_data));
        serial_puts(SERIAL_COM1, ", got ");
        serial_put_dec(SERIAL_COM1, written);
        serial_puts(SERIAL_COM1, "\n");
    }
    TEST_ASSERT(written == (int)strlen(test_data));
    
    hvfs_close(fd);
    
    hvfs_sync();
    
    fd = hvfs_open(test_file, HVFS_O_RDONLY, test_root_pwid);
    if (fd < 0) {
        serial_puts(SERIAL_COM1, "[FAIL] Failed to open file for read\n");
    }
    TEST_ASSERT(fd >= 0);
    
    memset(read_buf, 0, sizeof(read_buf));
    int bytes_read = hvfs_read(fd, read_buf, sizeof(read_buf) - 1);
    if (bytes_read != (int)strlen(test_data)) {
        serial_puts(SERIAL_COM1, "[FAIL] Read failed: expected ");
        serial_put_dec(SERIAL_COM1, strlen(test_data));
        serial_puts(SERIAL_COM1, ", got ");
        serial_put_dec(SERIAL_COM1, bytes_read);
        serial_puts(SERIAL_COM1, "\n");
    }
    TEST_ASSERT(bytes_read == (int)strlen(test_data));
    
    if (strcmp(read_buf, test_data) != 0) {
        serial_puts(SERIAL_COM1, "[FAIL] Data mismatch: expected '");
        serial_puts(SERIAL_COM1, test_data);
        serial_puts(SERIAL_COM1, "', got '");
        serial_puts(SERIAL_COM1, read_buf);
        serial_puts(SERIAL_COM1, "'\n");
    }
    TEST_ASSERT(strcmp(read_buf, test_data) == 0);
    
    hvfs_close(fd);
    
    hvfs_unlink(test_file, test_root_pwid);
    
    return TEST_PASS;
}

static int test_hvfs_directory_persistence(void) {
    if (ensure_hvfs_initialized() != 0) {
        return TEST_SKIP;
    }
    
    if (!disk_mode_available) {
        return TEST_SKIP;
    }
    
    const char *test_dir = "/test_persist_dir";
    
    int result = hvfs_mkdir(test_dir, test_root_pwid);
    TEST_ASSERT(result == 0);
    
    hvfs_sync();
    
    int fd = hvfs_open(test_dir, HVFS_O_RDONLY, test_root_pwid);
    TEST_ASSERT(fd >= 0);
    
    hvfs_close(fd);
    
    hvfs_rmdir(test_dir, test_root_pwid);
    
    return TEST_PASS;
}

static int test_hvfs_large_file_persistence(void) {
    if (ensure_hvfs_initialized() != 0) {
        return TEST_SKIP;
    }
    
    if (!disk_mode_available) {
        return TEST_SKIP;
    }
    
    const char *test_file = "/test_large.bin";
    char write_buf[512];
    char read_buf[512];
    
    for (int i = 0; i < 512; i++) {
        write_buf[i] = (char)(i & 0xFF);
    }
    
    int fd = hvfs_open(test_file, HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, test_root_pwid);
    TEST_ASSERT(fd >= 0);
    
    int written = hvfs_write(fd, write_buf, 512);
    TEST_ASSERT(written == 512);
    
    hvfs_close(fd);
    
    hvfs_sync();
    
    fd = hvfs_open(test_file, HVFS_O_RDONLY, test_root_pwid);
    TEST_ASSERT(fd >= 0);
    
    memset(read_buf, 0, sizeof(read_buf));
    int bytes_read = hvfs_read(fd, read_buf, 512);
    TEST_ASSERT(bytes_read == 512);
    
    TEST_ASSERT(memcmp(write_buf, read_buf, 512) == 0);
    
    hvfs_close(fd);
    
    hvfs_unlink(test_file, test_root_pwid);
    
    return TEST_PASS;
}

static int test_hvfs_multiple_files_persistence(void) {
    if (ensure_hvfs_initialized() != 0) {
        return TEST_SKIP;
    }
    
    if (!disk_mode_available) {
        return TEST_SKIP;
    }
    
    const char *files[] = {
        "/test_multi_1.txt",
        "/test_multi_2.txt",
        "/test_multi_3.txt"
    };
    const char *contents[] = {
        "FIRST_FILE",
        "SECOND_FILE_DATA",
        "THIRD_FILE_CONTENT_HERE"
    };
    
    for (int i = 0; i < 3; i++) {
        int fd = hvfs_open(files[i], HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, test_root_pwid);
        TEST_ASSERT(fd >= 0);
        hvfs_write(fd, contents[i], strlen(contents[i]));
        hvfs_close(fd);
    }
    
    hvfs_sync();
    
    for (int i = 0; i < 3; i++) {
        char buf[64];
        int fd = hvfs_open(files[i], HVFS_O_RDONLY, test_root_pwid);
        TEST_ASSERT(fd >= 0);
        memset(buf, 0, sizeof(buf));
        int bytes_read = hvfs_read(fd, buf, sizeof(buf) - 1);
        TEST_ASSERT(bytes_read == (int)strlen(contents[i]));
        TEST_ASSERT(strcmp(buf, contents[i]) == 0);
        hvfs_close(fd);
    }
    
    for (int i = 0; i < 3; i++) {
        hvfs_unlink(files[i], test_root_pwid);
    }
    
    return TEST_PASS;
}

static int test_pwid_persistence_save(void) {
    if (ensure_hvfs_initialized() != 0) {
        return TEST_SKIP;
    }
    
    struct pwid_entry *existing_root = pwid_find_by_note("root");
    if (existing_root == NULL) {
        pwid_create_original_root("test_root_password");
    }
    
    int result = pwid_create("test_password_1", "test_user_1", PWID_LEVEL_TRUSTWORTHY);
    TEST_ASSERT(result == 0);
    
    result = pwid_create("test_password_2", "test_user_2", PWID_LEVEL_UNTRUSTWORTHY);
    TEST_ASSERT(result == 0);
    
    TEST_ASSERT(pwid_is_modified() == 1);
    
    return TEST_PASS;
}

static int test_pwid_persistence_load(void) {
    if (ensure_hvfs_initialized() != 0) {
        return TEST_SKIP;
    }
    
    struct pwid_entry *entry1 = pwid_find_by_note("test_user_1");
    TEST_ASSERT_NOT_NULL(entry1);
    TEST_ASSERT(entry1->level == PWID_LEVEL_TRUSTWORTHY);
    TEST_ASSERT(pwid_verify_password(entry1->pwid, "test_password_1") == 1);
    
    struct pwid_entry *entry2 = pwid_find_by_note("test_user_2");
    TEST_ASSERT_NOT_NULL(entry2);
    TEST_ASSERT(entry2->level == PWID_LEVEL_UNTRUSTWORTHY);
    TEST_ASSERT(pwid_verify_password(entry2->pwid, "test_password_2") == 1);
    
    return TEST_PASS;
}

static int test_pwid_original_root_persistence(void) {
    if (ensure_hvfs_initialized() != 0) {
        return TEST_SKIP;
    }
    
    struct pwid_entry *root = pwid_find_by_note("root");
    if (root == NULL) {
        int result = pwid_create_original_root("root_secret_password");
        TEST_ASSERT(result == 0);
        root = pwid_find_by_note("root");
    }
    
    TEST_ASSERT_NOT_NULL(root);
    TEST_ASSERT(root->level == PWID_LEVEL_ROOT);
    TEST_ASSERT(root->flags & PWID_FLAG_ORIGINAL_ROOT);
    
    return TEST_PASS;
}

static int test_hvfs_sync_consistency(void) {
    if (ensure_hvfs_initialized() != 0) {
        return TEST_SKIP;
    }
    
    if (!disk_mode_available) {
        return TEST_SKIP;
    }
    
    const char *test_file = "/test_sync.txt";
    
    int fd = hvfs_open(test_file, HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, test_root_pwid);
    TEST_ASSERT(fd >= 0);
    
    const char *data1 = "INITIAL_DATA";
    hvfs_write(fd, data1, strlen(data1));
    hvfs_close(fd);
    
    hvfs_sync();
    
    fd = hvfs_open(test_file, HVFS_O_RDONLY, test_root_pwid);
    char buf[64];
    memset(buf, 0, sizeof(buf));
    hvfs_read(fd, buf, sizeof(buf) - 1);
    hvfs_close(fd);
    TEST_ASSERT(strcmp(buf, data1) == 0);
    
    fd = hvfs_open(test_file, HVFS_O_WRONLY, test_root_pwid);
    const char *data2 = "MODIFIED_DATA_AFTER_SYNC";
    hvfs_write(fd, data2, strlen(data2));
    hvfs_close(fd);
    
    hvfs_sync();
    
    fd = hvfs_open(test_file, HVFS_O_RDONLY, test_root_pwid);
    memset(buf, 0, sizeof(buf));
    hvfs_read(fd, buf, sizeof(buf) - 1);
    hvfs_close(fd);
    TEST_ASSERT(strcmp(buf, data2) == 0);
    
    hvfs_unlink(test_file, test_root_pwid);
    
    return TEST_PASS;
}

void test_persistence_register(void) {
    test_persistence_module = test_register_module("Persistence");
    
    test_register_case(test_persistence_module, "PWID save to disk", test_pwid_persistence_save);
    test_register_case(test_persistence_module, "PWID load from disk", test_pwid_persistence_load);
    test_register_case(test_persistence_module, "PWID original root persistence", test_pwid_original_root_persistence);
    test_register_case(test_persistence_module, "HvFS file persistence", test_hvfs_file_persistence);
    test_register_case(test_persistence_module, "HvFS directory persistence", test_hvfs_directory_persistence);
    test_register_case(test_persistence_module, "HvFS large file persistence", test_hvfs_large_file_persistence);
    test_register_case(test_persistence_module, "HvFS multiple files persistence", test_hvfs_multiple_files_persistence);
    test_register_case(test_persistence_module, "HvFS sync consistency", test_hvfs_sync_consistency);
}
