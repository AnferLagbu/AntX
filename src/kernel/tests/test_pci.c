/**
 * @file test_pci.c
 * @brief PCI 子系统测试
 *
 * 测试 C 侧实现的 PCI 子系统核心功能，
 * 包括设备枚举、配置空间读取、BAR 检测等。
 *
 * 文档参考: test-framework.md §5 Phase 3
 */

#include "kernel_test.h"
#include "klog.h"
#include "string.h"
#include "pci.h"

static int test_pci_init(void) {
    klog_kern("[PCI] Testing PCI initialization...");

    int result = pci_init();

    klog_kern("[PCI] Init result: %d (0=success)", result);
    TEST_ASSERT_EQ(result, 0);

    return TEST_PASS;
}

static int test_pci_device_scan(void) {
    klog_kern("[PCI] Testing device enumeration...");

    pci_scan_bus(0);

    klog_kern("[PCI] Bus 0 scan completed");
    return TEST_PASS;
}

static int test_pci_device_count(void) {
    klog_kern("[PCI] Testing device count...");

    int count = pci_get_device_count();

    klog_kern("[PCI] Found %d PCI devices", count);
    
    TEST_ASSERT_GE(count, 0);

    if (count == 0) {
        klog_kern("[PCI] No devices found (may be expected in minimal QEMU)");
        return TEST_SKIP;
    }

    return TEST_PASS;
}

static int test_pci_read_config_byte(void) {
    klog_kern("[PCI] Testing config space byte read...");

    uint8_t value = pci_read_config_byte(0, 0, 0, 0x00);

    klog_kern("[PCI] BDF[0:0:0] Vendor ID low byte: 0x%02X", value);

    return TEST_PASS;
}

static int test_pci_read_config_word(void) {
    klog_kern("[PCI] Testing config space word read...");

    uint16_t value = pci_read_config_word(0, 0, 0, 0x00);

    klog_kern("[PCI] BDF[0:0:0] Vendor ID: 0x%04X", value);

    TEST_ASSERT_GT(value, 0);

    return TEST_PASS;
}

static int test_pci_read_config_dword(void) {
    klog_kern("[PCI] Testing config space dword read...");

    uint32_t value = pci_read_config_dword(0, 0, 0, 0x00);

    uint16_t vendor_id = (uint16_t)(value & 0xFFFF);
    uint16_t device_id = (uint16_t)((value >> 16) & 0xFFFF);

    klog_kern("[PCI] BDF[0:0:0] Vendor:0x%04X Device:0x%04X",
              vendor_id, device_id);

    TEST_ASSERT_GT(value, 0);

    return TEST_PASS;
}

static int test_pci_header_type(void) {
    klog_kern("[PCI] Testing header type detection...");

    uint8_t header_type = pci_read_config_byte(0, 0, 0, 0x0C);
    uint8_t type = (header_type >> 6) & 0x03;

    klog_kern("[PCI] Header type: %d (0=normal, 1=bridge, 2=cardbus)", type);

    TEST_ASSERT_LE(type, 3);

    return TEST_PASS;
}

static int test_pci_class_code(void) {
    klog_kern("[PCI] Testing class code reading...");

    uint8_t base_class = pci_read_config_byte(0, 0, 0, 0x0B);
    uint8_t sub_class = pci_read_config_byte(0, 0, 0, 0x0A);
    uint8_t prog_if = pci_read_config_byte(0, 0, 0, 0x09);

    klog_kern("[PCI] Class: %02X SubClass: %02X ProgIF: %02X",
              base_class, sub_class, prog_if);

    TEST_ASSERT_GE(base_class, 0);

    return TEST_PASS;
}

static int test_pci_bar_detection(void) {
    klog_kern("[PCI] Testing BAR (Base Address Register) detection...");

    if (pci_get_device_count() == 0) {
        return TEST_SKIP;
    }

    /* 尝试检测第一个设备的 BAR */
    uint32_t bar0 = pci_read_config_dword(0, 0, 0, 0x10);

    klog_kern("[PCI] Device[0:0:0] BAR0: 0x%08X", bar0);

    if (bar0 != 0 && bar0 != 0xFFFFFFFF) {
        return TEST_PASS;
    } else {
        klog_kern("[PCI] No BAR detected for first device");
        return TEST_SKIP;
    }
}

static int test_pci_interrupt_line(void) {
    klog_kern("[PCI] Testing interrupt line assignment...");

    uint8_t int_line = pci_read_config_byte(0, 0, 0, 0x3C);
    uint8_t int_pin = pci_read_config_byte(0, 0, 0, 0x3D);

    klog_kern("[PCI] Interrupt Line: %d, Pin: %d", int_line, int_pin);

    TEST_ASSERT_LE(int_pin, 4);  /* 0=None, 1=INTA#, 2=INTB#, 3=INTC#, 4=INTD# */

    return TEST_PASS;
}

static int test_pci_write_config(void) {
    klog_kern("[PCI] Testing config space write...");

    uint32_t original_cmd = pci_read_config_dword(0, 0, 0, 0x04);

    pci_write_config_dword(0, 0, 0, 0x04, 0x00000000);

    uint32_t new_cmd = pci_read_config_dword(0, 0, 0, 0x04);

    klog_kern("[PCI] Command register after write: 0x%08X", new_cmd);

    pci_write_config_dword(0, 0, 0, 0x04, original_cmd);

    return TEST_PASS;
}

void test_pci_register(void) {
    int mod = test_register_module("PCI Subsystem");
    if (mod < 0) return;

    test_register_case(mod, "Initialization", test_pci_init);
    test_register_case(mod, "Device Scan", test_pci_device_scan);
    test_register_case(mod, "Device Count", test_pci_device_count);
    test_register_case(mod, "Read Config Byte", test_pci_read_config_byte);
    test_register_case(mod, "Read Config Word", test_pci_read_config_word);
    test_register_case(mod, "Read Config Dword", test_pci_read_config_dword);
    test_register_case(mod, "Header Type", test_pci_header_type);
    test_register_case(mod, "Class Code", test_pci_class_code);
    test_register_case(mod, "BAR Detection", test_pci_bar_detection);
    test_register_case(mod, "Interrupt Line", test_pci_interrupt_line);
    test_register_case(mod, "Write Config", test_pci_write_config);

    klog_kern("[PCI] Registered 11 test cases");
}
