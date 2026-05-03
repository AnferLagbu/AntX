#ifndef _SMP_H
#define _SMP_H

#include "types.h"

#define MAX_CPUS           64
#define AP_BOOT_ADDR       0x7000
#define AP_STACK_SIZE      0x4000  /* 16KB per CPU */
#define APIC_ID_INVALID    0xFF

/**
 * @brief CPU 状态枚举
 */
typedef enum {
    CPU_STATE_UNINITIALIZED = 0,  /**< 未初始化 */
    CPU_STATE_BOOTING,             /**< 正在启动 */
    CPU_STATE_RUNNING,             /**< 运行中 */
    CPU_STATE_HALTED,              /**< 已停止 */
    CPU_STATE_ERROR                /**< 错误状态 */
} cpu_state_t;

/**
 * @brief Per-CPU 数据结构
 *
 * 每个 CPU 核心拥有独立的数据副本，
 * 避免并发访问冲突。
 */
typedef struct {
    /** 基本信息 */
    uint8_t  apic_id;              /**< Local APIC ID */
    uint8_t  cpu_id;               /**< 逻辑 CPU 编号 (0 = BSP) */
    uint8_t  is_bsp;               /**< 是否为 Bootstrap Processor */
    cpu_state_t state;              /**< 当前状态 */

    /** 栈空间 */
    void    *kernel_stack;          /**< 内核栈顶 */
    void    *kernel_stack_bottom;   /**< 内核栈底 */

    /** GDT 和 TSS (Per-CPU) */
    uint64_t gdt_ptr;               /**< GDT 基地址 */
    uint16_t gdt_size;              /**< GDT 大小 */
    uint64_t tss_ptr;               /**< TSS 结构地址 */

    /** Local APIC */
    volatile uint32_t *local_apic;  /**< Local APIC MMIO 地址 */
    uint32_t apic_base;             /**< APIC base physical address */

    /** 调度相关 */
    uint64_t current_thread;        /**< 当前运行的线程 ID */
    uint64_t scheduler_ticks;       /**< 调度器 tick 计数 */
    int      preempt_count;         /**< 抢占计数器 */

    /** 统计信息 */
    uint64_t interrupts_total;      /**< 总中断数 */
    uint64_t ipi_received;          /**< 收到的 IPI 数 */
    uint64_t ipi_sent;              /**< 发送的 IPI 数 */

    /** CPU 特性 */
    char     vendor_string[13];     /**< 厂商字符串 */
    uint32_t max_cpuid_leaf;        /**< 最大 CPUID leaf */
} cpu_info_t;

/**
 * @brief IPI 类型定义
 */
typedef enum {
    IPI_INTERRUPT = 0,              /**< 通用中断 */
    IPI_RESCHEDULE,                 /**< 重新调度 */
    IPI_STOP,                       /**< 停止 CPU */
    IPI_FLUSH_TLB,                  /**< TLB 刷新 */
    IPI_CALL_FUNCTION,              /**< 远程函数调用 */
    IPI_MAX_TYPES
} ipi_type_t;

/**
 * @brief IPI 处理函数类型
 */
typedef void (*ipi_handler_t)(cpu_info_t *cpu, void *data);

/**
 * @brief 初始化 SMP 子系统
 *
 * 检测并启动所有 Application Processors (AP)。
 *
 * @return 成功启动的 CPU 数量 (包括 BSP)，-1 表示失败
 */
int smp_init(void);

/**
 * @brief 获取当前 CPU 信息
 *
 * @return 当前 CPU 的信息结构指针
 */
cpu_info_t* smp_get_current_cpu(void);

/**
 * @brief 根据 APIC ID 获取 CPU 信息
 *
 * @param apic_id Local APIC ID
 * @return CPU 信息指针，如果不存在返回 NULL
 */
cpu_info_t* smp_get_cpu(uint8_t apic_id);

/**
 * @brief 获取 BSP (Bootstrap Processor)
 *
 * @return BSP 的 CPU 信息指针
 */
cpu_info_t* smp_get_bsp(void);

/**
 * @brief 获取活跃 CPU 数量
 *
 * @return 正在运行的 CPU 数量
 */
int smp_get_active_cpu_count(void);

/**
 * @brief 发送 IPI 到指定 CPU
 *
 * @param target_apic_id 目标 CPU 的 APIC ID
 * @param type IPI 类型
 * @param data 可选数据指针
 * @return 0 成功，-1 失败
 */
int smp_send_ipi(uint8_t target_apic_id, ipi_type_t type, void *data);

/**
 * @brief 广播 IPI 到所有其他 CPU
 *
 * @param exclude_self 是否排除自身
 * @param type IPI 类型
 * @param data 可选数据指针
 * @return 0 成功，-1 失败
 */
int smp_broadcast_ipi(int exclude_self, ipi_type_t type, void *data);

/**
 * @brief 注册 IPI 处理程序
 *
 * @param type IPI 类型
 * @param handler 处理函数
 * @return 0 成功，-1 失败
 */
int smp_register_ipi_handler(ipi_type_t type, ipi_handler_t handler);

/**
 * @brief 等待所有 CPU 达到同步点
 *
 * 用于 barrier synchronization。
 *
 * @param timeout_us 超时时间（微秒）
 * @return 0 成功，-1 超时
 */
int smp_barrier_wait(uint32_t timeout_us);

/**
 * @brief 打印 SMP 状态信息
 */
void smp_dump_status(void);

/**
 * @brief 停止指定 CPU
 *
 * @param apic_id 目标 CPU 的 APIC ID
 * @return 0 成功，-1 失败
 */
int smp_stop_cpu(uint8_t apic_id);

/**
 * @brief 重启指定 CPU
 *
 * @param apic_id 目标 CPU 的 APIC ID
 * @return 0 成功，-1 失败
 */
int smp_restart_cpu(uint8_t apic_id);

#endif /* _SMP_H */
