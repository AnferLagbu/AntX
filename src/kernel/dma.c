/**
 * @file dma.c
 * @brief DMA 引擎实现
 *
 * 提供一致性 DMA 内存管理、缓冲区映射和传输控制功能。
 * 当前版本为软件模拟实现，后续可扩展为硬件 DMA 支持。
 */

#include "dma.h"
#include "mm.h"
#include "kmalloc.h"
#include "serial.h"
#include "string.h"

/* ============================================================
 * 全局变量
 * ============================================================ */

static dma_mapping_t *mapping_list = NULL;
static int mapping_count = 0;
static int max_concurrent_mappings = 0;

static spinlock_t dma_lock = SPINLOCK_INIT(dma_lock);

static dma_pool_stats_t stats = {0};

static int dma_initialized = 0;

/* ============================================================
 * 内部辅助函数 - 物理地址转换
 * ============================================================ */

/**
 * @brief 虚拟地址转物理地址 (简化版)
 *
 * 注意：这是一个简化实现，仅适用于直接映射的内核内存区域。
 */
static uint64_t virt_to_phys(void *virt)
{
    return (uint64_t)virt;  /* 简化：假设恒等映射或已知偏移 */
}

/**
 * @brief 物理地址转虚拟地址 (简化版)
 */
static void *phys_to_virt(uint64_t phys)
{
    return (void *)phys;  /* 简化 */
}

/* ============================================================
 * 初始化与关闭
 * ============================================================ */

int dma_init(void)
{
    if (dma_initialized) {
        return 0;
    }

    serial_puts(SERIAL_COM1, "\n[DMA] Initializing DMA engine...\n");

    memset(&stats, 0, sizeof(stats));
    mapping_list = NULL;
    mapping_count = 0;
    max_concurrent_mappings = 0;

    dma_initialized = 1;

    serial_puts(SERIAL_COM1, "[DMA] Engine initialized successfully\n");

    return 0;
}

void dma_shutdown(void)
{
    if (!dma_initialized) {
        return;
    }

    spin_lock(&dma_lock);

    /* 清理所有活跃映射 */
    dma_mapping_t *mapping = mapping_list;
    while (mapping) {
        dma_mapping_t *next = mapping->next;

        if (mapping->is_coherent) {
            kfree(mapping->cpu_addr);
        }

        kfree(mapping);
        mapping = next;
    }

    mapping_list = NULL;
    mapping_count = 0;

    spin_unlock(&dma_lock);

    dma_initialized = 0;

    serial_puts(SERIAL_COM1, "[DMA] Engine shutdown complete\n");
}

/* ============================================================
 * 一致性 DMA 内存管理
 * ============================================================ */

void *dma_alloc_coherent(size_t size, size_t align)
{
    void *cpu_addr;
    void *aligned_ptr;

    if (size == 0 || align == 0) {
        return NULL;
    }

    /* 分配稍大的空间以支持对齐 */
    size_t alloc_size = size + align;

    cpu_addr = kmalloc(alloc_size);
    if (!cpu_addr) {
        stats.coherence_fails++;
        return NULL;
    }

    /* 对齐到指定边界 */
    uintptr_t addr = (uintptr_t)cpu_addr;
    aligned_ptr = (void *)((addr + align - 1) & ~(align - 1));

    /* 清零内存 */
    memset(aligned_ptr, 0, size);

    /* 更新统计 */
    spin_lock(&dma_lock);
    stats.total_allocations++;
    stats.total_bytes_allocated += size;
    stats.current_bytes_used += size;
    spin_unlock(&dma_lock);

    return aligned_ptr;
}

void dma_free_coherent(void *addr, size_t size)
{
    if (!addr || !dma_initialized) {
        return;
    }

    /* 更新统计 */
    spin_lock(&dma_lock);
    stats.total_frees++;
    if (stats.current_bytes_used >= size) {
        stats.current_bytes_used -= size;
    }
    spin_unlock(&dma_lock);

    kfree(addr);
}

uint64_t dma_get_device_address(void *cpu_addr)
{
    if (!cpu_addr) {
        return 0;
    }

    return virt_to_phys(cpu_addr);
}

/* ============================================================
 * 流式 DMA 映射
 * ============================================================ */

