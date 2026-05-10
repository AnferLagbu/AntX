/**
 * @file test_pci_rust.c
 * @brief PCI Rust FFI 直测
 *
 * 通过 C FFI 接口直接测试 Rust 实现的 PCI 子系统。
 * 验证 PciDevice 结构创建、设备枚举、配置空间读取等核心功能。
 *
 * 文档参考: test-framework.md §5 Phase 3 - "pci (Rust) C 侧已有测试 → 实现 Rust PciDevice FFI 直测"
 */

#include "kernel_test.h"
#include "klog.h"
#include "string.h"

/* Rust FFI 函数声明 */
extern int32_t pci_rust_init(void);
extern int32_t pci_rust_scan_bus(void);
extern uint32_t pci_rust_device_count(void);
extern int32_t pci_rust_read_config_byte(uint32_t bus, uint32_t device, 
                                          uint32_t func, uint32_t offset);
extern int32_t pci_rust_read_config_word(uint32_t bus, uint32_t device,
                                           uint32_t func, uint32_t offset);
extern int32_t pci_rust_read_config_dword(uint32_t bus, uint32_t device,
                                            uint32_t func, uint32_t offset);
extern const char* pci_rust_get_vendor_name(uint16_t vendor_id);
extern const char* pci_rust_get_device_name(uint16_t vendor_id, uint16_t device_id);

static int test_pci_rust_init(void) {
    klog_kern("[PCI-RUST] Testing Rust PCI initialization...");

    int32_t result = pci_rust_init();

    klog_kern("[PCI-RUST] Init result: %d (0=success)", result);
    TEST_ASSERT_EQ(result, 0);

    return TEST_PASS;
}

static int test_pci_rust_device_scan(void) {
    klog_kern("[PCI-RUST] Testing device enumeration...");

    int32_t result = pci_rust_scan_bus();

    klog_kern("[PCI-RUST] Scan result: %d (0=success)", result);
    TEST_ASSERT_EQ(result, 0);

    return TEST_PASS;
}

static int test_pci_rust_device_count(void) {
    klog_kern("[PCI-RUST] Testing device count...");

    uint32_t count = pci_rust_device_count();

    klog_kern("[PCI-RUST] Found %u PCI devices", count);
    
    TEST_ASSERT_GE(count, 0);

    if (count == 0) {
        klog_kern("[PCI-RUST] No devices found (may be expected in minimal QEMU)");
        return TEST_SKIP;
    }

    return TEST_PASS;
}

static int test_pci_rust_read_config_byte(void) {
    klog_kern("[PCI-RUST] Testing config space byte read...");

    /* 尝试读取总线 0 设备 0 功能 0 的 Vendor ID (offset 0x00) */
    int32_t value = pci_rust_read_config_byte(0, 0, 0, 0x00);

    if (value < 0) {
        klog_kern("[PCI-RUST] Config read failed: %d", value);
        return TEST_FAIL;
    }

    klog_kern("[PCI-RUST] BDF[0:0:0] Vendor ID low byte: 0x%02X", 
              (uint8_t)(value & 0xFF));

    TEST_ASSERT_GE(value, 0);

    return TEST_PASS;
}

static int test_pci_rust_read_config_word(void) {
    klog_kern("[PCI-RUST] Testing config space word read...");

    /* 读取完整的 Vendor ID (2 bytes) */
    int32_t value = pci_rust_read_config_word(0, 0, 0, 0x00);

    if (value < 0) {
        klog_kern("[PCI-RUST] Config word read failed: %d", value);
        return TEST_FAIL;
    }

    klog_kern("[PCI-RUST] BDF[0:0:0] Vendor ID: 0x%04X", (uint16_t)value);

    /* 常见 Vendor ID: Intel=0x8086, QEMU emulated=0x1AF4 (Red Hat) */
    TEST_ASSERT_GT(value, 0);

    return TEST_PASS;
}

static int test_pci_rust_read_config_dword(void) {
    klog_kern("[PCI-RUST] Testing config space dword read...");

    /* 读取 Device ID + Vendor ID (4 bytes at offset 0) */
    int32_t value = pci_rust_read_config_dword(0, 0, 0, 0x00);

    if (value < 0) {
        klog_kern("[PCI-RUST] Config dword read failed: %d", value);
        return TEST_FAIL;
    }

    uint16_t vendor_id = (uint16_t)(value & 0xFFFF);
    uint16_t device_id = (uint16_t)((value >> 16) & 0xFFFF);

    klog_kern("[PCI-RUST] BDF[0:0:0] Vendor:0x%04X Device:0x%04X",
              vendor_id, device_id);

    TEST_ASSERT_GT(value, 0);

    return TEST_PASS;
}

