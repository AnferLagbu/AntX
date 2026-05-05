#include "pci.h"
#include "io.h"
#include "klog.h"
#include "string.h"
#include "kmalloc.h"

static pci_device_t *device_list = NULL;
static int device_count = 0;
static int bus_count = 0;

static pci_driver_t *driver_list = NULL;
static int driver_count = 0;

static spinlock_t pci_lock = SPINLOCK_INIT(pci_lock);

static int pci_initialized = 0;

static uint32_t pci_make_config_addr(uint8_t bus, uint8_t device,
                                      uint8_t function, uint8_t offset)
{
    return (uint32_t)(0x80000000 |
           ((uint32_t)bus << 16) |
           ((uint32_t)(device & 0x1F) << 11) |
           ((uint32_t)(function & 0x07) << 8) |
           ((uint32_t)(offset & 0xFC)));
}

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

        if (bar_val & 0x01) {
            dev->bars[i].type = PCI_BAR_IO;
            dev->bars[i].base_addr = bar_val & ~0x03;
            dev->bars[i].is_64bit = 0;

            pci_write_config_dword(dev->bus, dev->device,
                                    dev->function, bar_offset, 0xFFFFFFFF);
            uint32_t size_mask = pci_read_config_dword(dev->bus, dev->device,
                                                        dev->function, bar_offset);
            pci_write_config_dword(dev->bus, dev->device,
                                    dev->function, bar_offset, bar_val);

            dev->bars[i].size = ~(size_mask & ~0x03) + 1;
        } else {
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
                    dev->bars[i].type = PCI_BAR_MEMORY_32;
                    dev->bars[i].is_64bit = 0;
                    break;
            }

            dev->bars[i].base_addr = bar_val & ~0x0F;
            dev->bars[i].prefetchable = (bar_val >> 3) & 0x01;

            pci_write_config_dword(dev->bus, dev->device,
                                    dev->function, bar_offset, 0xFFFFFFFF);
            uint32_t size_mask = pci_read_config_dword(dev->bus, dev->device,
                                                        dev->function, bar_offset);
            pci_write_config_dword(dev->bus, dev->device,
                                    dev->function, bar_offset, bar_val);

            dev->bars[i].size = ~(size_mask & ~0x0F) + 1;

            if (dev->bars[i].is_64bit && i < 5) {
                uint32_t bar_high = pci_read_config_dword(dev->bus, dev->device,
                                                           dev->function,
                                                           bar_offset + 4);
                dev->bars[i].base_addr |= ((uint64_t)bar_high << 32);

                i++;
            }
        }

        dev->bar_count++;
    }
}

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

int pci_allocate_irq(pci_device_t *dev)
{
    if (!dev || dev->interrupt_pin == 0) {
        return -1;
    }

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

static void pci_check_function(uint8_t bus, uint8_t device, uint8_t function)
{
    uint32_t vendor_device = pci_read_config_dword(bus, device,
                                                    function, PCI_VENDOR_ID);
    uint16_t vendor_id = (uint16_t)(vendor_device & 0xFFFF);
    uint16_t device_id = (uint16_t)((vendor_device >> 16) & 0xFFFF);

    if (vendor_id == 0xFFFF || vendor_id == 0x0000) {
        return;
    }

    pci_device_t *dev = (pci_device_t *)kmalloc(sizeof(pci_device_t));
    if (!dev) {
        klog_drv_err("PCI: Failed to allocate device struct");
        return;
    }

    memset(dev, 0, sizeof(pci_device_t));

    dev->bus = bus;
    dev->device = device;
    dev->function = function;
    dev->vendor_id = vendor_id;
    dev->device_id = device_id;

    uint32_t class_rev = pci_read_config_dword(bus, device,
                                                function, PCI_REVISION_ID);
    dev->revision_id = (uint8_t)(class_rev & 0xFF);
    dev->prog_if = (uint8_t)((class_rev >> 8) & 0xFF);
    dev->subclass_code = (uint8_t)((class_rev >> 16) & 0xFF);
    dev->class_code = (uint8_t)((class_rev >> 24) & 0xFF);

    uint8_t header_type = pci_read_config_byte(bus, device,
                                               function, PCI_HEADER_TYPE);
    dev->header_type = header_type & 0x7F;

    dev->command = pci_read_config_word(bus, device,
                                        function, PCI_COMMAND);
    dev->status = pci_read_config_word(bus, device,
                                       function, PCI_STATUS);

    uint32_t subsys = pci_read_config_dword(bus, device,
                                            function, PCI_SUBSYSTEM_VENDOR_ID);
    dev->subsystem_vendor_id = (uint16_t)(subsys & 0xFFFF);
    dev->subsystem_id = (uint16_t)((subsys >> 16) & 0xFFFF);

    dev->interrupt_line = pci_read_config_byte(bus, device,
                                              function, PCI_INTERRUPT_LINE);
    dev->interrupt_pin = pci_read_config_byte(bus, device,
                                              function, PCI_INTERRUPT_PIN);

    pci_parse_bars(dev);

    dev->capabilities_ptr = pci_read_config_byte(bus, device,
                                                 function, PCI_CAPABILITIES_PTR);

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

            uint8_t next_ptr = pci_read_config_byte(bus, device,
                                                     function, cap_ptr + 1);
            if (next_ptr < 0x40) {
                cap_ptr = next_ptr;
            } else {
                break;
            }
        }
    }

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

    klog_drv("Initializing PCI subsystem...");

    pci_scan_all_buses();

    pci_initialized = 1;

    klog_drv("PCI initialization complete: %d device(s) on %d bus(es)", device_count, bus_count);

    return 0;
}

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

    klog_drv("Driver registered: %s", driver->name);

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
                    klog_drv("Device matched: 0x%x:0x%x -> %s",
                             dev->vendor_id, dev->device_id, driver->name);
                }
            }
        }
    }

    spin_unlock(&pci_lock);
}

