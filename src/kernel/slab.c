/**
 * @file slab.c
 * @brief Slab 分配器实现
 *
 * 基于伙伴系统 (Buddy System) 和位图管理的 Slab 分配器。
 * 预定义 8 个通用缓存，支持高效的小对象内存分配。
 */

#include "slab.h"
#include "mm.h"
#include "kmalloc.h"
#include "klog.h"
#include "string.h"

/* ============================================================
 * 全局变量
 * ============================================================ */

/**
 * @brief 预定义的通用缓存大小
 */
const size_t slab_general_sizes[SLAB_GENERAL_CACHE_NUM] = {
    16, 32, 64, 128, 256, 512, 1024, 2048
};

/**
 * @brief 通用缓存数组
 */
static KmemCache *general_caches[SLAB_GENERAL_CACHE_NUM] = { NULL };

/**
 * @brief 系统是否已初始化
 */
static int slab_initialized = 0;

/* ============================================================
 * 内部辅助函数
 * ============================================================ */

/**
 * @brief 根据对象大小选择合适的通用缓存索引
 *
 * @param size 请求的大小
 * @return 缓存索引，如果超出范围返回 -1
 */
static int find_general_cache_index(size_t size)
{
    for (int i = 0; i < SLAB_GENERAL_CACHE_NUM; i++) {
        if (size <= slab_general_sizes[i]) {
            return i;
        }
    }
    return -1;
}

/**
 * @brief 计算每个 Slab 可容纳的对象数
 *
 * @param object_size 对象大小
 * @return 每个 Slab 的对象数
 */
static unsigned int calculate_objects_per_slab(size_t object_size)
{
    size_t usable_size = SLAB_DEFAULT_SIZE - sizeof(Slab);

    if (object_size < SLAB_MIN_OBJECT_SIZE) {
        object_size = SLAB_MIN_OBJECT_SIZE;
    }

    return (unsigned int)(usable_size / object_size);
}

/**
 * @brief 创建一个新的 Slab
 *
 * 从 PMM 分配一页物理内存并初始化为 Slab。
 * 包含边界检查以防止 GPF（General Protection Fault）。
 *
 * @param cache 所属缓存指针
 * @return 新创建的 Slab 指针，失败返回 NULL
 */
static Slab *slab_new(KmemCache *cache)
{
    void *page;
    Slab *slab;
    unsigned int bitmap_bytes;
    size_t total_needed;

    page = pmm_alloc_page();
    if (!page) {
        return NULL;
    }

    slab = (Slab *)page;
    slab->start_addr = (void *)((uint8_t *)page + sizeof(Slab));
    slab->obj_count = cache->objects_per_slab;
    slab->active_count = 0;
    slab->next = NULL;
    slab->prev = NULL;
    slab->full = 0;

    bitmap_bytes = (cache->objects_per_slab + 7) / 8;

    /*
     * 🔧 Phase 2 修复: 边界检查防止 GPF
     *
     * 确保 [Slab header] + [objects] + [bitmap] 不超过页面大小
     * 如果超出，动态减少 obj_count 以适应页面限制
     */
    total_needed = (size_t)((uint8_t *)slab->start_addr - (uint8_t *)page) +
                    (size_t)cache->objects_per_slab * cache->object_size +
                    bitmap_bytes;

    if (total_needed > SLAB_DEFAULT_SIZE) {
        /* 重新计算可容纳的最大对象数 */
        size_t available_space = SLAB_DEFAULT_SIZE -
                                  sizeof(Slab) - bitmap_bytes;
        unsigned int max_objects = (unsigned int)(available_space / cache->object_size);

        if (max_objects < 1) {
            klog_mem_err("SLAB: Object size %zu too large for single page", 
                        cache->object_size);
            pmm_free_page(page);
            return NULL;
        }

        klog_mem_warn("SLAB: Reduced objects from %u to %u (size=%zu)",
                     cache->objects_per_slab, max_objects, cache->object_size);
        
        slab->obj_count = max_objects;
        bitmap_bytes = (max_objects + 7) / 8;
    }

    slab->bitmap = (unsigned char *)(slab->start_addr) +
                    slab->obj_count * cache->object_size;

    /* 最终安全检查: 确保 bitmap 在页面内 */
    if ((uintptr_t)slab->bitmap + bitmap_bytes > 
        (uintptr_t)page + SLAB_DEFAULT_SIZE) {
        klog_mem_err("SLAB: Bitmap overflow detected, aborting slab creation");
        pmm_free_page(page);
        return NULL;
    }

    memset(slab->bitmap, 0, bitmap_bytes);

    return slab;
}