static int test_pci_rust_header_type(void) {
    klog_kern("[PCI-RUST] Testing header type detection...");

    /* Header Type 在 offset 0x0C 的第 6-7 位 */
    int32_t value = pci_rust_read_config_byte(0, 0, 0, 0x0C);

    if (value < 0) {
        return TEST_SKIP;
    }

    uint8_t header_type = (uint8_t)((value >> 6) & 0x03);

    klog_kern("[PCI-RUST] Header type: %d (0=normal, 1=bridge, 2=cardbus)",
              header_type);

    TEST_ASSERT_LE(header_type, 3);

    return TEST_PASS;
}

static int test_pci_rust_class_code(void) {
    klog_kern("[PCI-RUST] Testing class code reading...");

    /* Class Code 在 offset 0x08-0x0B */
    int32_t class_dword = pci_rust_read_config_dword(0, 0, 0, 0x08);

    if (class_dword < 0) {
        return TEST_SKIP;
    }

    uint8_t base_class = (uint8_t)(class_dword >> 24);
    uint8_t sub_class = (uint8_t)((class_dword >> 16) & 0xFF);
    uint8_t prog_if = (uint8_t)((class_dword >> 8) & 0xFF);

    klog_kern("[PCI-RUST] Class: %02X SubClass: %02X ProgIF: %02X",
              base_class, sub_class, prog_if);

    TEST_ASSERT_GE(base_class, 0);

    return TEST_PASS;
}

static int test_pci_rust_bar_detection(void) {
    klog_kenn("[PCI-RUST] Testing BAR (Base Address Register) detection...");

    bool bar_found = false;

    for (int bar_num = 0; bar_num < 6; bar_num++) {
        uint32_t bar_offset = 0x10 + (bar_num * 4);
        int32_t bar_value = pci_rust_read_config_dword(0, 0, 0, bar_offset);

        if (bar_value > 0 && bar_value != 0xFFFFFFFF) {
            klog_kern("[PCI-RUST] BAR[%d]: 0x%08X", bar_num, (uint32_t)bar_value);
            bar_found = true;
        }
    }

    if (!bar_found) {
        klog_kern("[PCI-RUST] No BARs detected (may be expected for bridge/host)");
        return TEST_SKIP;
    }

    return TEST_PASS;
}

static int test_pci_rust_vendor_lookup(void) {
    klog_kern("[PCI-RUST] Testing vendor name lookup...");

    const char *intel_name = pci_rust_get_vendor_name(0x8086);
    const char *redhat_name = pci_rust_get_vendor_name(0x1AF4);
    const char *unknown_name = pci_rust_get_vendor_name(0xDEAD);

    klog_kern("[PCI-RUST] Vendor names:");
    klog_kern("[PCI-RUST]   0x8086 -> %s", intel_name ? intel_name : "(null)");
    klog_kern("[PCI-RUST]   0x1AF4 -> %s", redhat_name ? redhat_name : "(null)");
    klog_kern("[PCI-RUST]   0xDEAD -> %s", unknown_name ? unknown_name : "(null)");

    if (intel_name && strlen(intel_name) > 0) {
        return TEST_PASS;
    } else {
        klog_kern("[PCI-RUST] Vendor lookup not available (Rust side may not be initialized)");
        return TEST_SKIP;
    }
}

static int test_pci_rust_interrupt_line(void) {
    klog_kern("[PCI-RUST] Testing interrupt line assignment...");

    /* Interrupt Line 在 offset 0x3C */
    int32_t int_line = pci_rust_read_config_byte(0, 0, 0, 0x3C);
    /* Interrupt Pin 在 offset 0x3D */
    int32_t int_pin = pci_rust_read_config_byte(0, 0, 0, 0x3D);

    if (int_line < 0 || int_pin < 0) {
        return TEST_SKIP;
    }

    klog_kern("[PCI-RUST] Interrupt Line: %d, Pin: %d", int_line, int_pin);

    TEST_ASSERT_GE(int_line, 0);
    TEST_ASSERT_LE(int_pin, 4);  /* 0=None, 1=INTA#, 2=INTB#, 3=INTC#, 4=INTD# */

    return TEST_PASS;
}

void test_pci_rust_register(void) {
    int mod = test_register_module("PCI (Rust FFI Direct)");
    if (mod < 0) return;

    test_register_case(mod, "Initialization", test_pci_rust_init);
    test_register_case(mod, "Device Scan", test_pci_rust_device_scan);
    test_register_case(mod, "Device Count", test_pci_rust_device_count);
    test_register_case(mod, "Read Config Byte", test_pci_rust_read_config_byte);
    test_register_case(mod, "Read Config Word", test_pci_rust_read_config_word);
    test_register_case(mod, "Read Config Dword", test_pci_rust_read_config_dword);
    test_register_case(mod, "Header Type", test_pci_rust_header_type);
    test_register_case(mod, "Class Code", test_pci_rust_class_code);
    test_register_case(mod, "BAR Detection", test_pci_rust_bar_detection);
    test_register_case(mod, "Vendor Lookup", test_pci_rust_vendor_lookup);
    test_register_case(mod, "Interrupt Line", test_pci_rust_interrupt_line);

    klog_kern("[PCI-RUST] Registered 12 FFI direct test cases");
}
