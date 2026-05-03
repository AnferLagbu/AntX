/**
 * @file pci.c
 * @brief PCI 总线驱动实现
 *
 * 实现 x86_64 架构下的 PCI 总线驱动，包括设备枚举、配置空间访问、
 * BAR 解析、中断路由和驱动管理等功能。
 */

#include "pci.h"
#include "io.h"
#include "serial.h"
#include "string.h"
#include "kmalloc.h"

/* ============================================================
 * 全局变量
 * ============================================================ */

static pci_device_t *device_list = NULL;
static int device_count = 0;
static int bus_count = 0;

static pci_driver_t *driver_list = NULL;
static int driver_count = 0;

static spinlock_t pci_lock = SPINLOCK_INIT(pci_lock);

static int pci_initialized = 0;

/* ============================================================
 * 内部辅助函数 - 配置空间访问 (Type 1 Configuration Cycle)
 * ============================================================ */

/**
 * @brief 生成 PCI 配置地址
 *
 * 使用 x86 的 Type 1 配置周期机制。
 *
 * Format: [31:Enable] [30-24:Reserved] [23-16:Bus]
 *         [15-11:Device] [10-8:Function] [7-2:Register Offset] [1-0:00]
 */
static uint32_t pci_make_config_addr(uint8_t bus, uint8_t device,
                                      uint8_t function, uint8_t offset)
{
    return (uint32_t)(0x80000000 |
           ((uint32_t)bus << 16) |
           ((uint32_t)(device & 0x1F) << 11) |
           ((uint32_t)(function & 0x07) << 8) |
           ((uint32_t)(offset & 0xFC)));
}

/* ============================================================
 * 公共接口 - 配置空间访问实现
 * ============================================================ */

uint8_t pci_read_config_byte(uint8_t bus, uint8_t device,
                              uint8_t function, uint8_t offset)
{
    outl(PCI_CONFIG_ADDR_PORT,
          pci_make_config_addr(bus, device, function, offset));
    return (uint8_t)((inl(PCI_CONFIG_DATA_PORT) >> ((offset & 3) * 8)) & 0xFF);
}

uint16_t pci_read_config_word(uint8_t bus, uint8_t device,
                               uint8_t function, uint8_t offset)
{
    outl(PCI_CONFIG_ADDR_PORT,
          pci_make_config_addr(bus, device, function, offset));
    return (uint16_t)((inl(PCI_CONFIG_DATA_PORT) >> ((offset & 2) * 16)) & 0xFFFF);
}

uint32_t pci_read_config_dword(uint8_t bus, uint8_t device,
                                uint8_t function, uint8_t offset)
{
    outl(PCI_CONFIG_ADDR_PORT,
          pci_make_config_addr(bus, device, function, offset));
    return inl(PCI_CONFIG_DATA_PORT);
}

void pci_write_config_byte(uint8_t bus, uint8_t device,
                            uint8_t function, uint8_t offset,
                            uint8_t value)
{
    outl(PCI_CONFIG_ADDR_PORT,
          pci_make_config_addr(bus, device, function, offset));

    uint32_t tmp = inl(PCI_CONFIG_DATA_PORT);
    int shift = (offset & 3) * 8;
    tmp &= ~(0xFF << shift);
    tmp |= ((uint32_t)value << shift);

    outl(PCI_CONFIG_DATA_PORT, tmp);
}

void pci_write_config_word(uint8_t bus, uint8_t device,
                            uint8_t function, uint8_t offset,
                            uint16_t value)
{
    outl(PCI_CONFIG_ADDR_PORT,
          pci_make_config_addr(bus, device, function, offset));

    uint32_t tmp = inl(PCI_CONFIG_DATA_PORT);
    int shift = (offset & 2) * 16;
    tmp &= ~(0xFFFF << shift);
    tmp |= ((uint32_t)value << shift);

    outl(PCI_CONFIG_DATA_PORT, tmp);
}