/**
 * @brief 销毁一个 Slab 并释放其内存
 *
 * @param slab 要销毁的 Slab 指针
 */
static void slab_destroy(Slab *slab)
{
    if (slab && slab->start_addr) {
        pmm_free_page(slab);
    }
}

/**
 * @brief 在 Slab 中查找空闲对象
 *
 * 使用线性扫描位图的方式。
 *
 * @param slab 目标 Slab 指针
 * @return 空闲对象的偏移量，-1 表示无空闲对象
 */
static int slab_find_free(Slab *slab)
{
    for (unsigned int i = 0; i < slab->obj_count; i++) {
        unsigned int byte_idx = i / 8;
        unsigned int bit_idx = i % 8;

        if (!(slab->bitmap[byte_idx] & (1 << bit_idx))) {
            return (int)i;
        }
    }
    return -1;
}

/**
 * @brief 将 Slab 从链表中移除
 *
 * @param head 链表头指针的指针
 * @param slab 要移除的 Slab 指针
 */
static void list_remove(Slab **head, Slab *slab)
{
    if (!*head || !slab) {
        return;
    }

    if (*head == slab) {
        *head = slab->next;
        if (*head) {
            (*head)->prev = NULL;
        }
    } else {
        if (slab->next) {
            slab->next->prev = slab->prev;
        }
        if (slab->prev) {
            slab->prev->next = slab->next;
        }
    }

    slab->next = NULL;
    slab->prev = NULL;
}

/**
 * @brief 将 Slab 添加到链表头部
 *
 * @param head 链表头指针的指针
 * @param slab 要添加的 Slab 指针
 */
static void list_push_front(Slab **head, Slab *slab)
{
    if (!slab) {
        return;
    }

    slab->next = *head;
    slab->prev = NULL;

    if (*head) {
        (*head)->prev = slab;
    }
    *head = slab;
}

/* ============================================================
 * 缓存核心实现
 * ============================================================ */

int slab_system_init(void)
{
    char name[32];

    for (int i = 0; i < SLAB_GENERAL_CACHE_NUM; i++) {
        snprintf(name, sizeof(name), "slab-%zu",
                 slab_general_sizes[i]);

        general_caches[i] = kmem_cache_create(name,
                                               slab_general_sizes[i]);
        if (!general_caches[i]) {
            klog_mem_err("SLAB: failed to create general cache");
            return -1;
        }
    }

    slab_initialized = 1;

    klog_mem("SLAB: System initialized with %d general caches", SLAB_GENERAL_CACHE_NUM);

    return 0;
}

KmemCache *kmem_cache_create(const char *name, size_t size)
{
    KmemCache *cache;

    if (size > SLAB_MAX_OBJECT_SIZE || size == 0) {
        return NULL;
    }

    if (size < SLAB_MIN_OBJECT_SIZE) {
        size = SLAB_MIN_OBJECT_SIZE;
    }

    cache = (KmemCache *)kmalloc(sizeof(KmemCache));
    if (!cache) {
        return NULL;
    }

    memset(cache, 0, sizeof(KmemCache));

    cache->name = name;
    cache->object_size = size;
    cache->objects_per_slab = calculate_objects_per_slab(size);
    cache->slabs_full = NULL;
    cache->slabs_partial = NULL;
    cache->slabs_free = NULL;
    spin_init(&cache->lock);

    return cache;
}

