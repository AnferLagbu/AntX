#include "mm.h"
#include "kmalloc.h"
#include "serial.h"
#include "assert.h"
#include "string.h"

#define INITIAL_BITMAP_SIZE 4096

static uint32_t *bitmap = NULL;
static uint64_t bitmap_size = 0;
static uint64_t bitmap_capacity = 0;
static struct memory_info mem_info;

static int bitmap_initialized = 0;

static uint64_t early_alloc_end = 0;

static void set_bit(uint64_t index) {
    if (index >= bitmap_size * 32) return;
    bitmap[index / 32] |= (1 << (index % 32));
}

static void clear_bit(uint64_t index) {
    if (index >= bitmap_size * 32) return;
    bitmap[index / 32] &= ~(1 << (index % 32));
}

static int test_bit(uint64_t index) {
    if (index >= bitmap_size * 32) return 0;
    return bitmap[index / 32] & (1 << (index % 32));
}

static int expand_bitmap(uint64_t needed_pages) {
    uint64_t needed_bitmap_size = (needed_pages + 31) / 32;
    
    if (needed_bitmap_size <= bitmap_capacity) {
        bitmap_size = needed_bitmap_size;
        return 0;
    }
    
    uint64_t new_capacity = bitmap_capacity;
    if (new_capacity == 0) {
        new_capacity = INITIAL_BITMAP_SIZE;
    }
    
    while (new_capacity < needed_bitmap_size) {
        new_capacity *= 2;
    }
    
    uint32_t *new_bitmap = (uint32_t*)krealloc(bitmap, new_capacity * sizeof(uint32_t));
    if (new_bitmap == NULL) {
        new_bitmap = (uint32_t*)kmalloc(new_capacity * sizeof(uint32_t));
        if (new_bitmap == NULL) {
            serial_puts(SERIAL_COM1, "PMM: Failed to expand bitmap\n");
            return -1;
        }
        if (bitmap != NULL) {
            memcpy(new_bitmap, bitmap, bitmap_size * sizeof(uint32_t));
            kfree(bitmap);
        }
    }
    
    for (uint64_t i = bitmap_size; i < new_capacity; i++) {
        new_bitmap[i] = 0xFFFFFFFF;
    }
    
    bitmap = new_bitmap;
    bitmap_capacity = new_capacity;
    bitmap_size = needed_bitmap_size;
    
    return 0;
}

void pmm_init(uint64_t mem_size, uint64_t kernel_end) {
    mem_info.total_pages = mem_size / PAGE_SIZE;
    mem_info.free_pages = 0;
    mem_info.used_pages = 0;
    mem_info.kernel_end = kernel_end;
    
    early_alloc_end = (kernel_end + PAGE_SIZE - 1) & ~(PAGE_SIZE - 1);
    early_alloc_end += 16 * PAGE_SIZE;
    
    bitmap = NULL;
    bitmap_size = 0;
    bitmap_capacity = 0;
    bitmap_initialized = 0;
    
    serial_puts(SERIAL_COM1, "PMM: Memory size = ");
    serial_put_dec(SERIAL_COM1, mem_size / (1024 * 1024));
    serial_puts(SERIAL_COM1, " MB\n");
    serial_puts(SERIAL_COM1, "PMM: Total pages = ");
    serial_put_dec(SERIAL_COM1, mem_info.total_pages);
    serial_puts(SERIAL_COM1, "\n");
}

void pmm_init_bitmap(void) {
    if (bitmap_initialized) return;
    
    if (expand_bitmap(mem_info.total_pages) != 0) {
        serial_puts(SERIAL_COM1, "PMM: Failed to initialize bitmap\n");
        return;
    }
    
    for (uint64_t i = 0; i < bitmap_size; i++) {
        bitmap[i] = 0xFFFFFFFF;
    }
    
    uint64_t kernel_pages = (early_alloc_end + PAGE_SIZE - 1) / PAGE_SIZE;
    for (uint64_t i = 0; i < kernel_pages && i < mem_info.total_pages; i++) {
        clear_bit(i);
        mem_info.used_pages++;
    }
    
    mem_info.free_pages = mem_info.total_pages - mem_info.used_pages;
    bitmap_initialized = 1;
    
    serial_puts(SERIAL_COM1, "PMM initialized: ");
    serial_put_dec(SERIAL_COM1, mem_info.total_pages);
    serial_puts(SERIAL_COM1, " total pages, ");
    serial_put_dec(SERIAL_COM1, mem_info.free_pages);
    serial_puts(SERIAL_COM1, " free, ");
    serial_put_dec(SERIAL_COM1, mem_info.used_pages);
    serial_puts(SERIAL_COM1, " used\n");
    serial_puts(SERIAL_COM1, "PMM: Bitmap size = ");
    serial_put_dec(SERIAL_COM1, bitmap_size * sizeof(uint32_t));
    serial_puts(SERIAL_COM1, " bytes\n");
}