void pci_write_config_dword(uint8_t bus, uint8_t device,
                             uint8_t function, uint8_t offset,
                             uint32_t value)
{
    outl(PCI_CONFIG_ADDR_PORT,
          pci_make_config_addr(bus, device, function, offset));
    outl(PCI_CONFIG_DATA_PORT, value);
}

/* ============================================================
 * BAR 解析
 * ============================================================ */

void pci_parse_bars(pci_device_t *dev)
{
    if (!dev) {
        return;
    }

    dev->bar_count = 0;

    for (int i = 0; i < 6; i++) {
        uint8_t bar_offset = PCI_BAR0 + i * 4;
        uint32_t bar_val = pci_read_config_dword(dev->bus, dev->device,
                                                  dev->function, bar_offset);

        if (bar_val == 0 || bar_val == 0xFFFFFFFF) {
            dev->bars[i].type = PCI_BAR_NONE;
            continue;
        }

        /* 判断 BAR 类型 */
        if (bar_val & 0x01) {
            /* I/O 空间 BAR */
            dev->bars[i].type = PCI_BAR_IO;
            dev->bars[i].base_addr = bar_val & ~0x03;
            dev->bars[i].is_64bit = 0;

            /* 计算 I/O 空间大小 */
            pci_write_config_dword(dev->bus, dev->device,
                                    dev->function, bar_offset, 0xFFFFFFFF);
            uint32_t size_mask = pci_read_config_dword(dev->bus, dev->device,
                                                        dev->function, bar_offset);
            pci_write_config_dword(dev->bus, dev->device,
                                    dev->function, bar_offset, bar_val);

            dev->bars[i].size = ~(size_mask & ~0x03) + 1;
        } else {
            /* 内存空间 BAR */
            uint32_t mem_type = (bar_val >> 1) & 0x03;

            switch (mem_type) {
                case 0x00:
                    dev->bars[i].type = PCI_BAR_MEMORY_32;
                    dev->bars[i].is_64bit = 0;
                    break;
                case 0x02:
                    dev->bars[i].type = PCI_BAR_MEMORY_64;
                    dev->bars[i].is_64bit = 1;
                    break;
                default:
                    /* 保留类型，按 32 位处理 */
                    dev->bars[i].type = PCI_BAR_MEMORY_32;
                    dev->bars[i].is_64bit = 0;
                    break;
            }

            dev->bars[i].base_addr = bar_val & ~0x0F;
            dev->bars[i].prefetchable = (bar_val >> 3) & 0x01;

            /* 计算内存空间大小 */
            pci_write_config_dword(dev->bus, dev->device,
                                    dev->function, bar_offset, 0xFFFFFFFF);
            uint32_t size_mask = pci_read_config_dword(dev->bus, dev->device,
                                                        dev->function, bar_offset);
            pci_write_config_dword(dev->bus, dev->device,
                                    dev->function, bar_offset, bar_val);

            dev->bars[i].size = ~(size_mask & ~0x0F) + 1;

            /* 如果是 64 位 BAR，读取高 32 位 */
            if (dev->bars[i].is_64bit && i < 5) {
                uint32_t bar_high = pci_read_config_dword(dev->bus, dev->device,
                                                           dev->function,
                                                           bar_offset + 4);
                dev->bars[i].base_addr |= ((uint64_t)bar_high << 32);

                /* 跳过下一个 BAR（它是当前 64 位 BAR 的高位部分）*/
                i++;
            }
        }

        dev->bar_count++;
    }
}

/* ============================================================
 * 设备使能函数
 * ============================================================ */

void pci_enable_memory_space(pci_device_t *dev)
{
    if (!dev) {
        return;
    }
    uint16_t cmd = pci_read_config_word(dev->bus, dev->device,
                                        dev->function, PCI_COMMAND);
    cmd |= PCI_CMD_MEMORY_SPACE;
    pci_write_config_word(dev->bus, dev->device,
                           dev->function, PCI_COMMAND, cmd);
    dev->command = cmd;
}

