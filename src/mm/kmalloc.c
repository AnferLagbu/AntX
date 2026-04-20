#include "kmalloc.h"
#include "mm.h"
#include "serial.h"
#include "assert.h"
#include "string.h"

#define HEAP_START          0xFFFF800001000000ULL
#define HEAP_MAX_SIZE       (256 * 1024 * 1024)
#define MIN_BLOCK_SIZE      16
#define ALIGN_SIZE          8

#define ALIGN_UP(x, align)  (((x) + (align) - 1) & ~((align) - 1))
#define ALIGN_DOWN(x, align) ((x) & ~((align) - 1))

#define EARLY_HEAP_PAGES    16

static struct heap_header *free_list = NULL;
static uint64_t heap_current = HEAP_START;
static uint64_t heap_end = HEAP_START;
static uint64_t heap_allocated = 0;
static uint64_t heap_free_total = 0;

static int kmalloc_initialized = 0;
static uint64_t early_heap_phys[EARLY_HEAP_PAGES];
static int early_heap_used = 0;

static void heap_expand(uint64_t min_size) {
    uint64_t pages_needed = ALIGN_UP(min_size, PAGE_SIZE) / PAGE_SIZE;
    uint64_t pages_available = (HEAP_MAX_SIZE - (heap_current - HEAP_START)) / PAGE_SIZE;
    
    if (pages_needed > pages_available) {
        pages_needed = pages_available;
    }
    
    if (pages_needed == 0) return;
    
    for (uint64_t i = 0; i < pages_needed; i++) {
        void *page = pmm_alloc_page();
        if (page == NULL) {
            serial_puts(SERIAL_COM1, "kmalloc: out of physical memory\n");
            return;
        }
        
        vmm_map_page(heap_current, (uint64_t)page, PAGE_PRESENT | PAGE_WRITABLE);
        heap_current += PAGE_SIZE;
    }
    
    heap_end = heap_current;
    
    struct heap_header *new_block = (struct heap_header *)(heap_end - pages_needed * PAGE_SIZE);
    new_block->size = pages_needed * PAGE_SIZE - sizeof(struct heap_header);
    new_block->free = 1;
    new_block->magic = HEAP_MAGIC;
    new_block->next = free_list;
    new_block->prev = NULL;
    
    if (free_list != NULL) {
        free_list->prev = new_block;
    }
    free_list = new_block;
    
    heap_free_total += new_block->size;
}

static void early_heap_init(void) {
    if (early_heap_used > 0) return;
    
    extern char _kernel_end_phys[];
    uint64_t kernel_end = (uint64_t)_kernel_end_phys;
    uint64_t heap_phys_start = (kernel_end + PAGE_SIZE - 1) & ~(PAGE_SIZE - 1);
    
    for (int i = 0; i < EARLY_HEAP_PAGES; i++) {
        early_heap_phys[i] = heap_phys_start + i * PAGE_SIZE;
    }
    early_heap_used = 0;
    
    for (int i = 0; i < EARLY_HEAP_PAGES; i++) {
        vmm_map_page(heap_current, early_heap_phys[i], PAGE_PRESENT | PAGE_WRITABLE);
        heap_current += PAGE_SIZE;
    }
    
    heap_end = heap_current;
    
    struct heap_header *new_block = (struct heap_header *)(HEAP_START);
    new_block->size = EARLY_HEAP_PAGES * PAGE_SIZE - sizeof(struct heap_header);
    new_block->free = 1;
    new_block->magic = HEAP_MAGIC;
    new_block->next = NULL;
    new_block->prev = NULL;
    free_list = new_block;
    
    heap_free_total += new_block->size;
}

void kmalloc_init(void) {
    free_list = NULL;
    heap_current = HEAP_START;
    heap_end = HEAP_START;
    heap_allocated = 0;
    heap_free_total = 0;
    
    early_heap_init();
    
    kmalloc_initialized = 1;
    
    serial_puts(SERIAL_COM1, "kmalloc: heap initialized at 0x");
    serial_put_hex(SERIAL_COM1, HEAP_START);
    serial_puts(SERIAL_COM1, "\n");
}

static void coalesce_forward(struct heap_header *block) {
    if (block == NULL || block->next == NULL) return;
    if (!block->free || !block->next->free) return;
    
    struct heap_header *next = block->next;
    
    if ((uint8_t*)block + sizeof(struct heap_header) + block->size != (uint8_t*)next) {
        return;
    }
    
    block->size += sizeof(struct heap_header) + next->size;
    block->next = next->next;
    
    if (next->next != NULL) {
        next->next->prev = block;
    }
    
    next->magic = 0;
}

static void coalesce_backward(struct heap_header *block) {
    if (block == NULL || block->prev == NULL) return;
    if (!block->free || !block->prev->free) return;
    
    struct heap_header *prev = block->prev;
    
    if ((uint8_t*)prev + sizeof(struct heap_header) + prev->size != (uint8_t*)block) {
        return;
    }
    
    prev->size += sizeof(struct heap_header) + block->size;
    prev->next = block->next;
    
    if (block->next != NULL) {
        block->next->prev = prev;
    }
    
    block->magic = 0;
}

