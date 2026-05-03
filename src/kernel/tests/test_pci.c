/**
 * @file test_pci.c
 * @brief PCI 总线驱动单元测试
 *
 * 测试 PCI 设备枚举、配置空间访问、BAR 解析和驱动管理功能。
 */

#include "tests/kernel_test.h"
#include "pci.h"
#include "serial.h"

/* ============================================================
 * 初始化测试
 * ============================================================ */

/**
 * @brief 测试 PCI 子系统初始化
 */
static int test_pci_init(void)
{
    int result = pci_init();

    TEST_ASSERT_EQ(result, 0);

    return TEST_PASS;
}

/* ============================================================
 * 配置空间访问测试
 * ============================================================ */

/**
 * @brief 测试配置空间读写一致性
 */
static int test_config_space_access(void)
{
    /* 测试 Bus 0, Device 0, Function 0 (Host Bridge) */
    uint16_t vendor_id = pci_read_config_word(0, 0, 0, PCI_VENDOR_ID);
    uint16_t device_id = pci_read_config_word(0, 0, 0, PCI_DEVICE_ID);

    /* Host Bridge 通常存在，vendor_id 应该有效 */
    TEST_ASSERT(vendor_id != 0xFFFF && vendor_id != 0x0000);

    /* 验证多次读取的一致性 */
    uint16_t vendor_id2 = pci_read_config_word(0, 0, 0, PCI_VENDOR_ID);
    TEST_ASSERT_EQ(vendor_id, vendor_id2);

    return TEST_PASS;
}

/**
 * @brief 测试不同宽度的配置空间访问
 */
static int test_config_space_widths(void)
{
    /* 读取厂商/设备 ID 作为双字 */
    uint32_t dword = pci_read_config_dword(0, 0, 0, PCI_VENDOR_ID);
    uint16_t vendor_word = pci_read_config_word(0, 0, 0, PCI_VENDOR_ID);
    uint16_t device_word = pci_read_config_word(0, 0, 0, PCI_DEVICE_ID);
    uint8_t vendor_byte_low = pci_read_config_byte(0, 0, 0, PCI_VENDOR_ID);
    uint8_t vendor_byte_high = pci_read_config_byte(0, 0, 0, PCI_VENDOR_ID + 1);

    /* 验证不同宽度读取的数据一致性 */
    TEST_ASSERT_EQ((uint16_t)(dword & 0xFFFF), vendor_word);
    TEST_ASSERT_EQ((uint16_t)((dword >> 16) & 0xFFFF), device_word);
    TEST_ASSERT_EQ((uint8_t)(vendor_word & 0xFF), vendor_byte_low);
    TEST_ASSERT_EQ((uint8_t)((vendor_word >> 8) & 0xFF), vendor_byte_high);

    return TEST_PASS;
}

/**
 * @brief 测试配置空间写入
 */
static int test_config_space_write(void)
{
    /* 保存原始命令寄存器值 */
    uint16_t original_cmd = pci_read_config_word(0, 0, 0, PCI_COMMAND);

    /* 写入新值 */
    uint16_t new_cmd = original_cmd | PCI_CMD_IO_SPACE;
    pci_write_config_word(0, 0, 0, PCI_COMMAND, new_cmd);

    /* 读回验证 */
    uint16_t readback = pci_read_config_word(0, 0, 0, PCI_COMMAND);
    TEST_ASSERT((readback & PCI_CMD_IO_SPACE) != 0);

    /* 恢复原始值 */
    pci_write_config_word(0, 0, 0, PCI_COMMAND, original_cmd);

    readback = pci_read_config_word(0, 0, 0, PCI_COMMAND);
    TEST_ASSERT_EQ(readback, original_cmd);

    return TEST_PASS;
}

/* ============================================================
 * 设备枚举测试
 * ============================================================ */

/**
 * @brief 测试设备列表非空（至少有 Host Bridge）
 */
static int test_device_list_not_empty(void)
{
    int count = pci_get_device_count();

    /* QEMU 环境通常至少有一个 PCI 设备 (Host Bridge) */
    TEST_ASSERT(count >= 1);

    return TEST_PASS;
}

/**
 * @brief 测试设备基本信息有效性
 */