void pci_enable_io_space(pci_device_t *dev)
{
    if (!dev) {
        return;
    }
    uint16_t cmd = pci_read_config_word(dev->bus, dev->device,
                                        dev->function, PCI_COMMAND);
    cmd |= PCI_CMD_IO_SPACE;
    pci_write_config_word(dev->bus, dev->device,
                           dev->function, PCI_COMMAND, cmd);
    dev->command = cmd;
}

void pci_enable_bus_master(pci_device_t *dev)
{
    if (!dev) {
        return;
    }
    uint16_t cmd = pci_read_config_word(dev->bus, dev->device,
                                        dev->function, PCI_COMMAND);
    cmd |= PCI_CMD_BUS_MASTER;
    pci_write_config_word(dev->bus, dev->device,
                           dev->function, PCI_COMMAND, cmd);
    dev->command = cmd;
}

void pci_disable_bus_master(pci_device_t *dev)
{
    if (!dev) {
        return;
    }
    uint16_t cmd = pci_read_config_word(dev->bus, dev->device,
                                        dev->function, PCI_COMMAND);
    cmd &= ~PCI_CMD_BUS_MASTER;
    pci_write_config_word(dev->bus, dev->device,
                           dev->function, PCI_COMMAND, cmd);
    dev->command = cmd;
}

/* ============================================================
 * 中断管理
 * ============================================================ */

int pci_allocate_irq(pci_device_t *dev)
{
    if (!dev || dev->interrupt_pin == 0) {
        return -1;
    }

    /* 简化实现：直接返回已分配的 IRQ */
    return (int)dev->interrupt_line;
}

const char *pci_get_interrupt_pin_name(uint8_t pin)
{
    switch (pin) {
        case 0: return "None";
        case 1: return "INTA#";
        case 2: return "INTB#";
        case 3: return "INTC#";
        case 4: return "INTD#";
        default: return "Unknown";
    }
}

/* ============================================================
 * 设备扫描与枚举
 * ============================================================ */