dma_mapping_t *dma_map_single(void *buffer, size_t size,
                               dma_direction_t direction)
{
    dma_mapping_t *mapping;

    if (!buffer || size == 0 || !dma_initialized) {
        return NULL;
    }

    mapping = (dma_mapping_t *)kmalloc(sizeof(dma_mapping_t));
    if (!mapping) {
        return NULL;
    }

    memset(mapping, 0, sizeof(dma_mapping_t));

    mapping->cpu_addr = buffer;
    mapping->dma_addr = virt_to_phys(buffer);
    mapping->size = size;
    mapping->direction = direction;
    mapping->cache = DMA_CACHE_WRITEBACK;
    mapping->is_coherent = 0;
    mapping->is_mapped = 1;

    /* 根据方向执行缓存操作 */
    if (direction == DMA_TO_DEVICE) {
        dma_sync_for_device(mapping, 0, size);
    } else if (direction == DMA_FROM_DEVICE) {
        dma_sync_for_cpu(mapping, 0, size);
    }

    /* 添加到全局列表 */
    spin_lock(&dma_lock);

    mapping->next = mapping_list;
    mapping->prev = NULL;

    if (mapping_list) {
        mapping_list->prev = mapping;
    }
    mapping_list = mapping;

    mapping_count++;
    stats.total_mappings++;
    if (mapping_count > max_concurrent_mappings) {
        max_concurrent_mappings = mapping_count;
    }
    stats.current_in_use = mapping_count;

    spin_unlock(&dma_lock);

    return mapping;
}

void dma_unmap_single(dma_mapping_t *mapping)
{
    if (!mapping || !mapping->is_mapped || !dma_initialized) {
        return;
    }

    spin_lock(&dma_lock);

    /* 从列表中移除 */
    if (mapping->next) {
        mapping->next->prev = mapping->prev;
    }
    if (mapping->prev) {
        mapping->prev->next = mapping->next;
    } else {
        mapping_list = mapping->next;
    }

    mapping_count--;
    stats.total_unmappings++;
    stats.current_in_use = mapping_count;

    spin_unlock(&dma_lock);

    mapping->is_mapped = 0;
    kfree(mapping);
}

dma_mapping_t *dma_map_sg(dma_scatter_list_t *sglist,
                           dma_direction_t direction)
{
    dma_mapping_t *mapping;
    size_t total_size;

    if (!sglist || sglist->entry_count == 0 || !dma_initialized) {
        return NULL;
    }

    total_size = dma_sg_total_length(sglist);

    mapping = (dma_mapping_t *)kmalloc(sizeof(dma_mapping_t));
    if (!mapping) {
        return NULL;
    }

    memset(mapping, 0, sizeof(dma_mapping_t));

    /* 使用第一个条目的地址作为代表 */
    mapping->cpu_addr = sglist->entries[0].page_addr;
    mapping->dma_addr = sglist->entries[0].phys_addr;
    mapping->size = total_size;
    mapping->direction = direction;
    mapping->cache = DMA_CACHE_WRITEBACK;
    mapping->is_coherent = 0;
    mapping->is_mapped = 1;

    /* 同步所有条目 */
    for (int i = 0; i < sglist->entry_count; i++) {
        if (direction == DMA_TO_DEVICE) {
            /* 确保所有数据写入内存 */
            __asm__ volatile("mfence" ::: "memory");
        }
    }

    /* 添加到全局列表 */
    spin_lock(&dma_lock);

    mapping->next = mapping_list;
    mapping->prev = NULL;

    if (mapping_list) {
        mapping_list->prev = mapping;
    }
    mapping_list = mapping;

    mapping_count++;
    stats.total_mappings++;

    spin_unlock(&dma_lock);

    return mapping;
}

void dma_unmap_sg(dma_mapping_t *mapping)
{
    if (!mapping || !mapping->is_mapped || !dma_initialized) {
        return;
    }

    spin_lock(&dma_lock);

    if (mapping->next) {
        mapping->next->prev = mapping->prev;
    }
    if (mapping->prev) {
        mapping->prev->next = mapping->next;
    } else {
        mapping_list = mapping->next;
    }

    mapping_count--;
    stats.total_unmappings++;

    spin_unlock(&dma_lock);

    mapping->is_mapped = 0;
    kfree(mapping);
}

/* ============================================================
 * DMA 同步操作
 * ============================================================ */

void dma_sync_for_device(dma_mapping_t *mapping,
                          size_t offset, size_t size)
{
    if (!mapping || !mapping->cpu_addr || !dma_initialized) {
        return;
    }

    /* 执行写回屏障，确保 CPU 写入的数据对设备可见 */
    smp_wmb();

    /* 在 x86 上，这通常由硬件维护一致性，
       但我们仍然执行显式屏障以确保安全 */
    __asm__ volatile("" ::: "memory");
}

