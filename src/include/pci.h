/**
 * @file pci.h
 * @brief PCI 总线驱动接口定义
 *
 * 提供 x86_64 架构下的 PCI (Peripheral Component Interconnect) 总线驱动框架，
 * 支持设备枚举、配置空间访问、BAR 解析和中断路由。
 */

#ifndef PCI_H
#define PCI_H

#include "types.h"
#include "spinlock.h"

/* ============================================================
 * PCI 常量定义
 * ============================================================ */

#define PCI_CONFIG_ADDR_PORT   0xCF8
#define PCI_CONFIG_DATA_PORT    0xCFC

#define PCI_MAX_BUS             256
#define PCI_MAX_DEVICE          32
#define PCI_MAX_FUNCTION        8

#define PCI_VENDOR_ID           0x00
#define PCI_DEVICE_ID           0x02
#define PCI_COMMAND             0x04
#define PCI_STATUS              0x06
#define PCI_REVISION_ID         0x08
#define PCI_PROG_IF            0x09
#define PCI_SUBCLASS           0x0A
#define PCI_CLASS_CODE         0x0B
#define PCI_CACHE_LINE_SIZE    0x0C
#define PCI_LATENCY_TIMER      0x0D
#define PCI_HEADER_TYPE        0x0E
#define PCI_BIST               0x0F
#define PCI_BAR0               0x10
#define PCI_BAR1               0x14
#define PCI_BAR2               0x18
#define PCI_BAR3               0x1C
#define PCI_BAR4               0x20
#define PCI_BAR5               0x24
#define PCI_CARDBUS_CIS_PTR    0x28
#define PCI_SUBSYSTEM_VENDOR_ID 0x2C
#define PCI_SUBSYSTEM_ID       0x2E
#define PCI_EXPANSION_ROM_BASE 0x30
#define PCI_CAPABILITIES_PTR   0x34
#define PCI_INTERRUPT_LINE     0x3C
#define PCI_INTERRUPT_PIN      0x3D
#define PCI_MIN_GNT           0x3E
#define PCI_MAX_LAT           0x3F

/* ============================================================
 * PCI 命令寄存器位定义
 * ============================================================ */

#define PCI_CMD_IO_SPACE       (1 << 0)
#define PCI_CMD_MEMORY_SPACE    (1 << 1)
#define PCI_CMD_BUS_MASTER      (1 << 2)
#define PCI_CMD_SPECIAL_CYCLES  (1 << 3)
#define PCI_MEM_WRITE_ENABLE    (1 << 4)
#define PCI_VGA_PALETTE_SNOOP   (1 << 5)
#define PCI_PARITY_ERR_RESPOND  (1 << 6)
#define PCI_SERR_ENABLE         (1 << 8)
#define PCI_FAST_BACK_TO_BACK   (1 << 9)
#define PCI_INT_DISABLE         (1 << 10)

/* ============================================================
 * PCI 头类型定义
 * ============================================================ */

#define PCI_HEADER_TYPE_NORMAL    0x00
#define PCI_HEADER_TYPE_BRIDGE    0x01
#define PCI_HEADER_TYPE_CARDBUS   0x02

#define PCI_HEADER_TYPE_MULTI_FUNC (1 << 7)

/* ============================================================
 * PCI 类别代码 (Class Code) 定义
 * ============================================================ */

#define PCI_CLASS_OLD                0x00
#define PCI_CLASS_STORAGE            0x01
#define PCI_CLASS_NETWORK            0x02
#define PCI_CLASS_DISPLAY            0x03
#define PCI_CLASS_MULTIMEDIA         0x04
#define PCI_CLASS_MEMORY             0x05
#define PCI_CLASS_BRIDGE             0x06
#define PCI_CLASS_COMMUNICATION      0x07
#define PCI_CLASS_SYSTEM_PERIPHERAL  0x08
#define PCI_CLASS_INPUT              0x09
#define PCI_CLASS_DOCKING_STATION    0x0A
#define PCI_CLASS_PROCESSOR          0x0B
#define PCI_CLASS_SERIAL_BUS         0x0C
#define PCI_CLASS_WIRELESS           0x0D
#define PCI_CLASS_INTELLIGENT_IO     0x0E
#define PCI_CLASS_SATELLITE          0x0F
#define PCI_CLASS_ENCRYPTION         0x10
#define PCI_CLASS_DATA_ACQUISITION   0x11
#define PCI_CLASS_PROGRAMMING        0x12

/* ============================================================
 * PCI 能力结构 (Capability) 定义
 * ============================================================ */

