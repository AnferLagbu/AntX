#ifndef _KMALLOC_H
#define _KMALLOC_H

#include "types.h"

#define HEAP_MAGIC  0xDEADBEEFCAFEBABEULL

struct heap_header {
    uint64_t magic;
    uint64_t size;
    int free;
    struct heap_header *next;
    struct heap_header *prev;
};

struct kmalloc_stats {
    uint64_t heap_start;
    uint64_t heap_end;
    uint64_t heap_size;
    uint64_t allocated;
    uint64_t free;
    uint64_t overhead;
};

void kmalloc_init(void);
void* kmalloc(uint64_t size);
void kfree(void *ptr);
void* krealloc(void *ptr, uint64_t size);
void* kcalloc(uint64_t num, uint64_t size);

void kmalloc_stats(struct kmalloc_stats *stats);
void kmalloc_dump(void);

#endif