int pci_enable_msi(pci_device_t *dev, int vector_count)
{
    if (!dev || !dev->msi_capable) {
        return 0;
    }

    (void)vector_count;

    klog_drv("MSI requested for device 0x%x:0x%x (framework only)",
             dev->vendor_id, dev->device_id);

    return 1;
}

void pci_disable_msi(pci_device_t *dev)
{
    if (!dev) {
        return;
    }

    (void)dev;
}

int pci_enable_msix(pci_device_t *dev, int vector_count)
{
    if (!dev || !dev->msix_capable) {
        return 0;
    }

    (void)vector_count;

    klog_drv("MSI-X requested for device 0x%x:0x%x (framework only)",
             dev->vendor_id, dev->device_id);

    return 1;
}

void pci_disable_msix(pci_device_t *dev)
{
    if (!dev) {
        return;
    }

    (void)dev;
}

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

    klog_drv("PCI Device: Bus=%d, Dev=%d, Func=%d",
             dev->bus, dev->device, dev->function);
    klog_drv("  Vendor:Device = 0x%x:0x%x (Subsys 0x%x:0x%x)",
             dev->vendor_id, dev->device_id,
             dev->subsystem_vendor_id, dev->subsystem_id);
    klog_drv("  Class: %s (%d,%d,%d) Rev=%d",
             pci_class_to_string(dev->class_code),
             dev->class_code, dev->subclass_code, dev->prog_if,
             dev->revision_id);
    klog_drv("  IRQ: %s -> IRQ %d",
             pci_get_interrupt_pin_name(dev->interrupt_pin),
             dev->interrupt_line);

    char caps[64] = "";
    if (dev->msi_capable) { strcat(caps, "MSI "); }
    if (dev->msix_capable) { strcat(caps, "MSI-X "); }
    if (dev->pm_capable) { strcat(caps, "PM "); }
    klog_drv("  Capabilities: %s", caps);

    for (int i = 0; i < dev->bar_count; i++) {
        const char *type_str;
        switch (dev->bars[i].type) {
            case PCI_BAR_IO: type_str = "I/O"; break;
            case PCI_BAR_MEMORY_32: type_str = "Mem32"; break;
            case PCI_BAR_MEMORY_64: type_str = "Mem64"; break;
            default: type_str = "None"; break;
        }

        klog_drv("  BAR[%d] %s 0x%x Size=0x%x",
                 i, type_str,
                 (uint32_t)dev->bars[i].base_addr,
                 (uint32_t)dev->bars[i].size);
    }
}

void pci_dump_devices(void)
{
    klog_drv("PCI Device List (%d devices found)", device_count);

    spin_lock(&pci_lock);

    for (pci_device_t *dev = device_list; dev; dev = dev->next) {
        pci_dump_device(dev);
    }

    spin_unlock(&pci_lock);
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