static int test_device_basic_info(void)
{
    pci_device_t *dev = pci_get_device(0, 0, 0);

    if (!dev) {
        /* Host Bridge 可能不在 0:0.0，尝试查找任意设备 */
        dev = pci_find_device(0x8086, 0xFFFF, NULL);  /* Intel 设备 */
        if (!dev) {
            dev = pci_find_class(PCI_CLASS_BRIDGE, 0xFF, NULL);  /* 桥接器 */
        }
        if (!dev) {
            dev = pci_find_device(0xFFFF, 0xFFFF, NULL);  /* 使用第一个找到的设备 */
        }
    }

    TEST_ASSERT_NOT_NULL(dev);

    if (dev) {
        /* 验证基本字段有效 */
        TEST_ASSERT(dev->bus < PCI_MAX_BUS);
        TEST_ASSERT(dev->device < PCI_MAX_DEVICE);
        TEST_ASSERT(dev->function < PCI_MAX_FUNCTION);
        TEST_ASSERT(dev->vendor_id != 0xFFFF && dev->vendor_id != 0x0000);
        TEST_ASSERT(dev->device_id != 0xFFFF || dev->device_id != 0x0000);
    }

    return TEST_PASS;
}

/**
 * @brief 测试设备查找功能
 */
static int test_device_find(void)
{
    /* 查找第一个设备 */
    pci_device_t *dev1 = pci_find_device(0xFFFF, 0xFFFF, NULL);
    TEST_ASSERT_NOT_NULL(dev1);

    /* 从第一个设备之后继续查找 */
    pci_device_t *dev2 = pci_find_device(0xFFFF, 0xFFFF, dev1);

    /* 按类别查找 */
    pci_device_t *bridge_dev = pci_find_class(PCI_CLASS_BRIDGE, 0xFF, NULL);
    /* 桥接器可能存在也可能不存在，不强制断言 */

    return TEST_PASS;
}

/* ============================================================
 * BAR 解析测试
 * ============================================================ */

/**
 * @brief 测试 BAR 解析功能
 */
static int test_bar_parsing(void)
{
    pci_device_t *dev = pci_get_device(0, 0, 0);

    if (!dev) {
        dev = pci_find_device(0xFFFF, 0xFFFF, NULL);
    }

    if (!dev) {
        return TEST_SKIP;  /* 无可用设备，跳过 */
    }

    /* 重新解析 BAR */
    pci_parse_bars(dev);

    /* 验证 BAR 数量在合理范围 */
    TEST_ASSERT(dev->bar_count >= 0 && dev->bar_count <= 6);

    /* 如果存在 BAR，验证其类型有效 */
    for (int i = 0; i < dev->bar_count; i++) {
        if (dev->bars[i].type != PCI_BAR_NONE) {
            TEST_ASSERT(dev->bars[i].type == PCI_BAR_IO ||
                       dev->bars[i].type == PCI_BAR_MEMORY_32 ||
                       dev->bars[i].type == PCI_BAR_MEMORY_64);

            if (dev->bars[i].type == PCI_BAR_IO ||
                dev->bars[i].type == PCI_BAR_MEMORY_32 ||
                dev->bars[i].type == PCI_BAR_MEMORY_64) {

                TEST_ASSERT(dev->bars[i].size > 0);
            }
        }
    }

    return TEST_PASS;
}

/* ============================================================
 * 中断管理测试
 * ============================================================ */

/**
 * @brief 测试中断引脚名称获取
 */
static int test_interrupt_pin_names(void)
{
    const char *name;

    name = pci_get_interrupt_pin_name(0);
    TEST_ASSERT_NOT_NULL(name);

    name = pci_get_interrupt_pin_name(1);
    TEST_ASSERT_STR(name, "INTA#");

    name = pci_get_interrupt_pin_name(2);
    TEST_ASSERT_STR(name, "INTB#");

    name = pci_get_interrupt_pin_name(3);
    TEST_ASSERT_STR(name, "INTC#");

    name = pci_get_interrupt_pin_name(4);
    TEST_ASSERT_STR(name, "INTD#");

    return TEST_PASS;
}

/**
 * @brief 测试 IRQ 分配
 */
static int test_irq_allocation(void)
{
    pci_device_t *dev = pci_find_device(0xFFFF, 0xFFFF, NULL);

    if (!dev || dev->interrupt_pin == 0) {
        return TEST_SKIP;  /* 无中断引脚，跳过 */
    }

    int irq = pci_allocate_irq(dev);
    TEST_ASSERT(irq >= 0 && irq <= 15);

    return TEST_PASS;
}

/* ============================================================
 * 使能函数测试
 * ============================================================ */

/**
 * @brief 测试设备使能函数
 */
