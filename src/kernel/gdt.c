#include "gdt.h"
#include "serial.h"
#include "string.h"

struct gdt_entry gdt[GDT_ENTRIES];
struct gdt_ptr gdt_ptr;
struct tss_entry tss;

extern void gdt_flush(uint64_t gdt_ptr_addr);
extern void tss_flush(void);

void gdt_set_gate(uint8_t num, uint32_t base, uint32_t limit, uint8_t access, uint8_t gran) {
    gdt[num].base_low = (base & 0xFFFF);
    gdt[num].base_middle = (base >> 16) & 0xFF;
    gdt[num].base_high = (base >> 24) & 0xFF;

    gdt[num].limit_low = (limit & 0xFFFF);
    gdt[num].granularity = (limit >> 16) & 0x0F;
    gdt[num].granularity |= gran & 0xF0;

    gdt[num].access = access;
}

void tss_set_gate(uint8_t num, uint64_t tss_addr) {
    uint32_t base = (uint32_t)tss_addr;
    uint32_t limit = sizeof(struct tss_entry) - 1;
    
    gdt[num].base_low = (base & 0xFFFF);
    gdt[num].base_middle = (base >> 16) & 0xFF;
    gdt[num].base_high = (base >> 24) & 0xFF;
    
    gdt[num].limit_low = (limit & 0xFFFF);
    gdt[num].granularity = 0x00;
    
    gdt[num].access = 0x89;
    
    gdt[num + 1].base_low = (uint16_t)(tss_addr >> 32);
    gdt[num + 1].base_middle = 0;
    gdt[num + 1].base_high = 0;
    gdt[num + 1].limit_low = 0;
    gdt[num + 1].granularity = 0;
    gdt[num + 1].access = 0;
}

void tss_set_kernel_stack(uint64_t rsp0) {
    tss.rsp0 = rsp0;
}

int gdt_init(void) {
    gdt_ptr.limit = sizeof(gdt) - 1;
    gdt_ptr.base = (uint64_t)&gdt;

    memset(&gdt, 0, sizeof(gdt));
    
    gdt_set_gate(0, 0, 0, 0, 0);
    gdt_set_gate(1, 0, 0xFFFFFFFF, 0x9A, 0xAF);
    gdt_set_gate(2, 0, 0xFFFFFFFF, 0x92, 0xCF);
    gdt_set_gate(3, 0, 0xFFFFFFFF, 0xFA, 0xAF);
    gdt_set_gate(4, 0, 0xFFFFFFFF, 0xF2, 0xCF);
    
    memset(&tss, 0, sizeof(tss));
    tss.rsp0 = 0;
    tss.iomap_base = sizeof(struct tss_entry);
    
    tss_set_gate(5, (uint64_t)&tss);

    gdt_flush((uint64_t)&gdt_ptr);
    tss_flush();
    
    return MODULE_INIT_SUCCESS;
}