void dma_sync_for_cpu(dma_mapping_t *mapping,
                       size_t offset, size_t size)
{
    if (!mapping || !mapping->cpu_addr || !dma_initialized) {
        return;
    }

    /* 执行读屏障，确保设备写入的数据对 CPU 可见 */
    smp_rmb();

    /* 使相关缓存行失效（简化实现中省略）*/
    __asm__ volatile("" ::: "memory");
}

void dma_sync_both(dma_mapping_t *mapping,
                    size_t offset, size_t size)
{
    dma_sync_for_device(mapping, offset, size);
    dma_sync_for_cpu(mapping, offset, size);
}

/* ============================================================
 * DMA 传输控制
 * ============================================================ */

int dma_memcpy(uint64_t dest, uint64_t source,
               size_t length, dma_direction_t direction)
{
    void *dest_ptr, *src_ptr;

    if (length == 0) {
        return 0;
    }

    dest_ptr = phys_to_virt(dest);
    src_ptr = phys_to_virt(source);

    if (!dest_ptr || !src_ptr) {
        return -1;
    }

    switch (direction) {
        case DMA_TO_DEVICE:
        case DMA_FROM_DEVICE:
            memcpy(dest_ptr, src_ptr, length);
            break;

        case DMA_BIDIRECTIONAL:
            memcpy(dest_ptr, src_ptr, length);
            break;

        default:
            return -1;
    }

    /* 内存屏障确保传输完成 */
    smp_mb();

    return 0;
}

int dma_async_memcpy(dma_transfer_t *transfer)
{
    if (!transfer || !dma_initialized) {
        return -1;
    }

    transfer->completed = 0;
    transfer->result = 0;

    /* 执行实际传输 */
    int result = dma_memcpy(transfer->dst_addr, transfer->src_addr,
                             transfer->length, transfer->direction);

    transfer->completed = 1;
    transfer->result = result;

    /* 如果有回调函数，调用它 */
    if (transfer->callback) {
        transfer->callback(transfer->private_data, result);
    }

    return 0;
}

int dma_wait_for_completion(dma_transfer_t *transfer,
                            unsigned int timeout_ms)
{
    uint64_t start, current;

    if (!transfer || !dma_initialized) {
        return -1;
    }

    __asm__ volatile("rdtsc" : "=A"(start));

    while (!transfer->completed) {
        if (timeout_ms > 0) {
            __asm__ volatile("rdtsc" : "=A"(current));

            if ((current - start) > (uint64_t)timeout_ms * 2400000ULL) {
                return -1;  /* 超时 */
            }
        }

        __asm__ volatile("pause" ::: "memory");
    }

    return transfer->result;
}

int dma_cancel_transfer(dma_transfer_t *transfer)
{
    if (!transfer || !dma_initialized) {
        return -1;
    }

    if (transfer->completed) {
        return -1;  /* 已经完成，无法取消 */
    }

    /* 标记为取消 */
    transfer->completed = 1;
    transfer->result = -1;  /* 取消错误码 */

    return 0;
}

dma_transfer_t *dma_create_transfer(uint64_t src, uint64_t dst,
                                     size_t length,
                                     dma_direction_t direction,
                                     dma_callback_t callback,
                                     void *private_data)
{
    dma_transfer_t *transfer;

    if (length == 0 || !dma_initialized) {
        return NULL;
    }

    transfer = (dma_transfer_t *)kmalloc(sizeof(dma_transfer_t));
    if (!transfer) {
        return NULL;
    }

    memset(transfer, 0, sizeof(dma_transfer_t));

    transfer->src_addr = src;
    transfer->dst_addr = dst;
    transfer->length = length;
    transfer->direction = direction;
    transfer->synchronous = (callback == NULL);
    transfer->completed = 0;
    transfer->result = 0;
    transfer->callback = callback;
    transfer->private_data = private_data;

    return transfer;
}

void dma_destroy_transfer(dma_transfer_t *transfer)
{
    if (!transfer) {
        return;
    }

    /* 如果传输还在进行中，先尝试取消 */
    if (!transfer->completed) {
        dma_cancel_transfer(transfer);
    }

    kfree(transfer);
}

/* ============================================================
 * 散射-聚集辅助函数
 * ============================================================ */

void dma_sg_init(dma_scatter_list_t *sglist)
{
    if (!sglist) {
        return;
    }

    memset(sglist, 0, sizeof(dma_scatter_list_t));
    sglist->entry_count = 0;
    sglist->total_length = 0;
}