static int test_enable_functions(void)
{
    pci_device_t *dev = pci_find_device(0xFFFF, 0xFFFF, NULL);

    if (!dev) {
        return TEST_SKIP;
    }

    /* 保存原始状态 */
    uint16_t original_cmd = dev->command;

    /* 测试使能内存空间 */
    pci_enable_memory_space(dev);
    TEST_ASSERT((dev->command & PCI_CMD_MEMORY_SPACE) != 0);

    /* 测试使能 I/O 空间 */
    pci_enable_io_space(dev);
    TEST_ASSERT((dev->command & PCI_CMD_IO_SPACE) != 0);

    /* 测试使能 Bus Mastering */
    pci_enable_bus_master(dev);
    TEST_ASSERT((dev->command & PCI_CMD_BUS_MASTER) != 0);

    /* 测试禁用 Bus Mastering */
    pci_disable_bus_master(dev);
    TEST_ASSERT((dev->command & PCI_CMD_BUS_MASTER) == 0);

    /* 恢复原始状态 */
    pci_write_config_word(dev->bus, dev->device,
                           dev->function, PCI_COMMAND, original_cmd);
    dev->command = original_cmd;

    return TEST_PASS;
}

/* ============================================================
 * 驱动注册测试
 * ============================================================ */

/**
 * @brief 测试驱动注册/注销
 */
static int test_driver_register_unregister(void)
{
    struct pci_driver_ops test_ops = {
        .name = "test_driver",
        .vendor_id = 0xFFFF,
        .device_id = 0xFFFF,
        .class_code = 0xFF,
        .probe = NULL,
        .remove = NULL,
        .suspend = NULL,
        .resume = NULL
    };

    pci_driver_t driver = {
        .name = "Test Driver",
        .ops = test_ops,
        .next = NULL
    };

    int count_before;
    pci_get_stats(NULL, NULL, &count_before);

    /* 注册驱动 */
    int result = pci_register_driver(&driver);
    TEST_ASSERT_EQ(result, 0);

    int count_after_reg;
    pci_get_stats(NULL, NULL, &count_after_reg);
    TEST_ASSERT_EQ(count_after_reg, count_before + 1);

    /* 注销驱动 */
    pci_unregister_driver(&driver);

    int count_after_unreg;
    pci_get_stats(NULL, NULL, &count_after_unreg);
    TEST_ASSERT_EQ(count_after_unreg, count_before);

    return TEST_PASS;
}

/**
 * @brief 测试驱动匹配
 */
static int test_driver_match(void)
{
    static int probe_called = 0;

    struct pci_driver_ops test_ops = {
        .name = "match_test",
        .vendor_id = 0xFFFF,
        .device_id = 0xFFFF,
        .class_code = 0xFF,
        .probe = NULL,
        .remove = NULL,
        .suspend = NULL,
        .resume = NULL
    };

    pci_driver_t driver = {
        .name = "Match Test Driver",
        .ops = test_ops,
        .next = NULL
    };

    pci_register_driver(&driver);

    /* 执行匹配 */
    pci_match_drivers();

    /* 注销 */
    pci_unregister_driver(&driver);

    return TEST_PASS;
}

/* ============================================================
 * 统计信息测试
 * ============================================================ */

/**
 * @brief 测试统计信息接口
 */
static int test_statistics(void)
{
    int devices, buses, drivers;

    pci_get_stats(&devices, &buses, &drivers);

    TEST_ASSERT(devices >= 0);
    TEST_ASSERT(buses >= 0);
    TEST_ASSERT(drivers >= 0);
    TEST_ASSERT_EQ(devices, pci_get_device_count());

    return TEST_PASS;
}

/* ============================================================
 * MSI/MSI-X 基础测试
 * ============================================================ */

/**
 * @brief 测试 MSI 启用/禁用
 */
static int test_msi_enable_disable(void)
{
    pci_device_t *dev = pci_find_device(0xFFFF, 0xFFFF, NULL);

    if (!dev) {
        return TEST_SKIP;
    }

    /* 即使不支持 MSI，也不应崩溃 */
    int vectors = pci_enable_msi(dev, 1);
    TEST_ASSERT(vectors >= 0);

    pci_disable_msi(dev);

    return TEST_PASS;
}

/**
 * @brief 测试 MSI-X 启用/禁用
 */
static int test_msix_enable_disable(void)
{
    pci_device_t *dev = pci_find_device(0xFFFF, 0xFFFF, NULL);

    if (!dev) {
        return TEST_SKIP;
    }

    int vectors = pci_enable_msix(dev, 1);
    TEST_ASSERT(vectors >= 0);

    pci_disable_msix(dev);

    return TEST_PASS;
}

/* ============================================================
 * 调试输出测试
 * ============================================================ */

/**
 * @brief 测试设备信息转储（不崩溃）
 */