static void pci_check_function(uint8_t bus, uint8_t device, uint8_t function)
{
    uint32_t vendor_device = pci_read_config_dword(bus, device,
                                                    function, PCI_VENDOR_ID);
    uint16_t vendor_id = (uint16_t)(vendor_device & 0xFFFF);
    uint16_t device_id = (uint16_t)((vendor_device >> 16) & 0xFFFF);

    if (vendor_id == 0xFFFF || vendor_id == 0x0000) {
        return;  /* 无效厂商 ID，设备不存在 */
    }

    /* 分配并初始化设备结构体 */
    pci_device_t *dev = (pci_device_t *)kmalloc(sizeof(pci_device_t));
    if (!dev) {
        serial_puts(SERIAL_COM1, "[PCI] ERROR: Failed to allocate device struct\n");
        return;
    }

    memset(dev, 0, sizeof(pci_device_t));

    dev->bus = bus;
    dev->device = device;
    dev->function = function;
    dev->vendor_id = vendor_id;
    dev->device_id = device_id;

    /* 读取基本信息 */
    uint32_t class_rev = pci_read_config_dword(bus, device,
                                                function, PCI_REVISION_ID);
    dev->revision_id = (uint8_t)(class_rev & 0xFF);
    dev->prog_if = (uint8_t)((class_rev >> 8) & 0xFF);
    dev->subclass_code = (uint8_t)((class_rev >> 16) & 0xFF);
    dev->class_code = (uint8_t)((class_rev >> 24) & 0xFF);

    /* 读取头类型 */
    uint8_t header_type = pci_read_config_byte(bus, device,
                                               function, PCI_HEADER_TYPE);
    dev->header_type = header_type & 0x7F;

    /* 读取命令/状态寄存器 */
    dev->command = pci_read_config_word(bus, device,
                                        function, PCI_COMMAND);
    dev->status = pci_read_config_word(bus, device,
                                       function, PCI_STATUS);

    /* 读取子系统信息 */
    uint32_t subsys = pci_read_config_dword(bus, device,
                                            function, PCI_SUBSYSTEM_VENDOR_ID);
    dev->subsystem_vendor_id = (uint16_t)(subsys & 0xFFFF);
    dev->subsystem_id = (uint16_t)((subsys >> 16) & 0xFFFF);

    /* 读取中断信息 */
    dev->interrupt_line = pci_read_config_byte(bus, device,
                                              function, PCI_INTERRUPT_LINE);
    dev->interrupt_pin = pci_read_config_byte(bus, device,
                                              function, PCI_INTERRUPT_PIN);

    /* 解析 BAR */
    pci_parse_bars(dev);

    /* 检查能力指针 */
    dev->capabilities_ptr = pci_read_config_byte(bus, device,
                                                 function, PCI_CAPABILITIES_PTR);

    /* 检查 MSI/MSI-X 支持 */
    dev->msi_capable = 0;
    dev->msix_capable = 0;
    dev->pm_capable = 0;

    if (dev->capabilities_ptr != 0 && dev->capabilities_ptr != 0xFF) {
        uint8_t cap_ptr = dev->capabilities_ptr;
        while (cap_ptr != 0) {
            uint8_t cap_id = pci_read_config_byte(bus, device,
                                                    function, cap_ptr);

            switch (cap_id) {
                case PCI_CAP_ID_MSI:
                    dev->msi_capable = 1;
                    break;
                case PCI_CAP_ID_MSIX:
                    dev->msix_capable = 1;
                    break;
                case PCI_CAP_ID_PM:
                    dev->pm_capable = 1;
                    break;
            }

            /* 移动到下一个能力结构 */
            uint8_t next_ptr = pci_read_config_byte(bus, device,
                                                     function, cap_ptr + 1);
            if (next_ptr < 0x40) {
                cap_ptr = next_ptr;
            } else {
                break;
            }
        }
    }

    /* 添加到设备链表 */
    spin_lock(&pci_lock);

    dev->next = device_list;
    dev->prev = NULL;

    if (device_list) {
        device_list->prev = dev;
    }
    device_list = dev;

    device_count++;

    spin_unlock(&pci_lock);
}

static void pci_scan_device(uint8_t bus, uint8_t device)
{
    pci_check_function(bus, device, 0);

    /* 检查是否为多功能设备 */
    uint8_t header_type = pci_read_config_byte(bus, device,
                                               0, PCI_HEADER_TYPE);

    if ((header_type & PCI_HEADER_TYPE_MULTI_FUNC) != 0) {
        for (uint8_t func = 1; func < PCI_MAX_FUNCTION; func++) {
            uint16_t vendor_id = pci_read_config_word(bus, device,
                                                       func, PCI_VENDOR_ID);
            if (vendor_id != 0xFFFF && vendor_id != 0x0000) {
                pci_check_function(bus, device, func);
            }
        }
    }
}

void pci_scan_bus(uint8_t bus)
{
    for (uint8_t dev = 0; dev < PCI_MAX_DEVICE; dev++) {
        pci_scan_device(bus, dev);
    }

    spin_lock(&pci_lock);
    bus_count++;
    spin_unlock(&pci_lock);
}

void pci_scan_all_buses(void)
{
    for (uint8_t bus = 0; bus < PCI_MAX_BUS; bus++) {
        /* 检查总线是否存在 */
        uint16_t vendor_id = pci_read_config_word(bus, 0, 0, PCI_VENDOR_ID);
        if (vendor_id == 0xFFFF || vendor_id == 0x0000) {
            continue;
        }

        pci_scan_bus(bus);
    }
}

int pci_init(void)
{
    if (pci_initialized) {
        return 0;
    }

    serial_puts(SERIAL_COM1, "\n[PCI] Initializing PCI subsystem...\n");

    /* 扫描所有 PCI 总线 */
    pci_scan_all_buses();

    pci_initialized = 1;

    serial_puts(SERIAL_COM1, "[PCI] Initialization complete\n");
    serial_puts(SERIAL_COM1, "[PCI] Found ");
    serial_put_dec(SERIAL_COM1, device_count);
    serial_puts(SERIAL_COM1, " device(s) on ");
    serial_put_dec(SERIAL_COM1, bus_count);
    serial_puts(SERIAL_COM1, " bus(es)\n");

    return 0;
}