#define PCI_CAP_ID_PM              0x01  /* Power Management */
#define PCI_CAP_ID_AGP             0x02  /* AGP */
#define PCI_CAP_ID_VPD             0x03  /* Vital Product Data */
#define PCI_CAP_ID_SLOTID          0x04  /* Slot Identification */
#define PCI_CAP_ID_MSI             0x05  /* Message Signaled Interrupts */
#define PCI_CAP_ID_CHSWP           0x06  /* CompactPCI HotSwap */
#define PCI_CAP_ID_PCIX           0x07  /* PCI-X */
#define PCI_CAP_ID_HT             0x08  /* HyperTransport */
#define PCI_CAP_ID_VNDR           0x09  /* Vendor Specific */
#define PCI_CAP_ID_DEBUG_PORT      0x0A  /* Debug Port */
#define PCI_CAP_ID_CCRC           0x0B  /* CompactPCI Central Resource Control */
#define PCI_CAP_ID_HOTPLUG        0x0C  /* PCI Hot-Plug */
#define PCI_CAP_ID_SSVID          0x0D  /* Bridge Subsystem Vendor/Subsystem ID */
#define PCI_CAP_ID_AGP3           0x0E  /* AGP Target */
#define PCI_CAP_ID_SECURE         0x0F  /* Secure Device */
#define PCI_CAP_ID_EXPRESS        0x10  /* PCI Express */
#define PCI_CAP_ID_MSIX           0x11  /* MSI-X */
#define PCI_CAPID_AF             0x13  /* Advanced Features */
#define PCI_CAP_ID_EA             0x14  /* Enhanced Allocation */

/* ============================================================
 * 数据结构定义
 * ============================================================ */

/**
 * @brief PCI BAR 类型枚举
 */
typedef enum {
    PCI_BAR_NONE = 0,        /**< 无效/未使用 */
    PCI_BAR_IO = 1,           /**< I/O 空间映射 */
    PCI_BAR_MEMORY_32 = 2,    /**< 32位内存映射 */
    PCI_BAR_MEMORY_64 = 3     /**< 64位内存映射 */
} pci_bar_type_t;

/**
 * @brief PCI BAR (Base Address Register) 信息
 */
typedef struct pci_bar {
    uint64_t base_addr;       /**< 基地址 */
    uint64_t size;            /**< 大小 (字节) */
    pci_bar_type_t type;      /**< BAR 类型 */
    int prefetchable;         /**< 可预取标志 (仅内存BAR有效) */
    int is_64bit;             /**< 是否为64位BAR */
} pci_bar_t;

/**
 * @brief PCI 设备信息结构体
 *
 * 描述一个 PCI 设备的完整配置信息。
 */
typedef struct pci_device {
    uint8_t bus;              /**< 总线号 (0-255) */
    uint8_t device;           /**< 设备号 (0-31) */
    uint8_t function;         /**< 功能号 (0-7) */

    uint16_t vendor_id;       /**< 厂商 ID */
    uint16_t device_id;       /**< 设备 ID */
    uint16_t subsystem_vendor_id; /**< 子系统厂商 ID */
    uint16_t subsystem_id;    /**< 子系统 ID */

    uint8_t revision_id;      /**< 版本号 */
    uint8_t class_code;       /**< 类别代码 */
    uint8_t subclass_code;    /**< 子类别代码 */
    uint8_t prog_if;          /**< 编程接口 */
    uint8_t header_type;      /**< 头类型 */

    uint16_t command;         /**< 命令寄存器值 */
    uint16_t status;          /**< 状态寄存器值 */

    uint8_t interrupt_line;   /**< 中断线 (IRQ 号) */
    uint8_t interrupt_pin;    /**< 中断引脚 (INTA#/INTB#/INTC#/INTD#) */

    pci_bar_t bars[6];        /**< 6 个 BAR 寄存器信息 */
    int bar_count;            /**< 有效 BAR 数量 */

    uint32_t capabilities_ptr; /**< 能力指针 */
    int msi_capable;          /**< 支持 MSI */
    int msix_capable;         /**< 支持 MSI-X */
    int pm_capable;           /**< 支持电源管理 */

    struct pci_device *next;  /**< 设备链表下一节点 */
    struct pci_device *prev;  /**< 设备链表前一节点 */
} pci_device_t;

/**
 * @brief PCI 驱动操作接口
 *
 * 所有 PCI 设备驱动必须实现的回调函数集合。
 */
