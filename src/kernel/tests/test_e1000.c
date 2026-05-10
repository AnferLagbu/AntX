/**
 * @file test_e1000.c
 * @brief E1000 网卡测试
 *
 * 测试 E1000 网卡驱动的核心功能。
 *
 * 文档参考: test-framework.md §5 Phase 3
 */

#include "kernel_test.h"
#include "klog.h"
#include "string.h"
#include "e1000.h"

static int test_e1000_probe(void) {
    klog_kern("[E1000] Testing NIC probe...");

    int result = e1000_probe();

    klog_kern("[E1000] Probe result: %d (0=success)", result);
    
    if (result == 0) {
        return TEST_PASS;
    } else {
        klog_kern("[E1000] NIC not found (may be expected in some QEMU configs)");
        return TEST_SKIP;
    }
}

static int test_e1000_device_structure(void) {
    klog_kern("[E1000] Testing device structure...");

    if (!g_e1000.mmio_base) {
        klog_kern("[E1000] Device not initialized");
        return TEST_SKIP;
    }

    klog_kern("[E1000] Bus:Device:Func = %d:%d:%d", 
              g_e1000.bus, g_e1000.device, g_e1000.func);
    klog_kern("[E1000] IRQ: %d", g_e1000.irq);
    klog_kern("[E1000] MAC: %02X:%02X:%02X:%02X:%02X:%02X",
              g_e1000.mac[0], g_e1000.mac[1], g_e1000.mac[2],
              g_e1000.mac[3], g_e1000.mac[4], g_e1000.mac[5]);

    TEST_ASSERT_GT(g_e1000.irq, 0);

    return TEST_PASS;
}

static int test_e1000_mmio_base(void) {
    klog_kern("[E1000] Testing MMIO base address...");

    if (!g_e1000.mmio_base) {
        return TEST_SKIP;
    }

    klog_kern("[E1000] MMIO base: 0x%lX", (unsigned long)g_e1000.mmio_phys);
    
    TEST_ASSERT_NE((uintptr_t)g_e1000.mmio_base, 0);

    return TEST_PASS;
}

static int test_e1000_statistics_initial(void) {
    klog_kern("[E1000] Testing initial statistics...");

    if (!g_e1000.mmio_base) {
        return TEST_SKIP;
    }

    klog_kern("[E1000] Initial stats:");
    klog_kern("[E1000]   ISR count: %lu", (unsigned long)g_e1000.isr_count);
    klog_kern("[E1000]   RX count:  %lu", (unsigned long)g_e1000.rx_count);
    klog_kern("[E1000]   TX count:  %lu", (unsigned long)g_e1000.tx_count);

    return TEST_PASS;
}

static int test_e1000_dump_stats(void) {
    klog_kern("[E1000] Testing stats dump function...");

    if (!g_e1000.mmio_base) {
        return TEST_SKIP;
    }

    e1000_dump_stats();

    return TEST_PASS;
}

void test_e1000_register(void) {
    int mod = test_register_module("E1000 NIC");
    if (mod < 0) return;

    test_register_case(mod, "Probe", test_e1000_probe);
    test_register_case(mod, "Device Structure", test_e1000_device_structure);
    test_register_case(mod, "MMIO Base", test_e1000_mmio_base);
    test_register_case(mod, "Initial Statistics", test_e1000_statistics_initial);
    test_register_case(mod, "Dump Stats", test_e1000_dump_stats);

    klog_kern("[E1000] Registered 5 test cases");
}