/* ============================================================
 * 设备查询接口
 * ============================================================ */

pci_device_t *pci_get_device(uint8_t bus, uint8_t device,
                              uint8_t function)
{
    spin_lock(&pci_lock);

    for (pci_device_t *dev = device_list; dev; dev = dev->next) {
        if (dev->bus == bus &&
            dev->device == device &&
            dev->function == function) {
            spin_unlock(&pci_lock);
            return dev;
        }
    }

    spin_unlock(&pci_lock);
    return NULL;
}

pci_device_t *pci_find_device(uint16_t vendor_id, uint16_t device_id,
                              pci_device_t *from)
{
    spin_lock(&pci_lock);

    pci_device_t *start = from ? from->next : device_list;

    for (pci_device_t *dev = start; dev; dev = dev->next) {
        if ((dev->vendor_id == vendor_id || vendor_id == 0xFFFF) &&
            (dev->device_id == device_id || device_id == 0xFFFF)) {
            spin_unlock(&pci_lock);
            return dev;
        }
    }

    spin_unlock(&pci_lock);
    return NULL;
}

pci_device_t *pci_find_class(uint8_t class_code, uint8_t subclass_code,
                              pci_device_t *from)
{
    spin_lock(&pci_lock);

    pci_device_t *start = from ? from->next : device_list;

    for (pci_device_t *dev = start; dev; dev = dev->next) {
        if ((dev->class_code == class_code || class_code == 0xFF) &&
            (dev->subclass_code == subclass_code || subclass_code == 0xFF)) {
            spin_unlock(&pci_lock);
            return dev;
        }
    }

    spin_unlock(&pci_lock);
    return NULL;
}

int pci_get_device_count(void)
{
    return device_count;
}

/* ============================================================
 * 驱动注册与管理
 * ============================================================ */

int pci_register_driver(pci_driver_t *driver)
{
    if (!driver || !driver->name) {
        return -1;
    }

    spin_lock(&pci_lock);

    driver->next = driver_list;
    driver_list = driver;
    driver_count++;

    spin_unlock(&pci_lock);

    serial_puts(SERIAL_COM1, "[PCI] Driver registered: ");
    serial_puts(SERIAL_COM1, driver->name);
    serial_puts(SERIAL_COM1, "\n");

    return 0;
}

void pci_unregister_driver(pci_driver_t *driver)
{
    if (!driver) {
        return;
    }

    spin_lock(&pci_lock);

    if (driver_list == driver) {
        driver_list = driver->next;
    } else {
        for (pci_driver_t *d = driver_list; d; d = d->next) {
            if (d->next == driver) {
                d->next = driver->next;
                break;
            }
        }
    }

    driver_count--;

    spin_unlock(&pci_lock);
}

void pci_match_drivers(void)
{
    spin_lock(&pci_lock);

    for (pci_driver_t *driver = driver_list; driver; driver = driver->next) {
        for (pci_device_t *dev = device_list; dev; dev = dev->next) {
            /* 检查是否匹配 */
            int match = 0;

            if ((driver->ops.vendor_id == 0xFFFF ||
                 driver->ops.vendor_id == dev->vendor_id) &&
                (driver->ops.device_id == 0xFFFF ||
                 driver->ops.device_id == dev->device_id) &&
                (driver->ops.class_code == 0xFF ||
                 driver->ops.class_code == dev->class_code)) {

                match = 1;
            }

            if (match && driver->ops.probe) {
                int result = driver->ops.probe(dev);

                if (result == 0) {
                    serial_puts(SERIAL_COM1, "[PCI] Device matched: ");
                    serial_put_hex(SERIAL_COM1, dev->vendor_id);
                    serial_puts(SERIAL_COM1, ":");
                    serial_put_hex(SERIAL_COM1, dev->device_id);
                    serial_puts(SERIAL_COM1, " -> ");
                    serial_puts(SERIAL_COM1, driver->name);
                    serial_puts(SERIAL_COM1, "\n");
                }
            }
        }
    }

    spin_unlock(&pci_lock);
}