int dma_sg_add_entry(dma_scatter_list_t *sglist,
                     void *addr, size_t length)
{
    if (!sglist || !addr || length == 0) {
        return -1;
    }

    if (sglist->entry_count >= DMA_MAX_SCATTER_ENTRIES) {
        return -1;  /* 列表已满 */
    }

    int idx = sglist->entry_count;

    sglist->entries[idx].page_addr = addr;
    sglist->entries[idx].phys_addr = virt_to_phys(addr);
    sglist->entries[idx].length = length;

    sglist->entry_count++;
    sglist->total_length += length;

    return 0;
}

size_t dma_sg_total_length(dma_scatter_list_t *sglist)
{
    if (!sglist) {
        return 0;
    }

    return sglist->total_length;
}

/* ============================================================
 * 查询与调试接口
 * ============================================================ */

void dma_get_stats(dma_pool_stats_t *stats_out)
{
    if (!stats_out) {
        return;
    }

    spin_lock(&dma_lock);
    memcpy(stats_out, &stats, sizeof(dma_pool_stats_t));
    spin_unlock(&dma_lock);
}

void dma_dump_stats(void)
{
    serial_puts(SERIAL_COM1, "\n=== DMA Engine Statistics ===\n");
    serial_puts(SERIAL_COM1, "  Total Allocations: ");
    serial_put_dec(SERIAL_COM1, stats.total_allocations);
    serial_puts(SERIAL_COM1, "\n  Total Frees: ");
    serial_put_dec(SERIAL_COM1, stats.total_frees);
    serial_puts(SERIAL_COM1, "\n  Total Mappings: ");
    serial_put_dec(SERIAL_COM1, stats.total_mappings);
    serial_puts(SERIAL_COM1, "\n  Total Unmappings: ");
    serial_put_dec(SERIAL_COM1, stats.total_unmappings);
    serial_puts(SERIAL_COM1, "\n  Current Active: ");
    serial_put_dec(SERIAL_COM1, stats.current_in_use);
    serial_puts(SERIAL_COM1, "\n  Max Concurrent: ");
    serial_put_dec(SERIAL_COM1, stats.max_concurrent);
    serial_puts(SERIAL_COM1, "\n  Coherence Fails: ");
    serial_put_dec(SERIAL_COM1, stats.coherence_fails);
    serial_puts(SERIAL_COM1, "\n  Total Bytes Allocated: ");
    serial_put_dec(SERIAL_COM1, (uint32_t)(stats.total_bytes_allocated / 1024));
    serial_puts(SERIAL_COM1, " KB\n  Current Bytes Used: ");
    serial_put_dec(SERIAL_COM1, (uint32_t)(stats.current_bytes_used / 1024));
    serial_puts(SERIAL_COM1, " KB\n=============================\n");
}

void dump_active_mappings(void)
{
    serial_puts(SERIAL_COM1, "\n--- Active DMA Mappings ---\n");

    spin_lock(&dma_lock);

    int count = 0;
    for (dma_mapping_t *m = mapping_list; m; m = m->next) {
        const char *dir_str;

        switch (m->direction) {
            case DMA_TO_DEVICE: dir_str = "To Device"; break;
            case DMA_FROM_DEVICE: dir_str = "From Device"; break;
            case DMA_BIDIRECTIONAL: dir_str = "Bidirectional"; break;
            default: dir_str = "Unknown"; break;
        }

        serial_puts(SERIAL_COM1, "  [");
        serial_put_dec(SERIAL_COM1, count);
        serial_puts(SERIAL_COM1, "] CPU=0x");
        serial_put_hex(SERIAL_COM1, (uint32_t)(uintptr_t)m->cpu_addr);
        serial_puts(SERIAL_COM1, " DMA=0x");
        serial_put_hex(SERIAL_COM1, (uint32_t)m->dma_addr);
        serial_puts(SERIAL_COM1, " Size=");
        serial_put_dec(SERIAL_COM1, (uint32_t)m->size);
        serial_puts(SERIAL_COM1, " Dir=");
        serial_puts(SERIAL_COM1, dir_str);
        serial_puts(SERIAL_COM1, "\n");

        count++;
    }

    spin_unlock(&dma_lock);

    serial_puts(SERIAL_COM1, "  Total: ");
    serial_put_dec(SERIAL_COM1, count);
    serial_puts(SERIAL_COM1, " mappings\n--------------------------\n");
}

void dma_reset_stats(void)
{
    spin_lock(&dma_lock);
    memset(&stats, 0, sizeof(dma_pool_stats_t));
    spin_unlock(&dma_lock);

    serial_puts(SERIAL_COM1, "[DMA] Statistics reset\n");
}