struct pci_driver_ops {
    const char *name;         /**< 驱动名称 */
    uint16_t vendor_id;       /**< 支持的厂商 ID (0xFFFF=任意) */
    uint16_t device_id;       /**< 支持的设备 ID (0xFFFF=任意) */
    uint8_t class_code;       /**< 支持的类别代码 (0xFF=任意) */

    /**
     * @brief 探测设备
     *
     * @param dev PCI 设备指针
     * @return 0 成功，非零失败
     */
    int (*probe)(pci_device_t *dev);

    /**
     * @brief 移除设备
     *
     * @param dev PCI 设备指针
     */
    void (*remove)(pci_device_t *dev);

    /**
     * @brief 挂起设备 (电源管理)
     *
     * @param dev PCI 设备指针
     */
    void (*suspend)(pci_device_t *dev);

    /**
     * @brief 恢复设备 (电源管理)
     *
     * @param dev PCI 设备指针
     */
    void (*resume)(pci_device_t *dev);
};

/**
 * @brief PCI 驱动注册信息
 */
typedef struct pci_driver {
    const char *name;
    struct pci_driver_ops ops;
    struct pci_driver *next;
} pci_driver_t;

/* ============================================================
 * 核心接口函数声明
 * ============================================================ */

/**
 * @brief 初始化 PCI 子系统
 *
 * 扫描所有 PCI 总线和设备，建立设备列表。
 *
 * @return 0 成功，非零失败
 */
int pci_init(void);

/**
 * @brief 扫描指定总线
 *
 * @param bus 总线号
 */
void pci_scan_bus(uint8_t bus);

/**
 * @brief 扫描所有总线
 */
void pci_scan_all_buses(void);

/* ============================================================
 * PCI 配置空间访问
 * ============================================================ */

/**
 * @brief 读取 PCI 配置空间字节
 *
 * @param bus 总线号
 * @param device 设备号
 * @param function 功能号
 * @param offset 偏移量
 * @return 读取的字节值
 */
uint8_t pci_read_config_byte(uint8_t bus, uint8_t device,
                              uint8_t function, uint8_t offset);

/**
 * @brief 读取 PCI 配置空间字 (16位)
 *
 * @param bus 总线号
 * @param device 设备号
 * @param function 功能号
 * @param offset 偏移量
 * @return 读取的字值
 */
uint16_t pci_read_config_word(uint8_t bus, uint8_t device,
                               uint8_t function, uint8_t offset);

/**
 * @brief 读取 PCI 配置空间双字 (32位)
 *
 * @param bus 总线号
 * @param device 设备号
 * @param function 功能号
 * @param offset 偏移量
 * @return 读取的双字值
 */
uint32_t pci_read_config_dword(uint8_t bus, uint8_t device,
                                uint8_t function, uint8_t offset);

/**
 * @brief 写入 PCI 配置空间字节
 *
 * @param bus 总线号
 * @param device 设备号
 * @param function 功能号
 * @param offset 偏移量
 * @param value 要写入的值
 */
void pci_write_config_byte(uint8_t bus, uint8_t device,
                            uint8_t function, uint8_t offset,
                            uint8_t value);

/**
 * @brief 写入 PCI 配置空间字 (16位)
 *
 * @param bus 总线号
 * @param device 设备号
 * @param function 功能号
 * @param offset 偏移量
 * @param value 要写入的值
 */
void pci_write_config_word(uint8_t bus, uint8_t device,
                            uint8_t function, uint8_t offset,
                            uint16_t value);

/**
 * @brief 写入 PCI 配置空间双字 (32位)
 *
 * @param bus 总线号
 * @param device 设备号
 * @param function 功能号
 * @param offset 偏移量
 * @param value 要写入的值
 */
void pci_write_config_dword(uint8_t bus, uint8_t device,
                             uint8_t function, uint8_t offset,
                             uint32_t value);

/* ============================================================
 * 设备查询接口
 * ============================================================ */

/**
 * @brief 根据位置获取 PCI 设备
 *
 * @param bus 总线号
 * @param device 设备号
 * @param function 功能号
 * @return 设备指针，未找到返回 NULL
 */
pci_device_t *pci_get_device(uint8_t bus, uint8_t device,
                              uint8_t function);

/**
 * @brief 根据厂商/设备 ID 查找设备
 *
 * @param vendor_id 厂商 ID
 * @param device_id 设备 ID
 * @param from 从哪个设备开始搜索 (NULL=从头开始)
 * @return 设备指针，未找到返回 NULL
 */