/* ============================================================
 * MSI / MSI-X 基础框架
 * ============================================================ */

int pci_enable_msi(pci_device_t *dev, int vector_count)
{
    if (!dev || !dev->msi_capable) {
        return 0;
    }

    /* 基础框架：仅记录请求，实际实现在后续版本完善 */
    (void)vector_count;

    serial_puts(SERIAL_COM1, "[PCI] MSI requested for device ");
    serial_put_hex(SERIAL_COM1, dev->vendor_id);
    serial_puts(SERIAL_COM1, ":");
    serial_put_hex(SERIAL_COM1, dev->device_id);
    serial_puts(SERIAL_COM1, " (framework only)\n");

    return 1;  /* 返回最小支持 */
}

void pci_disable_msi(pci_device_t *dev)
{
    if (!dev) {
        return;
    }

    /* 框架占位 */
    (void)dev;
}

int pci_enable_msix(pci_device_t *dev, int vector_count)
{
    if (!dev || !dev->msix_capable) {
        return 0;
    }

    /* 基础框架：仅记录请求 */
    (void)vector_count;

    serial_puts(SERIAL_COM1, "[PCI] MSI-X requested for device ");
    serial_put_hex(SERIAL_COM1, dev->vendor_id);
    serial_puts(SERIAL_COM1, ":");
    serial_put_hex(SERIAL_COM1, dev->device_id);
    serial_puts(SERIAL_COM1, " (framework only)\n");

    return 1;  /* 返回最小支持 */
}

void pci_disable_msix(pci_device_t *dev)
{
    if (!dev) {
        return;
    }

    /* 框架占位 */
    (void)dev;
}

/* ============================================================
 * 调试与统计接口
 * ============================================================ */

const char *pci_class_to_string(uint8_t class_code)
{
    switch (class_code) {
        case PCI_CLASS_OLD:               return "Legacy";
        case PCI_CLASS_STORAGE:           return "Storage";
        case PCI_CLASS_NETWORK:           return "Network";
        case PCI_CLASS_DISPLAY:           return "Display";
        case PCI_CLASS_MULTIMEDIA:        return "Multimedia";
        case PCI_CLASS_MEMORY:            return "Memory";
        case PCI_CLASS_BRIDGE:            return "Bridge";
        case PCI_CLASS_COMMUNICATION:     return "Communication";
        case PCI_CLASS_SYSTEM_PERIPHERAL:  return "System Peripheral";
        case PCI_CLASS_INPUT:             return "Input";
        case PCI_CLASS_DOCKING_STATION:   return "Docking Station";
        case PCI_CLASS_PROCESSOR:         return "Processor";
        case PCI_CLASS_SERIAL_BUS:        return "Serial Bus";
        default:                          return "Unknown";
    }
}

