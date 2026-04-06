#ifndef _IDT_H
#define _IDT_H

#include "types.h"

#define IDT_ENTRIES 256
#define IRQ_BASE    32

#define IDT_TYPE_INTERRUPT  0x8E
#define IDT_TYPE_TRAP       0x8F
#define IDT_DPL_USER        0x60

struct idt_entry {
    uint16_t offset_low;
    uint16_t selector;
    uint8_t  ist;
    uint8_t  type_attr;
    uint16_t offset_mid;
    uint32_t offset_high;
    uint32_t reserved;
} __attribute__((packed));

struct idt_ptr {
    uint16_t limit;
    uint64_t base;
} __attribute__((packed));

struct interrupt_frame {
    uint64_t r15, r14, r13, r12, r11, r10, r9, r8;
    uint64_t rdi, rsi, rbp, rbx, rdx, rcx, rax;
    uint64_t int_no, err_code;
    uint64_t rip, cs, rflags, rsp, ss;
} __attribute__((packed));

typedef void (*interrupt_handler_t)(struct interrupt_frame *frame);

void idt_init(void);
void idt_set_gate(uint8_t num, uint64_t handler, uint16_t selector, uint8_t type);
void idt_set_handler(uint8_t num, interrupt_handler_t handler);
void irq_handler(struct interrupt_frame *frame);

#endif
