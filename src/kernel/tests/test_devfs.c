#include "kernel_test.h"
#include "klog.h"
#include "string.h"

extern void devfs_init(void);
extern int  devfs_mount(const char *path);
extern int  devfs_open(const char *path);
extern int  devfs_read(int dev_type, unsigned char *buf, unsigned int count);
extern int  devfs_write(int dev_type, const unsigned char *buf, unsigned int count);
extern unsigned int devfs_device_count(void);

static int test_devfs_init_and_mount(void) {
    klog_kern("[DevFS] Testing initialization and mount...");

    devfs_init();

    int result = devfs_mount("/dev");
    TEST_ASSERT_EQ(result, 0);

    klog_kern("[DevFS] Mount successful");
    return TEST_PASS;
}

static int test_devfs_device_count(void) {
    klog_kern("[DevFS] Testing device count...");

    unsigned int count = devfs_device_count();
    
    klog_kern("[DevFS] Device count: %d (expected: 4)", count);
    TEST_ASSERT_EQ(count, 4);

    return TEST_PASS;
}

static int test_devfs_open_null_device(void) {
    klog_kern("[DevFS] Testing /dev/null open...");

    int result = devfs_open("/dev/null");

    klog_kern("[DevFS] Open /dev/null returned: %d (expected: >=0)", result);
    TEST_ASSERT_GE(result, 0);

    return TEST_PASS;
}

static int test_devfs_open_zero_device(void) {
    klog_kern("[DevFS] Testing /dev/zero open...");

    int result = devfs_open("/dev/zero");

    klog_kern("[DevFS] Open /dev/zero returned: %d (expected: >=0)", result);
    TEST_ASSERT_GE(result, 0);

    return TEST_PASS;
}

static int test_devfs_open_console_device(void) {
    klog_kern("[DevFS] Testing /dev/console open...");

    int result = devfs_open("/dev/console");

    klog_kern("[DevFS] Open /dev/console returned: %d (expected: >=0)", result);
    TEST_ASSERT_GE(result, 0);

    return TEST_PASS;
}

static int test_devfs_open_tty_device(void) {
    klog_kern("[DevFS] Testing /dev/tty open...");

    int result = devfs_open("/dev/tty");

    klog_kern("[DevFS] Open /dev/tty returned: %d (expected: >=0)", result);
    TEST_ASSERT_GE(result, 0);

    return TEST_PASS;
}

static int test_devfs_open_nonexistent_device(void) {
    klog_kern("[DevFS] Testing nonexistent device open...");

    int result = devfs_open("/dev/nonexistent");

    klog_kern("[DevFS] Open nonexistent returned: %d (expected: -1)", result);
    TEST_ASSERT_EQ(result, -1);

    return TEST_PASS;
}

static int test_devfs_read_null_device(void) {
    klog_kern("[DevFS] Testing /dev/null read (should return 0 bytes)...");

    int dev_type = devfs_open("/dev/null");
    if (dev_type < 0) {
        klog_kern("[DevFS] Failed to open /dev/null");
        return TEST_SKIP;
    }

    unsigned char buf[64];
    int bytes_read = devfs_read(dev_type, buf, sizeof(buf));

    klog_kern("[DevFS] Read from /dev/null: %d bytes (expected: 0)", bytes_read);
    TEST_ASSERT_EQ(bytes_read, 0);

    return TEST_PASS;
}

static int test_devfs_read_zero_device(void) {
    klog_kern("[DevFS] Testing /dev/zero read (should return zeros)...");

    int dev_type = devfs_open("/dev/zero");
    if (dev_type < 0) {
        klog_kern("[DevFS] Failed to open /dev/zero");
        return TEST_SKIP;
    }

    unsigned char buf[32];
    int bytes_read = devfs_read(dev_type, buf, sizeof(buf));

    klog_kern("[DevFS] Read from /dev/zero: %d bytes", bytes_read);
    TEST_ASSERT_GT(bytes_read, 0);

    for (int i = 0; i < bytes_read; i++) {
        if (buf[i] != 0) {
            klog_kern("[DevFS] ERROR: byte[%d] = %d (expected 0)", i, buf[i]);
            return TEST_FAIL;
        }
    }

    klog_kern("[DevFS] All bytes are zero as expected");
    return TEST_PASS;
}

static int test_devfs_write_console_device(void) {
    klog_kern("[DevFS] Testing /dev/console write...");

    int dev_type = devfs_open("/dev/console");
    if (dev_type < 0) {
        klog_kern("[DevFS] Failed to open /dev/console");
        return TEST_SKIP;
    }

    const char *test_msg = "DevFS write test";
    int bytes_written = devfs_write(dev_type, (const unsigned char *)test_msg, 
                                     (unsigned int)(strlen(test_msg)));

    klog_kern("[DevFS] Written to console: %d bytes (expected: %d)", 
              bytes_written, (int)strlen(test_msg));
    TEST_ASSERT_EQ(bytes_written, (int)strlen(test_msg));

    return TEST_PASS;
}

static int test_devfs_multiple_opens(void) {
    klog_kern("[DevFS] Testing multiple opens of same device...");

    int fd1 = devfs_open("/dev/null");
    int fd2 = devfs_open("/dev/null");
    int fd3 = devfs_open("/dev/zero");

    klog_kern("[DevFS] Opens: fd1=%d, fd2=%d, fd3=%d", fd1, fd2, fd3);

    TEST_ASSERT_GE(fd1, 0);
    TEST_ASSERT_GE(fd2, 0);
    TEST_ASSERT_GE(fd3, 0);

    TEST_ASSERT_NE(fd1, fd3);

    return TEST_PASS;
}

void test_devfs_register(void) {
    int mod = test_register_module("DevFS (Device Filesystem)");
    if (mod < 0) return;

    test_register_case(mod, "Init and Mount", test_devfs_init_and_mount);
    test_register_case(mod, "Device Count", test_devfs_device_count);
    test_register_case(mod, "Open /dev/null", test_devfs_open_null_device);
    test_register_case(mod, "Open /dev/zero", test_devfs_open_zero_device);
    test_register_case(mod, "Open /dev/console", test_devfs_open_console_device);
    test_register_case(mod, "Open /dev/tty", test_devfs_open_tty_device);
    test_register_case(mod, "Open Nonexistent", test_devfs_open_nonexistent_device);
    test_register_case(mod, "Read /dev/null", test_devfs_read_null_device);
    test_register_case(mod, "Read /dev/zero", test_devfs_read_zero_device);
    test_register_case(mod, "Write /dev/console", test_devfs_write_console_device);
    test_register_case(mod, "Multiple Opens", test_devfs_multiple_opens);

    klog_kern("[DevFS] Registered 11 test cases");
}
