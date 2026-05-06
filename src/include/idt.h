#ifndef _IDT_H
#define _IDT_H

#include "types.h"
#include "module_check.h"

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
    /*
     * WARNING: field order MUST match push sequence in isr_common_stub (isr.asm):
     *   push rax, rbx, rcx, rdx, rbp, rsi, rdi, r8, r9, r10, r11, r12, r13, r14, r15
     * frame fields (low addr→high): rax, rbx, rcx, rdx, rbp, rsi, rdi, r8..r15
     * Any change to either side must be mirrored in the other.
     */
    uint64_t r15, r14, r13, r12, r11, r10, r9, r8;
    uint64_t rdi, rsi, rbp, rdx, rcx, rbx, rax;
    uint64_t int_no, err_code;
    uint64_t rip, cs, rflags, rsp, ss;
} __attribute__((packed));

typedef void (*interrupt_handler_t)(struct interrupt_frame *frame);

typedef struct {
    interrupt_handler_t handler;
    const char *name;
    const char *description;
    uint32_t flags;
    uint64_t call_count;
    uint64_t error_count;
} interrupt_descriptor_t;

#define IRQ_FLAG_SHARED     0x01
#define IRQ_FLAG_EDGE       0x02
#define IRQ_FLAG_LEVEL      0x04

int idt_init(void);
void idt_set_gate(uint8_t num, uint64_t handler, uint16_t selector, uint8_t type);
int idt_set_handler(uint8_t num, interrupt_handler_t handler, const char *name);
int idt_register_irq(uint8_t irq, interrupt_handler_t handler, const char *name, uint32_t flags);
int idt_unregister_irq(uint8_t irq, interrupt_handler_t handler);
void idt_enable_irq(uint8_t irq);
void idt_disable_irq(uint8_t irq);

void exception_handler(struct interrupt_frame *frame);
void irq_handler(struct interrupt_frame *frame);

void idt_dump_state(void);
uint64_t idt_get_interrupt_count(uint8_t vector);
const char* idt_get_exception_name(uint8_t vector);
void idt_print_interrupt_stats(void);

#endif