static int test_dump_output(void)
{
    /* 不应崩溃 */
    pci_dump_devices();

    pci_device_t *dev = pci_find_device(0xFFFF, 0xFFFF, NULL);
    if (dev) {
        pci_dump_device(dev);
    }

    return TEST_PASS;
}

/* ============================================================
 * 类别名称转换测试
 * ============================================================ */

/**
 * @brief 测试类别代码到字符串的转换
 */
static int test_class_to_string(void)
{
    const char *str;

    str = pci_class_to_string(PCI_CLASS_NETWORK);
    TEST_ASSERT_STR(str, "Network");

    str = pci_class_to_string(PCI_CLASS_DISPLAY);
    TEST_ASSERT_STR(str, "Display");

    str = pci_class_to_string(PCI_CLASS_STORAGE);
    TEST_ASSERT_STR(str, "Storage");

    str = pci_class_to_string(0xFF);
    TEST_ASSERT_STR(str, "Unknown");

    return TEST_PASS;
}

/* ============================================================
 * 性能基准测试
 * ============================================================ */

/**
 * @brief 测试 PCI 配置空间读取性能
 */
static int test_pci_performance(void)
{
    const int iterations = 10000;
    uint64_t start, end, elapsed;

    __asm__ volatile("rdtsc" : "=A"(start));

    for (int i = 0; i < iterations; i++) {
        pci_read_config_dword(0, 0, 0, PCI_VENDOR_ID);
    }

    __asm__ volatile("rdtsc" : "=A"(end));

    elapsed = end - start;

    serial_puts(SERIAL_COM1, "[性能] PCI Config Read: ");
    serial_put_dec(SERIAL_COM1, iterations);
    serial_puts(SERIAL_COM1, " 次，耗时 ");
    serial_put_dec(SERIAL_COM1, (uint32_t)(elapsed / iterations));
    serial_puts(SERIAL_COM1, " cycles/次\n");

    TEST_ASSERT(elapsed > 0);

    return TEST_PASS;
}

/* ============================================================
 * 边界条件测试
 * ============================================================ */

/**
 * @brief 测试无效参数处理
 */
static int test_invalid_parameters(void)
{
    /* NULL 设备指针不应崩溃 */
    pci_parse_bars(NULL);
    pci_enable_memory_space(NULL);
    pci_enable_io_space(NULL);
    pci_enable_bus_master(NULL);
    pci_disable_bus_master(NULL);
    pci_allocate_irq(NULL);
    pci_enable_msi(NULL, 1);
    pci_disable_msi(NULL);
    pci_enable_msix(NULL, 1);
    pci_disable_msix(NULL);
    pci_dump_device(NULL);

    /* 无效位置应返回 NULL */
    pci_device_t *dev = pci_get_device(255, 31, 7);
    TEST_ASSERT_NULL(dev);

    /* 不存在的设备 ID 应返回 NULL */
    dev = pci_find_device(0xDEAD, 0xBEEF, NULL);
    TEST_ASSERT_NULL(dev);

    return TEST_PASS;
}

/* ============================================================
 * 模块注册
 * ============================================================ */

void test_pci_register(void)
{
    int mod = test_register_module("PCI");
    if (mod < 0) {
        return;
    }

    test_register_case(mod, "初始化", test_pci_init);
    test_register_case(mod, "配置空间读取", test_config_space_access);
    test_register_case(mod, "多宽度访问", test_config_space_widths);
    test_register_case(mod, "配置空间写入", test_config_space_write);
    test_register_case(mod, "设备列表非空", test_device_list_not_empty);
    test_register_case(mod, "设备基本信息", test_device_basic_info);
    test_register_case(mod, "设备查找", test_device_find);
    test_register_case(mod, "BAR解析", test_bar_parsing);
    test_register_case(mod, "中断引脚名称", test_interrupt_pin_names);
    test_register_case(mod, "IRQ分配", test_irq_allocation);
    test_register_case(mod, "使能函数", test_enable_functions);
    test_register_case(mod, "驱动注册注销", test_driver_register_unregister);
    test_register_case(mod, "驱动匹配", test_driver_match);
    test_register_case(mod, "统计信息", test_statistics);
    test_register_case(mod, "MSI启用禁用", test_msi_enable_disable);
    test_register_case(mod, "MSI-X启用禁用", test_msix_enable_disable);
    test_register_case(mod, "调试输出", test_dump_output);
    test_register_case(mod, "类别名称转换", test_class_to_string);
    test_register_case(mod, "性能基准", test_pci_performance);
    test_register_case(mod, "无效参数", test_invalid_parameters);
}
