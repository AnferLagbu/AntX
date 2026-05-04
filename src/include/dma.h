/**
 * @file dma.h
 * @brief DMA 引擎接口定义
 *
 * 提供一致性 DMA 内存管理、缓冲区映射和传输控制功能，
 * 支持高性能 I/O 操作（网络、存储等）。
 */

#ifndef DMA_H
#define DMA_H

#include "types.h"
#include "spinlock.h"

/* ============================================================
 * DMA 常量定义
 * ============================================================ */

#define DMA_MAX_MAPPINGS        256
#define DMA_MAX_SCATTER_ENTRIES 64

/**
 * @brief DMA 方向枚举
 */
typedef enum {
    DMA_TO_DEVICE = 0,     /**< 数据从内存到设备 */
    DMA_FROM_DEVICE = 1,   /**< 数据从设备到内存 */
    DMA_BIDIRECTIONAL = 2  /**< 双向传输 */
} dma_direction_t;

/**
 * @brief DMA 缓存策略
 */
typedef enum {
    DMA_CACHE_NONE = 0,    /**< 无缓存 */
    DMA_CACHE_WRITEBACK = 1, /**< 写回缓存 */
    DMA_CACHE_WRITETHROUGH = 2 /**< 直写缓存 */
} dma_cache_policy_t;

/* ============================================================
 * 数据结构定义
 * ============================================================ */

/**
 * @brief DMA 映射信息结构体
 *
 * 记录一次 DMA 映射的详细信息，用于后续的同步和取消映射。
 */
typedef struct dma_mapping {
    void *cpu_addr;          /**< CPU 可访问的虚拟地址 */
    uint64_t dma_addr;       /**< 设备可访问的物理/DMA 地址 */
    size_t size;             /**< 映射区域大小 (字节) */
    dma_direction_t direction; /**< 传输方向 */
    dma_cache_policy_t cache;   /**< 缓存策略 */

    int is_coherent;         /**< 是否为一致性映射 */
    int is_mapped;           /**< 当前是否有效 */

    struct dma_mapping *next;
    struct dma_mapping *prev;
} dma_mapping_t;

/**
 * @brief 散射-聚集列表项
 *
 * 描述一个不连续的物理内存块。
 */
typedef struct dma_scatter_entry {
    uint64_t phys_addr;      /**< 物理地址 */
    size_t length;            /**< 长度 (字节) */
    void *page_addr;          /**< 页面虚拟地址 */
} dma_scatter_entry_t;

/**
 * @brief 散射-聚集列表
 *
 * 用于描述不连续内存区域的 DMA 操作。
 */
typedef struct dma_scatter_list {
    int entry_count;         /**< 条目数 */
    dma_scatter_entry_t entries[DMA_MAX_SCATTER_ENTRIES]; /**< 条目数组 */
    size_t total_length;     /**< 总长度 */
    dma_direction_t direction;
} dma_scatter_list_t;

/**
 * @brief DMA 传输完成回调函数类型
 *
 * @param private_data 用户私有数据
 * @param result 传输结果 (0=成功，非零=错误)
 */
typedef void (*dma_callback_t)(void *private_data, int result);

/**
 * @brief DMA 传输请求结构体
 */
typedef struct dma_transfer {
    uint64_t src_addr;        /**< 源地址 (物理地址) */
    uint64_t dst_addr;        /**< 目标地址 (物理地址) */
    size_t length;            /**< 传输长度 */
    dma_direction_t direction;

    int synchronous;           /**< 是否同步传输 */
    int completed;             /**< 是否已完成 */
    int result;                /**< 传输结果 */

    dma_callback_t callback;  /**< 异步完成回调 */
    void *private_data;        /**< 回调私有数据 */

    struct dma_transfer *next;
} dma_transfer_t;

/**
 * @brief DMA 池统计信息
 */
