/**
 * @file slab.h
 * @brief Slab 分配器接口定义
 *
 * 提供基于 Slab 的高效内核对象分配器，适用于频繁分配释放固定大小对象的场景。
 * Slab 分配器通过预分配和对象复用机制，显著降低内存碎片。
 */

#ifndef SLAB_H
#define SLAB_H

#include "spinlock.h"
#include "types.h"

/* ============================================================
 * 常量定义
 * ============================================================ */

/**
 * @brief 默认的 Slab 大小 (4KB)
 */
#define SLAB_DEFAULT_SIZE      4096

/**
 * @brief 最大对象大小 (2KB)
 */
#define SLAB_MAX_OBJECT_SIZE   2048

/**
 * @brief 最小对象大小 (16字节)
 */
#define SLAB_MIN_OBJECT_SIZE   16

/**
 * @brief 预定义的通用缓存数量
 */
#define SLAB_GENERAL_CACHE_NUM 8

/**
 * @brief 预定义的通用缓存大小
 */
extern const size_t slab_general_sizes[SLAB_GENERAL_CACHE_NUM];

/* ============================================================
 * 数据结构定义
 * ============================================================ */

/**
 * @brief Slab 结构体
 *
 * 表示一个物理页面级别的内存块，包含多个相同大小的对象。
 */
typedef struct Slab {
    void *start_addr;          /**< Slab 起始地址 */
    unsigned int obj_count;    /**< 总对象数 */
    unsigned int active_count; /**< 已分配对象数 */
    unsigned char *bitmap;     /**< 位图标记对象使用状态 */
    struct Slab *next;         /**< 下一个 Slab (链表) */
    struct Slab *prev;         /**< 上一个 Slab (链表) */
    int full;                  /**< 是否已满 (1=满, 0=未满) */
} Slab;

/**
 * @brief 缓存结构体
 *
 * 管理一组相同大小的 Slab，提供特定大小对象的分配接口。
 */
typedef struct KmemCache {
    const char *name;          /**< 缓存名称 */
    size_t object_size;        /**< 单个对象大小 (字节) */
    unsigned int objects_per_slab;  /**< 每个 Slab 可容纳的对象数 */
    unsigned int slab_count;    /**< 当前 Slab 总数 */

    Slab *slabs_full;          /**< 完全使用的 Slab 链表 */
    Slab *slabs_partial;       /**< 部分使用的 Slab 链表 */
    Slab *slabs_free;          /**< 完全空闲的 Slab 链表 */

    spinlock_t lock;           /**< 保护此缓存的自旋锁 */

    unsigned long total_allocs;    /**< 总分配次数统计 */
    unsigned long total_frees;     /**< 总释放次数统计 */
    unsigned long cache_hits;      /**< 命中已有 Slab 次数 */
    unsigned long cache_misses;    /**< 需要新 Slab 次数 */
} KmemCache;

/* ============================================================
 * 核心接口函数声明
 * ============================================================ */

/**
 * @brief 初始化 Slab 分配器系统
 *
 * 必须在 PMM 和 VMM 初始化之后调用。
 *
 * @return 0 成功，非零失败
 */
int slab_system_init(void);

/**
 * @brief 创建新的对象缓存
 *
 * @param name 缓存名称
 * @param size 对象大小（字节）
 * @return 新创建的缓存指针，失败返回 NULL
 */
KmemCache *kmem_cache_create(const char *name, size_t size);

/**
 * @brief 从缓存中分配一个对象
 *
 * @param cache 目标缓存指针
 * @return 对象指针，失败返回 NULL
 */
void *kmem_cache_alloc(KmemCache *cache);

/**
 * @brief 释放一个对象回缓存
 *
 * @param cache 目标缓存指针
 * @param obj 要释放的对象指针
 */
void kmem_cache_free(KmemCache *cache, void *obj);

/**
 * @brief 销毁一个缓存
 *
 * 释放该缓存管理的所有 Slab 内存。
 *
 * @param cache 要销毁的缓存指针
 */
void kmem_cache_destroy(KmemCache *cache);

/* ============================================================
 * 通用分配接口 (kmalloc/kfree 替代)
 * ============================================================ */

/**
 * @brief 通用内存分配 (Slab 版本)
 *
 * 根据请求大小自动选择合适的缓存。
 * 仅支持 <= 2048 字节的分配请求。
 *
 * @param size 请求的大小（字节）
 * @return 内存指针，失败返回 NULL
 */
void *slab_alloc(size_t size);

/**
 * @brief 通用内存释放 (Slab 版本)
 *
 * 自动识别所属缓存并释放。
 *
 * @param ptr 要释放的内存指针
 */
void slab_free(void *ptr);

/* ============================================================
 * 查询与调试接口
 * ============================================================ */

/**
 * @brief 获取缓存的统计信息
 *
 * @param cache 目标缓存指针
 * @param total_objects 输出：总对象容量
 * @param active_objects 输出：已使用对象数
 * @param total_slabs 输出：总 Slab 数
 */
void kmem_cache_get_stats(KmemCache *cache,
                           unsigned int *total_objects,
                           unsigned int *active_objects,
                           unsigned int *total_slabs);

/**
 * @brief 打印所有缓存的统计信息
 */
void slab_dump_all_caches(void);

/**
 * @brief 打印指定缓存的详细信息
 *
 * @param cache 目标缓存指针
 */
void kmem_cache_dump(KmemCache *cache);

/**
 * @brief 获取系统总体统计信息
 *
 * @param total_memory 输出：总内存使用量（字节）
 * @param used_memory 输出：已使用内存量（字节）
 * @param total_caches 输出：缓存总数
 */
void slab_get_system_stats(unsigned long *total_memory,
                            unsigned long *used_memory,
                            unsigned int *total_caches);

#endif /* SLAB_H */
