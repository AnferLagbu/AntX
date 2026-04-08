#include "mm.h"
#include "serial.h"
#include "assert.h"

#define BITMAP_SIZE 32768

static uint32_t bitmap[BITMAP_SIZE];
static struct memory_info mem_info;

static void set_bit(uint64_t index) {
    ASSERT(index < BITMAP_SIZE * 32);
    bitmap[index / 32] |= (1 << (index % 32));
}

static void clear_bit(uint64_t index) {
    ASSERT(index < BITMAP_SIZE * 32);
    bitmap[index / 32] &= ~(1 << (index % 32));
}

static int test_bit(uint64_t index) {
    ASSERT(index < BITMAP_SIZE * 32);
    return bitmap[index / 32] & (1 << (index % 32));
}

void pmm_init(uint64_t mem_size, uint64_t kernel_end) {
    mem_info.total_pages = mem_size / PAGE_SIZE;
    mem_info.free_pages = mem_info.total_pages;
    mem_info.used_pages = 0;
    mem_info.kernel_end = kernel_end;
    
    for (uint64_t i = 0; i < BITMAP_SIZE; i++) {
        bitmap[i] = 0xFFFFFFFF;
    }
    
    uint64_t kernel_pages = (kernel_end + PAGE_SIZE - 1) / PAGE_SIZE;
    for (uint64_t i = 0; i < kernel_pages; i++) {
        clear_bit(i);
        mem_info.free_pages--;
        mem_info.used_pages++;
    }
    
    serial_puts(SERIAL_COM1, "PMM initialized: ");
    serial_put_dec(SERIAL_COM1, mem_info.total_pages);
    serial_puts(SERIAL_COM1, " total pages, ");
    serial_put_dec(SERIAL_COM1, mem_info.free_pages);
    serial_puts(SERIAL_COM1, " free\n");
}

void* pmm_alloc_page(void) {
    for (uint64_t i = 0; i < BITMAP_SIZE * 32; i++) {
        if (test_bit(i)) {
            clear_bit(i);
            mem_info.free_pages--;
            mem_info.used_pages++;
            return (void*)(i * PAGE_SIZE);
        }
    }
    return NULL;
}

void pmm_free_page(void* addr) {
    if (addr == NULL) return;
    
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

void* pmm_alloc_pages(size_t count) {
    if (count == 0) return NULL;
    if (count == 1) return pmm_alloc_page();
    
    uint64_t consecutive = 0;
    uint64_t start_index = 0;
    
    for (uint64_t i = 0; i < BITMAP_SIZE * 32; i++) {
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
    
    return NULL;
}