typedef struct dma_pool_stats {
    unsigned long total_allocations;  /**< 总分配次数 */
    unsigned long total_frees;       /**< 总释放次数 */
    unsigned long total_mappings;    /**< 总映射次数 */
    unsigned long total_unmappings;  /**< 总取消映射次数 */
    unsigned long current_in_use;    /**< 当前使用中的映射数 */
    unsigned long max_concurrent;    /**< 最大并发映射数 */
    unsigned long coherence_fails;   /**< 一致性内存分配失败次数 */
    size_t total_bytes_allocated;   /**< 总分配字节数 */
    size_t current_bytes_used;       /**< 当前已用字节数 */
} dma_pool_stats_t;

/* ============================================================
 * 核心接口 - 初始化与配置
 * ============================================================ */

/**
 * @brief 初始化 DMA 子系统
 *
 * 必须在 PMM 和 VMM 初始化之后调用。
 *
 * @return 0 成功，非零失败
 */
int dma_init(void);

/**
 * @brief 关闭 DMA 子系统
 *
 * 释放所有资源并清理状态。
 */
void dma_shutdown(void);

/* ============================================================
 * 一致性 DMA 内存管理
 * ============================================================ */

/**
 * @brief 分配一致性 DMA 内存
 *
 * 分配一块对设备和 CPU 都可见的内存区域，
 * 保证数据一致性（自动刷新缓存）。
 *
 * @param size 分配大小 (字节)
 * @param align 对齐要求 (字节，必须是 2 的幂)
 * @return CPU 可访问的虚拟地址，失败返回 NULL
 */
void *dma_alloc_coherent(size_t size, size_t align);

/**
 * @brief 释放一致性 DMA 内存
 *
 * @param addr 之前通过 dma_alloc_coherent() 分配的地址
 * @param size 原始分配大小
 */
void dma_free_coherent(void *addr, size_t size);

/**
 * @brief 获取一致性 DMA 内存的设备地址
 *
 * @param cpu_addr CPU 虚拟地址
 * @return 设备可用的物理/DMA 地址
 */
uint64_t dma_get_device_address(void *cpu_addr);

/**
 * @brief 映射物理 MMIO 区域到内核虚拟地址空间 (ioremap)
 *
 * 将设备 MMIO BAR 的物理地址范围映射到内核可访问的虚拟地址。
 * 映射属性: 无缓存(UC) + 写穿透(WT)，适合 MMIO 寄存器访问。
 *
 * @param phys_addr 物理基地址
 * @param size 映射大小（字节）
 * @return 内核虚拟地址，失败返回 NULL
 */
void *ioremap(uint64_t phys_addr, size_t size);

/**
 * @brief 取消 ioremap 映射
 *
 * @param virt_addr ioremap 返回的虚拟地址
 * @param size 映射大小
 */
void iounmap(void *virt_addr, size_t size);

/* ============================================================
 * 流式 DMA 映射
 * ============================================================ */

/**
 * @brief 映射 CPU 地址用于 DMA 传输
 *
 * 将一段已有的内核缓冲区映射到 DMA 可访问的空间。
 * 对于流式 DMA，需要在传输前后进行同步操作。
 *
 * @param buffer CPU 缓冲区指针
 * @param size 缓冲区大小
 * @param direction DMA 方向
 * @return DMA 映射信息指针，失败返回 NULL
 */
dma_mapping_t *dma_map_single(void *buffer, size_t size,
                               dma_direction_t direction);

/**
 * @brief 取消单页 DMA 映射
 *
 * @param mapping DMA 映射信息
 */
void dma_unmap_single(dma_mapping_t *mapping);

/**
 * @brief 映射散射-聚集列表用于 DMA
 *
 * @param sglist 散射-聚集列表
 * @param direction DMA 方向
 * @return DMA 映射信息指针
 */
dma_mapping_t *dma_map_sg(dma_scatter_list_t *sglist,
                           dma_direction_t direction);

/**
 * @brief 取消散射-聚集 DMA 映射
 *
 * @param mapping DMA 易射信息
 */
void dma_unmap_sg(dma_mapping_t *mapping);

/* ============================================================
 * DMA 同步操作
 * ============================================================ */

/**
 * @brief DMA 传输前同步 (CPU -> Device)
 *
 * 确保CPU写入的数据对设备可见（刷新缓存）。
 *
 * @param mapping DMA 映射信息
 * @param offset 偏移量
 * @param size 同步范围大小
 */
