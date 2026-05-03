#ifndef _MM_H
#define _MM_H

#include "types.h"

#define PAGE_SIZE       4096
#define PAGE_SHIFT      12

/* 大页大小定义 */
#define HUGE_PAGE_2M_SIZE   (2 * 1024 * 1024)    /* 2 MB */
#define HUGE_PAGE_1G_SIZE   (1024 * 1024 * 1024) /* 1 GB */
#define HUGE_PAGE_2M_SHIFT  21
#define HUGE_PAGE_1G_SHIFT  30

#define KERNEL_BASE     0xFFFF800000000000ULL
#define PHYSICAL_BASE   0x0000000000000000ULL

#define PAGE_PRESENT    (1 << 0)
#define PAGE_WRITABLE   (1 << 1)
#define PAGE_USER       (1 << 2)
#define PAGE_HUGE       (1 << 7)               /* 大页标志 */
#define PAGE_NX         (1ULL << 63)

#define PML4_INDEX(addr) (((addr) >> 39) & 0x1FF)
#define PDPT_INDEX(addr) (((addr) >> 30) & 0x1FF)
#define PD_INDEX(addr)   (((addr) >> 21) & 0x1FF)
#define PT_INDEX(addr)   (((addr) >> 12) & 0x1FF)

/**
 * @brief 页面大小类型
 */
typedef enum {
    PAGE_SIZE_4K = 0,     /**< 标准 4KB 页面 */
    PAGE_SIZE_2M = 1,     /**< 2MB 大页 */
    PAGE_SIZE_1G = 2      /**< 1GB 巨页 */
} page_size_t;

#define KERNEL_CODE_START  0xFFFF800000000000ULL
#define KERNEL_CODE_END    0xFFFF8000FFFFFFFFULL

struct page_table_entry {
    uint64_t present    : 1;   /**< 位 0: Present */
    uint64_t rw         : 1;   /**< 位 1: Read/Write */
    uint64_t user       : 1;   /**< 位 2: User/Supervisor */
    uint64_t pwt        : 1;   /**< 位 3: Page-Level Write-Through */
    uint64_t pcd        : 1;   /**< 位 4: Page-Level Cache Disable */
    uint64_t accessed   : 1;   /**< 位 5: Accessed */
    uint64_t dirty      : 1;   /**< 位 6: Dirty */
    uint64_t ps         : 1;   /**< 位 7: Page Size (0=4KB, 1=2MB/1GB) */
    uint64_t pat        : 1;   /**< 位 12: PAT (仅 4KB 页) */
    uint64_t global     : 1;   /**< 位 8: Global (CR4.PGE 必须设置) */
    uint64_t available  : 3;   /**< 位 9-11: Available for OS use */
    uint64_t frame      : 40;
    uint64_t reserved   : 11;
    uint64_t xd         : 1;
} __attribute__((packed));

union pte_union {
    struct page_table_entry fields;
    uint64_t value;
};

typedef union pte_union pte_t;

struct memory_info {
    uint64_t total_pages;
    uint64_t free_pages;
    uint64_t used_pages;
    uint64_t kernel_end;
};

void pmm_init(uint64_t mem_size, uint64_t kernel_end);
void pmm_init_bitmap(void);
void* pmm_alloc_page(void);
void pmm_free_page(void* addr);
uint64_t pmm_get_free_pages(void);
uint64_t pmm_get_total_pages(void);
uint64_t pmm_get_used_pages(void);
void* pmm_alloc_pages(size_t count);
void pmm_free_pages(void* addr, size_t count);
void pmm_dump_stats(void);

/* 大页分配接口 */
void* pmm_alloc_huge_page(page_size_t size_type);  /**< 分配 2MB 或 1GB 连续物理页 */
void  pmm_free_huge_page(void* addr, page_size_t size_type);
int   pmm_is_aligned_for_huge(void* addr, page_size_t size_type);

void vmm_init(void);
int vmm_map_page(uint64_t virt, uint64_t phys, uint64_t flags);
int vmm_map_huge_page(uint64_t virt, uint64_t phys, uint64_t flags, page_size_t size_type);
void vmm_unmap_page(uint64_t virt);
uint64_t vmm_get_physical(uint64_t virt);
uint64_t vmm_get_physical_in_table(uint64_t pml4, uint64_t virt);
void vmm_switch_page_table(uint64_t cr3);

uint64_t vmm_create_user_page_table(void);
void vmm_map_page_in_table(uint64_t pml4, uint64_t virt, uint64_t phys, uint64_t flags);
void vmm_destroy_page_table(uint64_t pml4);

extern uint64_t kernel_pml4;

#endif