pci_device_t *pci_find_device(uint16_t vendor_id, uint16_t device_id,
                              pci_device_t *from);

/**
 * @brief 根据类别代码查找设备
 *
 * @param class_code 类别代码
 * @param subclass_code 子类别代码
 * @param from 从哪个设备开始搜索
 * @return 设备指针
 */
pci_device_t *pci_find_class(uint8_t class_code, uint8_t subclass_code,
                              pci_device_t *from);

/**
 * @brief 获取设备总数
 *
 * @return 已发现的 PCI 设备数量
 */
int pci_get_device_count(void);

/* ============================================================
 * BAR 操作接口
 * ============================================================ */

/**
 * @brief 解析设备的所有 BAR
 *
 * @param dev PCI 设备指针
 */
void pci_parse_bars(pci_device_t *dev);

/**
 * @brief 启用设备的内存空间访问
 *
 * @param dev PCI 设备指针
 */
void pci_enable_memory_space(pci_device_t *dev);

/**
 * @brief 启用设备的 I/O 空间访问
 *
 * @param dev PCI 设备指针
 */
void pci_enable_io_space(pci_device_t *dev);

/**
 * @brief 启用设备的 Bus Mastering
 *
 * 允许设备直接进行 DMA 访问。
 *
 * @param dev PCI 设备指针
 */
void pci_enable_bus_master(pci_device_t *dev);

/**
 * @brief 禁用设备的 Bus Mastering
 *
 * @param dev PCI 设备指针
 */
void pci_disable_bus_master(pci_device_t *dev);

/* ============================================================
 * 中断管理接口
 * ============================================================ */

/**
 * @brief 分配 IRQ 给设备
 *
 * @param dev PCI 设备指针
 * @return 分配的 IRQ 号，-1 表示失败
 */
int pci_allocate_irq(pci_device_t *dev);

/**
 * @brief 获取设备的中断引脚名称
 *
 * @param pin 中断引脚号 (1-4)
 * @return 引脚名称字符串 ("INTA#", "INTB#" 等)
 */
const char *pci_get_interrupt_pin_name(uint8_t pin);

/* ============================================================
 * 驱动注册接口
 * ============================================================ */

/**
 * @brief 注册 PCI 驱动
 *
 * @param driver 驱动结构体指针
 * @return 0 成功，非零失败
 */
int pci_register_driver(pci_driver_t *driver);

/**
 * @brief 注销 PCI 驱动
 *
 * @param driver 驱动结构体指针
 */
void pci_unregister_driver(pci_driver_t *driver);

/**
 * @brief 匹配并绑定驱动到设备
 *
 * 扫描所有已注册的驱动，尝试匹配未绑定的设备。
 */
void pci_match_drivers(void);

/* ============================================================
 * MSI / MSI-X 接口 (基础框架)
 * ============================================================ */

/**
 * @brief 启用 MSI 中断
 *
 * @param dev PCI 设备指针
 * @param vector_count 请求的中断向量数
 * @return 实际分配的向量数，0 表示失败
 */
int pci_enable_msi(pci_device_t *dev, int vector_count);

/**
 * @brief 禁用 MSI 中断
 *
 * @param dev PCI 设备指针
 */
void pci_disable_msi(pci_device_t *dev);

/**
 * @brief 启用 MSI-X 中断
 *
 * @param dev PCI 设备指针
 * @param vector_count 请求的中断向量数
 * @return 实际分配的向量数，0 表示失败
 */
int pci_enable_msix(pci_device_t *dev, int vector_count);

/**
 * @brief 禁用 MSI-X 中断
 *
 * @param dev PCI 设备指针
 */
void pci_disable_msix(pci_device_t *dev);

/* ============================================================
 * 调试与统计接口
 * ============================================================ */

/**
 * @brief 打印所有已发现设备的信息
 */
void pci_dump_devices(void);

/**
 * @brief 打印指定设备的详细信息
 *
 * @param dev PCI 设备指针
 */
void pci_dump_device(pci_device_t *dev);

/**
 * @brief 获取 PCI 子系统统计信息
 *
 * @param total_devices 输出：总设备数
 * @param total_buses 输出：总总线数
 * @param total_drivers 输出：已注册驱动数
 */
void pci_get_stats(int *total_devices, int *total_buses,
                    int *total_drivers);

/**
 * @brief 将 PCI 类别代码转换为可读字符串
 *
 * @param class_code 类别代码
 * @return 类别名称字符串
 */
const char *pci_class_to_string(uint8_t class_code);

#endif /* PCI_H */