void dma_sync_for_device(dma_mapping_t *mapping,
                          size_t offset, size_t size);

/**
 * @brief DMA 传输后同步 (Device -> CPU)
 *
 * 确保设备写入的数据对CPU可见（使缓存失效）。
 *
 * @param mapping DMA 映射信息
 * @param offset 偏移量
 * @param size 同步范围大小
 */
void dma_sync_for_cpu(dma_mapping_t *mapping,
                       size_t offset, size_t size);

/**
 * @brief 完整同步 (双向)
 *
 * 同时执行 for_device 和 for_cpu 操作。
 *
 * @param mapping DMA 映射信息
 * @param offset 偏移量
 * @param size 同步范围大小
 */
void dma_sync_both(dma_mapping_t *mapping,
                    size_t offset, size_t size);

/* ============================================================
 * DMA 传输控制
 * ============================================================ */

/**
 * @brief 执行同步 DMA 内存拷贝
 *
 * 在两块内存之间进行 DMA 传输（模拟或硬件加速）。
 *
 * @param dest 目标地址 (物理)
 * @param source 源地址 (物理)
 * @param length 传输长度
 * @param direction 传输方向
 * @return 0 成功，非零失败
 */
int dma_memcpy(uint64_t dest, uint64_t source,
               size_t length, dma_direction_t direction);

/**
 * @brief 启动异步 DMA 传输
 *
 * @param transfer 已初始化的传输请求
 * @return 0 成功启动，非零失败
 */
int dma_async_memcpy(dma_transfer_t *transfer);

/**
 * @brief 等待异步 DMA 传输完成
 *
 * @param transfer 传输请求
 * @param timeout_ms 超时时间 (毫秒)，0=无限等待
 * @return 0 完成，非零超时或错误
 */
int dma_wait_for_completion(dma_transfer_t *transfer,
                            unsigned int timeout_ms);

/**
 * @brief 取消正在进行的 DMA 传输
 *
 * @param transfer 传输请求
 * @return 0 成功取消，非零无法取消
 */
int dma_cancel_transfer(dma_transfer_t *transfer);

/**
 * @brief 创建 DMA 传输请求
 *
 * @param src 源地址 (物理)
 * @param dst 目标地址 (物理)
 * @param length 传输长度
 * @param direction 方向
 * @param callback 完成回调 (可为 NULL)
 * @param private_data 回调私有数据
 * @return 新创建的传输请求指针，失败返回 NULL
 */
dma_transfer_t *dma_create_transfer(uint64_t src, uint64_t dst,
                                     size_t length,
                                     dma_direction_t direction,
                                     dma_callback_t callback,
                                     void *private_data);

/**
 * @brief 销毁 DMA 传输请求
 *
 * @param transfer 要销毁的传输请求
 */
void dma_destroy_transfer(dma_transfer_t *transfer);

/* ============================================================
 * 散射-聚集辅助函数
 * ============================================================ */

/**
 * @brief 初始化散射-聚集列表
 *
 * @param sglist 散射-聚集列表指针
 */
void dma_sg_init(dma_scatter_list_t *sglist);

/**
 * @brief 向散射-聚集列表添加条目
 *
 * @param sglist 散射-聚集列表
 * @param addr 虚拟地址
 * @param length 长度
 * @return 0 成功，-1 列表已满
 */
int dma_sg_add_entry(dma_scatter_list_t *sglist,
                     void *addr, size_t length);

/**
 * @brief 计算散射-聚集列表总长度
 *
 * @param sglist 散射-聚集列表
 * @return 总长度 (字节)
 */
size_t dma_sg_total_length(dma_scatter_list_t *sglist);

/* ============================================================
 * 查询与调试接口
 * ============================================================ */

/**
 * @brief 获取 DMA 统计信息
 *
 * @param stats 输出：统计信息结构体
 */
void dma_get_stats(dma_pool_stats_t *stats);

/**
 * @brief 打印 DMA 统计信息
 */
void dma_dump_stats(void);

/**
 * @brief 打印当前活跃的 DMA 映射
 */
void dump_active_mappings(void);

/**
 * @brief 重置 DMA 统计计数器
 */
void dma_reset_stats(void);

#endif /* DMA_H */