void* kmalloc(uint64_t size) {
    if (size == 0) return NULL;
    
    size = ALIGN_UP(size, ALIGN_SIZE);
    if (size < MIN_BLOCK_SIZE) {
        size = MIN_BLOCK_SIZE;
    }
    
    struct heap_header *current = free_list;
    struct heap_header *best = NULL;
    
    while (current != NULL) {
        if (current->free && current->size >= size) {
            if (best == NULL || current->size < best->size) {
                best = current;
                if (current->size == size) break;
            }
        }
        current = current->next;
    }
    
    if (best == NULL) {
        uint64_t needed = size + sizeof(struct heap_header);
        heap_expand(needed);
        
        current = free_list;
        while (current != NULL) {
            if (current->free && current->size >= size) {
                best = current;
                break;
            }
            current = current->next;
        }
        
        if (best == NULL) {
            serial_puts(SERIAL_COM1, "kmalloc: out of heap memory\n");
            return NULL;
        }
    }
    
    if (best->size > size + sizeof(struct heap_header) + MIN_BLOCK_SIZE) {
        struct heap_header *new_block = (struct heap_header*)((uint8_t*)best + sizeof(struct heap_header) + size);
        new_block->size = best->size - size - sizeof(struct heap_header);
        new_block->free = 1;
        new_block->magic = HEAP_MAGIC;
        new_block->next = best->next;
        new_block->prev = best;
        
        if (best->next != NULL) {
            best->next->prev = new_block;
        }
        best->next = new_block;
        best->size = size;
        
        heap_free_total -= sizeof(struct heap_header);
    }
    
    best->free = 0;
    heap_allocated += best->size;
    heap_free_total -= best->size;
    
    return (void*)((uint8_t*)best + sizeof(struct heap_header));
}

void kfree(void *ptr) {
    if (ptr == NULL) return;
    
    struct heap_header *block = (struct heap_header*)((uint8_t*)ptr - sizeof(struct heap_header));
    
    if (block->magic != HEAP_MAGIC) {
        serial_puts(SERIAL_COM1, "kfree: invalid magic\n");
        return;
    }
    
    if (block->free) {
        serial_puts(SERIAL_COM1, "kfree: double free\n");
        return;
    }
    
    block->free = 1;
    heap_allocated -= block->size;
    heap_free_total += block->size;
    
    coalesce_forward(block);
    coalesce_backward(block);
}

void* krealloc(void *ptr, uint64_t size) {
    if (ptr == NULL) return kmalloc(size);
    if (size == 0) {
        kfree(ptr);
        return NULL;
    }
    
    struct heap_header *block = (struct heap_header*)((uint8_t*)ptr - sizeof(struct heap_header));
    
    if (block->magic != HEAP_MAGIC) {
        return NULL;
    }
    
    uint64_t aligned_size = ALIGN_UP(size, ALIGN_SIZE);
    if (aligned_size < MIN_BLOCK_SIZE) {
        aligned_size = MIN_BLOCK_SIZE;
    }
    
    if (block->size >= aligned_size) {
        return ptr;
    }
    
    void *new_ptr = kmalloc(size);
    if (new_ptr == NULL) return NULL;
    
    uint64_t copy_size = block->size < size ? block->size : size;
    memcpy(new_ptr, ptr, copy_size);
    kfree(ptr);
    
    return new_ptr;
}

void* kcalloc(uint64_t num, uint64_t size) {
    uint64_t total = num * size;
    void *ptr = kmalloc(total);
    if (ptr != NULL) {
        memset(ptr, 0, total);
    }
    return ptr;
}

void kmalloc_stats(struct kmalloc_stats *stats) {
    if (stats == NULL) return;
    
    stats->heap_start = HEAP_START;
    stats->heap_end = heap_end;
    stats->heap_size = heap_end - HEAP_START;
    stats->allocated = heap_allocated;
    stats->free = heap_free_total;
    stats->overhead = (heap_end - HEAP_START) - heap_allocated - heap_free_total;
}

void kmalloc_dump(void) {
    serial_puts(SERIAL_COM1, "\n=== Kernel Heap Stats ===\n");
    serial_puts(SERIAL_COM1, "  Heap start: 0x");
    serial_put_hex(SERIAL_COM1, HEAP_START);
    serial_puts(SERIAL_COM1, "\n  Heap end: 0x");
    serial_put_hex(SERIAL_COM1, heap_end);
    serial_puts(SERIAL_COM1, "\n  Heap size: ");
    serial_put_dec(SERIAL_COM1, heap_end - HEAP_START);
    serial_puts(SERIAL_COM1, " bytes\n  Allocated: ");
    serial_put_dec(SERIAL_COM1, heap_allocated);
    serial_puts(SERIAL_COM1, " bytes\n  Free: ");
    serial_put_dec(SERIAL_COM1, heap_free_total);
    serial_puts(SERIAL_COM1, " bytes\n");
    
    int block_count = 0;
    struct heap_header *current = free_list;
    while (current != NULL) {
        block_count++;
        current = current->next;
    }
    
    serial_puts(SERIAL_COM1, "  Total blocks: ");
    serial_put_dec(SERIAL_COM1, block_count);
    serial_puts(SERIAL_COM1, "\n=========================\n");
}