void pci_dump_device(pci_device_t *dev)
{
    if (!dev) {
        return;
    }

    serial_puts(SERIAL_COM1, "\n--- PCI Device ---\n");
    serial_puts(SERIAL_COM1, "  Location: Bus=");
    serial_put_dec(SERIAL_COM1, dev->bus);
    serial_puts(SERIAL_COM1, ", Dev=");
    serial_put_dec(SERIAL_COM1, dev->device);
    serial_puts(SERIAL_COM1, ", Func=");
    serial_put_dec(SERIAL_COM1, dev->function);
    serial_puts(SERIAL_COM1, "\n");

    serial_puts(SERIAL_COM1, "  Vendor:Device = ");
    serial_put_hex(SERIAL_COM1, dev->vendor_id);
    serial_puts(SERIAL_COM1, ":");
    serial_put_hex(SERIAL_COM1, dev->device_id);
    serial_puts(SERIAL_COM1, " (Subsys ");
    serial_put_hex(SERIAL_COM1, dev->subsystem_vendor_id);
    serial_puts(SERIAL_COM1, ":");
    serial_put_hex(SERIAL_COM1, dev->subsystem_id);
    serial_puts(SERIAL_COM1, ")\n");

    serial_puts(SERIAL_COM1, "  Class: ");
    serial_puts(SERIAL_COM1, pci_class_to_string(dev->class_code));
    serial_puts(SERIAL_COM1, " (");
    serial_put_dec(SERIAL_COM1, dev->class_code);
    serial_puts(SERIAL_COM1, ",");
    serial_put_dec(SERIAL_COM1, dev->subclass_code);
    serial_puts(SERIAL_COM1, ",");
    serial_put_dec(SERIAL_COM1, dev->prog_if);
    serial_puts(SERIAL_COM1, ") Rev=");
    serial_put_dec(SERIAL_COM1, dev->revision_id);
    serial_puts(SERIAL_COM1, "\n");

    serial_puts(SERIAL_COM1, "  IRQ: ");
    serial_puts(SERIAL_COM1, pci_get_interrupt_pin_name(dev->interrupt_pin));
    serial_puts(SERIAL_COM1, " -> IRQ ");
    serial_put_dec(SERIAL_COM1, dev->interrupt_line);
    serial_puts(SERIAL_COM1, "\n");

    serial_puts(SERIAL_COM1, "  Capabilities: ");
    if (dev->msi_capable) serial_puts(SERIAL_COM1, "MSI ");
    if (dev->msix_capable) serial_puts(SERIAL_COM1, "MSI-X ");
    if (dev->pm_capable) serial_puts(SERIAL_COM1, "PM ");
    serial_puts(SERIAL_COM1, "\n");

    serial_puts(SERIAL_COM1, "  BARs:\n");
    for (int i = 0; i < dev->bar_count; i++) {
        const char *type_str;
        switch (dev->bars[i].type) {
            case PCI_BAR_IO: type_str = "I/O"; break;
            case PCI_BAR_MEMORY_32: type_str = "Mem32"; break;
            case PCI_BAR_MEMORY_64: type_str = "Mem64"; break;
            default: type_str = "None"; break;
        }

        serial_puts(SERIAL_COM1, "    BAR[");
        serial_put_dec(SERIAL_COM1, i);
        serial_puts(SERIAL_COM1, "] ");
        serial_puts(SERIAL_COM1, type_str);
        serial_puts(SERIAL_COM1, " 0x");
        serial_put_hex(SERIAL_COM1, (uint32_t)dev->bars[i].base_addr);
        serial_puts(SERIAL_COM1, " Size=0x");
        serial_put_hex(SERIAL_COM1, (uint32_t)dev->bars[i].size);
        serial_puts(SERIAL_COM1, "\n");
    }

    serial_puts(SERIAL_COM1, "------------------\n");
}

void pci_dump_devices(void)
{
    serial_puts(SERIAL_COM1, "\n=========================================\n");
    serial_puts(SERIAL_COM1, " PCI Device List (");
    serial_put_dec(SERIAL_COM1, device_count);
    serial_puts(SERIAL_COM1, " devices found)\n");
    serial_puts(SERIAL_COM1, "=========================================\n");

    spin_lock(&pci_lock);

    for (pci_device_t *dev = device_list; dev; dev = dev->next) {
        pci_dump_device(dev);
    }

    spin_unlock(&pci_lock);

    serial_puts(SERIAL_COM1, "=========================================\n");
}

void pci_get_stats(int *total_devices, int *total_buses,
                    int *total_drivers)
{
    if (total_devices) {
        *total_devices = device_count;
    }
    if (total_buses) {
        *total_buses = bus_count;
    }
    if (total_drivers) {
        *total_drivers = driver_count;
    }
}
