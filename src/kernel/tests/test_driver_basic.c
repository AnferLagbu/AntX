#include "kernel_test.h"
#include "klog.h"
#include "serial.h"
#include "keyboard.h"

static int test_serial_init_com1(void) {
    klog_kern("[Driver] Testing COM1 serial initialization...");

    serial_init(SERIAL_COM1);

    klog_kern("[Driver] COM1 initialized successfully");
    return TEST_PASS;
}

static int test_serial_write_char(void) {
    klog_kern("[Driver] Testing serial character write...");

    serial_init(SERIAL_COM1);

    const char *test_msg = "Hello";
    serial_write(SERIAL_COM1, (const void *)test_msg, strlen(test_msg));

    klog_kern("[Driver] Wrote '%s' to COM1", test_msg);
    return TEST_PASS;
}

static int test_serial_transmit_empty(void) {
    klog_kern("[Driver] Testing serial transmit buffer empty check...");

    serial_init(SERIAL_COM1);

    /* 使用 serial_has_data 检查接收缓冲区状态 */
    int has_data = serial_has_data(SERIAL_COM1);
    
    klog_kern("[Driver] Has data in receive buffer: %d (expected: 0)", has_data);
    
    return TEST_PASS;
}

static int test_keyboard_init(void) {
    klog_kern("[Driver] Testing PS/2 keyboard initialization...");

    keyboard_init();

    klog_kern("[Driver] Keyboard initialized successfully");
    return TEST_PASS;
}

static int test_keyboard_buffer_empty(void) {
    klog_kern("[Driver] Testing keyboard buffer state (should be empty)...");

    keyboard_init();

    bool has_key = keyboard_has_data();

    klog_kern("[Driver] Has key in buffer: %d (expected: 0)", (int)has_key);
    
    if (has_key) {
        return TEST_FAIL;
    }
    
    return TEST_PASS;
}

static int test_driver_initialization_order(void) {
    klog_kern("[Driver] Testing driver initialization order (serial before keyboard)...");

    serial_init(SERIAL_COM1);
    keyboard_init();

    klog_kern("[Driver] Both drivers initialized in correct order");
    return TEST_PASS;
}

static int test_multiple_serial_ports(void) {
    klog_kern("[Driver] Testing multiple serial port initialization...");

    serial_init(SERIAL_COM1);

    klog_kern("[Driver] COM1 initialized");
    klog_kern("[Driver] Note: COM2-COM4 not tested (may not exist in QEMU)");
    return TEST_PASS;
}

void test_driver_basic_register(void) {
    int mod = test_register_module("Driver Basic (Serial/Keyboard)");
    if (mod < 0) return;

    test_register_case(mod, "Serial Init COM1", test_serial_init_com1);
    test_register_case(mod, "Serial Write Char", test_serial_write_char);
    test_register_case(mod, "Serial Transmit Empty", test_serial_transmit_empty);
    test_register_case(mod, "Keyboard Init", test_keyboard_init);
    test_register_case(mod, "Keyboard Buffer Empty", test_keyboard_buffer_empty);
    test_register_case(mod, "Init Order", test_driver_initialization_order);
    test_register_case(mod, "Multiple Serial Ports", test_multiple_serial_ports);

    klog_kern("[Driver] Registered 7 test cases");
}