void *kmem_cache_alloc(KmemCache *cache)
{
    Slab *slab;
    int free_idx;
    void *obj_ptr;

    if (!cache) {
        return NULL;
    }

    spin_lock(&cache->lock);

    cache->total_allocs++;

    slab = cache->slabs_partial;
    if (slab) {
        cache->cache_hits++;
    } else {
        slab = cache->slabs_free;
        if (!slab) {
            slab = slab_new(cache);
            if (!slab) {
                spin_unlock(&cache->lock);
                return NULL;
            }
            list_push_front(&cache->slabs_free, slab);
            cache->slab_count++;
            cache->cache_misses++;
        } else {
            cache->cache_hits++;
        }
    }

    free_idx = slab_find_free(slab);
    if (free_idx < 0) {
        spin_unlock(&cache->lock);
        return NULL;
    }

    {
        unsigned int byte_idx = (unsigned int)free_idx / 8;
        unsigned int bit_idx = (unsigned int)free_idx % 8;
        slab->bitmap[byte_idx] |= (1 << bit_idx);
    }

    slab->active_count++;

    if (slab->active_count >= slab->obj_count) {
        slab->full = 1;
        list_remove(&cache->slabs_partial, slab);
        if (slab == cache->slabs_free) {
            list_remove(&cache->slabs_free, slab);
        }
        list_push_front(&cache->slabs_full, slab);
    } else {
        if (slab->active_count == 1) {
            list_remove(&cache->slabs_free, slab);
            list_push_front(&cache->slabs_partial, slab);
        }
    }

    obj_ptr = (void *)((uint8_t *)slab->start_addr +
                         free_idx * cache->object_size);

    spin_unlock(&cache->lock);

    return obj_ptr;
}

void kmem_cache_free(KmemCache *cache, void *obj)
{
    Slab *slab;
    uintptr_t obj_addr, slab_start, slab_end;
    int obj_idx;

    if (!cache || !obj) {
        return;
    }

    spin_lock(&cache->lock);

    cache->total_frees++;

    obj_addr = (uintptr_t)obj;

    for (Slab *s = cache->slabs_full; s; s = s->next) {
        slab_start = (uintptr_t)s->start_addr;
        slab_end = slab_start + s->obj_count * cache->object_size;

        if (obj_addr >= slab_start && obj_addr < slab_end) {
            slab = s;
            break;
        }
    }

    if (!slab) {
        for (Slab *s = cache->slabs_partial; s; s = s->next) {
            slab_start = (uintptr_t)s->start_addr;
            slab_end = slab_start +
                       s->obj_count * cache->object_size;

            if (obj_addr >= slab_start && obj_addr < slab_end) {
                slab = s;
                break;
            }
        }
    }

    if (!slab) {
        spin_unlock(&cache->lock);
        return;
    }

    obj_idx = (int)((obj_addr - (uintptr_t)slab->start_addr) /
                     cache->object_size);

    if (obj_idx >= 0 && obj_idx < (int)slab->obj_count) {
        unsigned int byte_idx = (unsigned int)obj_idx / 8;
        unsigned int bit_idx = (unsigned int)obj_idx % 8;
        slab->bitmap[byte_idx] &= ~(1 << bit_idx);
        slab->active_count--;
    }

    if (slab->full) {
        slab->full = 0;
        list_remove(&cache->slabs_full, slab);
        list_push_front(&cache->slabs_partial, slab);
    }

    if (slab->active_count == 0) {
        list_remove(&cache->slabs_partial, slab);
        list_push_front(&cache->slabs_free, slab);
    }

    spin_unlock(&cache->lock);
}

void kmem_cache_destroy(KmemCache *cache)
{
    Slab *slab, *next;

    if (!cache) {
        return;
    }

    spin_lock(&cache->lock);

    slab = cache->slabs_full;
    while (slab) {
        next = slab->next;
        slab_destroy(slab);
        slab = next;
    }

    slab = cache->slabs_partial;
    while (slab) {
        next = slab->next;
        slab_destroy(slab);
        slab = next;
    }

    slab = cache->slabs_free;
    while (slab) {
        next = slab->next;
        slab_destroy(slab);
        slab = next;
    }

    spin_unlock(&cache->lock);

    kfree(cache);
}

/* ============================================================
 * 通用分配接口实现
 * ============================================================ */

void *slab_alloc(size_t size)
{
    int idx;

    if (!slab_initialized) {
        return kmalloc(size);
    }

    if (size == 0) {
        return NULL;
    }

    if (size > SLAB_MAX_OBJECT_SIZE) {
        return kmalloc(size);
    }

    idx = find_general_cache_index(size);
    if (idx < 0) {
        return kmalloc(size);
    }

    return kmem_cache_alloc(general_caches[idx]);
}