void* pmm_alloc_page(void) {
    if (!bitmap_initialized) {
        uint64_t page = early_alloc_end;
        early_alloc_end += PAGE_SIZE;
        return (void*)page;
    }
    
    for (uint64_t i = 0; i < bitmap_size * 32 && i < mem_info.total_pages; i++) {
        if (test_bit(i)) {
            clear_bit(i);
            mem_info.free_pages--;
            mem_info.used_pages++;
            return (void*)(i * PAGE_SIZE);
        }
    }
    
    serial_puts(SERIAL_COM1, "PMM: Out of memory\n");
    return NULL;
}

void pmm_free_page(void* addr) {
    if (addr == NULL || !bitmap_initialized) return;
    
    uint64_t index = (uint64_t)addr / PAGE_SIZE;
    if (index < mem_info.total_pages) {
        set_bit(index);
        mem_info.free_pages++;
        mem_info.used_pages--;
    }
}

uint64_t pmm_get_free_pages(void) {
    return mem_info.free_pages;
}

uint64_t pmm_get_total_pages(void) {
    return mem_info.total_pages;
}

uint64_t pmm_get_used_pages(void) {
    return mem_info.used_pages;
}

void* pmm_alloc_pages(size_t count) {
    if (count == 0) return NULL;
    if (!bitmap_initialized) {
        uint64_t page = early_alloc_end;
        early_alloc_end += count * PAGE_SIZE;
        return (void*)page;
    }
    if (count == 1) return pmm_alloc_page();
    
    uint64_t consecutive = 0;
    uint64_t start_index = 0;
    
    for (uint64_t i = 0; i < bitmap_size * 32 && i < mem_info.total_pages; i++) {
        if (test_bit(i)) {
            if (consecutive == 0) {
                start_index = i;
            }
            consecutive++;
            
            if (consecutive >= count) {
                for (uint64_t j = start_index; j < start_index + count; j++) {
                    clear_bit(j);
                    mem_info.free_pages--;
                    mem_info.used_pages++;
                }
                return (void*)(start_index * PAGE_SIZE);
            }
        } else {
            consecutive = 0;
        }
    }
    
    serial_puts(SERIAL_COM1, "PMM: Could not allocate ");
    serial_put_dec(SERIAL_COM1, count);
    serial_puts(SERIAL_COM1, " consecutive pages\n");
    return NULL;
}

void pmm_free_pages(void* addr, size_t count) {
    if (addr == NULL || count == 0 || !bitmap_initialized) return;
    
    uint64_t start_index = (uint64_t)addr / PAGE_SIZE;
    
    for (size_t i = 0; i < count && (start_index + i) < mem_info.total_pages; i++) {
        set_bit(start_index + i);
        mem_info.free_pages++;
        mem_info.used_pages--;
    }
}

void pmm_dump_stats(void) {
    serial_puts(SERIAL_COM1, "\n=== Physical Memory Stats ===\n");
    serial_puts(SERIAL_COM1, "  Total pages: ");
    serial_put_dec(SERIAL_COM1, mem_info.total_pages);
    serial_puts(SERIAL_COM1, "\n  Free pages: ");
    serial_put_dec(SERIAL_COM1, mem_info.free_pages);
    serial_puts(SERIAL_COM1, "\n  Used pages: ");
    serial_put_dec(SERIAL_COM1, mem_info.used_pages);
    serial_puts(SERIAL_COM1, "\n  Total memory: ");
    serial_put_dec(SERIAL_COM1, mem_info.total_pages * PAGE_SIZE / (1024 * 1024));
    serial_puts(SERIAL_COM1, " MB\n  Free memory: ");
    serial_put_dec(SERIAL_COM1, mem_info.free_pages * PAGE_SIZE / (1024 * 1024));
    serial_puts(SERIAL_COM1, " MB\n  Bitmap size: ");
    serial_put_dec(SERIAL_COM1, bitmap_size * sizeof(uint32_t));
    serial_puts(SERIAL_COM1, " bytes\n=============================\n");
}
