#ifndef _MM_H
#define _MM_H

#include "types.h"

#define PAGE_SIZE       4096
#define PAGE_SHIFT      12

#define KERNEL_BASE     0xFFFF800000000000ULL
#define PHYSICAL_BASE   0x0000000000000000ULL

#define PAGE_PRESENT    (1 << 0)
#define PAGE_WRITABLE   (1 << 1)
#define PAGE_USER       (1 << 2)
#define PAGE_HUGE       (1 << 7)
#define PAGE_NX         (1ULL << 63)

#define PML4_INDEX(addr) (((addr) >> 39) & 0x1FF)
#define PDPT_INDEX(addr) (((addr) >> 30) & 0x1FF)
#define PD_INDEX(addr)   (((addr) >> 21) & 0x1FF)
#define PT_INDEX(addr)   (((addr) >> 12) & 0x1FF)

#define KERNEL_CODE_START  0xFFFF800000000000ULL
#define KERNEL_CODE_END    0xFFFF8000FFFFFFFFULL

struct page_table_entry {
    uint64_t present    : 1;
    uint64_t rw         : 1;
    uint64_t user       : 1;
    uint64_t pwt        : 1;
    uint64_t pcd        : 1;
    uint64_t accessed   : 1;
    uint64_t dirty      : 1;
    uint64_t pat        : 1;
    uint64_t global     : 1;
    uint64_t available  : 3;
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
void* pmm_alloc_page(void);
void pmm_free_page(void* addr);
uint64_t pmm_get_free_pages(void);
void* pmm_alloc_pages(size_t count);

void vmm_init(void);
void vmm_map_page(uint64_t virt, uint64_t phys, uint64_t flags);
void vmm_unmap_page(uint64_t virt);
uint64_t vmm_get_physical(uint64_t virt);
uint64_t vmm_get_physical_in_table(uint64_t pml4, uint64_t virt);
void vmm_switch_page_table(uint64_t cr3);

uint64_t vmm_create_user_page_table(void);
void vmm_map_page_in_table(uint64_t pml4, uint64_t virt, uint64_t phys, uint64_t flags);
void vmm_destroy_page_table(uint64_t pml4);

extern uint64_t kernel_pml4;

#endif
