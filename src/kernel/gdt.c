#include "gdt.h"
#include "serial.h"
#include "string.h"

struct gdt_entry gdt[GDT_ENTRIES];
struct gdt_ptr gdt_ptr;
struct tss_entry tss;

extern void gdt_flush(uint64_t gdt_ptr_addr);
extern void tss_flush(void);

extern char stack_top[];

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
    uint32_t base_low = (uint32_t)(tss_addr & 0xFFFFFFFF);
    uint32_t limit = sizeof(struct tss_entry) - 1;
    
    gdt[num].limit_low = (limit & 0xFFFF);
    gdt[num].base_low = (base_low & 0xFFFF);
    gdt[num].base_middle = (base_low >> 16) & 0xFF;
    gdt[num].base_high = (base_low >> 24) & 0xFF;
    gdt[num].granularity = 0x00;
    gdt[num].access = 0x89;
    
    uint32_t base_high32 = (uint32_t)(tss_addr >> 32);
    gdt[num + 1].limit_low = (base_high32 & 0xFFFF);
    gdt[num + 1].base_low = (base_high32 >> 16) & 0xFFFF;
    gdt[num + 1].base_middle = 0;
    gdt[num + 1].base_high = 0;
    gdt[num + 1].granularity = 0;
    gdt[num + 1].access = 0;
}

void tss_set_kernel_stack(uint64_t rsp0) {
    tss.rsp0 = rsp0;
    serial_puts(SERIAL_COM1, "[TSS] Set rsp0=0x");
    serial_put_hex(SERIAL_COM1, rsp0);
    serial_puts(SERIAL_COM1, " (readback=0x");
    serial_put_hex(SERIAL_COM1, tss.rsp0);
    serial_puts(SERIAL_COM1, ")\n");
    serial_puts(SERIAL_COM1, "[TSS] TSS addr=0x");
    serial_put_hex(SERIAL_COM1, (uint64_t)&tss);
    serial_puts(SERIAL_COM1, " rsp0 offset=0x");
    serial_put_hex(SERIAL_COM1, (uint64_t)&tss.rsp0 - (uint64_t)&tss);
    serial_puts(SERIAL_COM1, "\n");
    
    extern struct gdt_entry gdt[];
    serial_puts(SERIAL_COM1, "[TSS] GDT[5] = 0x");
    serial_put_hex(SERIAL_COM1, ((uint64_t*)&gdt[5])[0]);
    serial_puts(SERIAL_COM1, " 0x");
    serial_put_hex(SERIAL_COM1, ((uint64_t*)&gdt[6])[0]);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "[TSS] TSS first 16 bytes: ");
    uint8_t* tss_bytes = (uint8_t*)&tss;
    for (int i = 0; i < 16; i++) {
        uint8_t b = tss_bytes[i];
        char hex[3];
        hex[0] = "0123456789ABCDEF"[b >> 4];
        hex[1] = "0123456789ABCDEF"[b & 0xF];
        hex[2] = '\0';
        serial_puts(SERIAL_COM1, hex);
        serial_puts(SERIAL_COM1, " ");
    }
    serial_puts(SERIAL_COM1, "\n");
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
    tss.rsp0 = (uint64_t)stack_top;
    tss.iomap_base = sizeof(struct tss_entry);
    
    tss_set_gate(5, (uint64_t)&tss);

    serial_puts(SERIAL_COM1, "[GDT] Before gdt_flush\n");
    gdt_flush((uint64_t)&gdt_ptr);
    serial_puts(SERIAL_COM1, "[GDT] After gdt_flush, before tss_flush\n");
    tss_flush();
    serial_puts(SERIAL_COM1, "[GDT] After tss_flush, returning success\n");
    
    return MODULE_INIT_SUCCESS;
}