void slab_free(void *ptr)
{
    (void)ptr;

    if (!ptr || !slab_initialized) {
        return;
    }

    for (int i = 0; i < SLAB_GENERAL_CACHE_NUM; i++) {
        KmemCache *cache = general_caches[i];
        uintptr_t ptr_addr = (uintptr_t)ptr;

        spin_lock(&cache->lock);

        for (Slab *s = cache->slabs_full; s; s = s->next) {
            uintptr_t start = (uintptr_t)s->start_addr;
            uintptr_t end = start +
                           s->obj_count * cache->object_size;

            if (ptr_addr >= start && ptr_addr < end) {
                spin_unlock(&cache->lock);
                kmem_cache_free(cache, ptr);
                return;
            }
        }

        for (Slab *s = cache->slabs_partial; s; s = s->next) {
            uintptr_t start = (uintptr_t)s->start_addr;
            uintptr_t end = start +
                           s->obj_count * cache->object_size;

            if (ptr_addr >= start && ptr_addr < end) {
                spin_unlock(&cache->lock);
                kmem_cache_free(cache, ptr);
                return;
            }
        }

        spin_unlock(&cache->lock);
    }
}

/* ============================================================
 * 查询与调试接口实现
 * ============================================================ */

void kmem_cache_get_stats(KmemCache *cache,
                           unsigned int *total_objects,
                           unsigned int *active_objects,
                           unsigned int *total_slabs)
{
    if (!cache) {
        return;
    }

    spin_lock(&cache->lock);

    *total_objects = cache->objects_per_slab * cache->slab_count;
    *active_objects = 0;
    *total_slabs = cache->slab_count;

    for (Slab *s = cache->slabs_full; s; s = s->next) {
        *active_objects += s->active_count;
    }

    for (Slab *s = cache->slabs_partial; s; s = s->next) {
        *active_objects += s->active_count;
    }

    spin_unlock(&cache->lock);
}

void slab_dump_all_caches(void)
{
    unsigned long total_mem = 0, used_mem = 0;

    klog_mem("=== Slab Allocator Status ===");
    klog_mem("  General Caches:");

    for (int i = 0; i < SLAB_GENERAL_CACHE_NUM; i++) {
        KmemCache *cache = general_caches[i];
        unsigned int total_obj, active_obj, total_slabs;

        if (!cache) {
            continue;
        }

        kmem_cache_get_stats(cache, &total_obj, &active_obj, &total_slabs);

        klog_mem("    [%s] Size=%dB, Slabs=%d, Active=%d/%d",
                 cache->name, (uint32_t)cache->object_size,
                 total_slabs, active_obj, total_obj);

        total_mem += (unsigned long)total_obj * cache->object_size;
        used_mem += (unsigned long)active_obj * cache->object_size;
    }

    klog_mem("  Total: %dKB/%dKB used", (uint32_t)(used_mem / 1024), (uint32_t)(total_mem / 1024));
}

void kmem_cache_dump(KmemCache *cache)
{
    unsigned int total_obj, active_obj, total_slabs;

    if (!cache) {
        return;
    }

    kmem_cache_get_stats(cache, &total_obj, &active_obj, &total_slabs);

    klog_mem("--- Cache: %s ---", cache->name);
    klog_mem("  Object Size: %d bytes", (uint32_t)cache->object_size);
    klog_mem("  Objects/Slab: %d", cache->objects_per_slab);
    klog_mem("  Total Slabs: %d", total_slabs);
    klog_mem("  Active Objects: %d/%d", active_obj, total_obj);
    klog_mem("  Allocs/Frees: %d/%d", (uint32_t)cache->total_allocs, (uint32_t)cache->total_frees);
    klog_mem("  Hit/Miss Ratio: %d/%d", (uint32_t)cache->cache_hits, (uint32_t)cache->cache_misses);
}

void slab_get_system_stats(unsigned long *total_memory,
                            unsigned long *used_memory,
                            unsigned int *total_caches)
{
    *total_memory = 0;
    *used_memory = 0;
    *total_caches = 0;

    for (int i = 0; i < SLAB_GENERAL_CACHE_NUM; i++) {
        KmemCache *cache = general_caches[i];
        unsigned int total_obj, active_obj, total_slabs;

        if (!cache) {
            continue;
        }

        kmem_cache_get_stats(cache, &total_obj, &active_obj, &total_slabs);

        *total_memory += (unsigned long)total_obj * cache->object_size;
        *used_memory += (unsigned long)active_obj * cache->object_size;
        (*total_caches)++;
    }
}
