#include "idt.h"
#include "gdt.h"
#include "serial.h"
#include "io.h"

struct idt_entry idt[IDT_ENTRIES];
struct idt_ptr idt_ptr;

interrupt_handler_t interrupt_handlers[IDT_ENTRIES];

extern void idt_flush(uint64_t idt_ptr_addr);
extern void isr_common_stub(void);
extern void irq_common_stub(void);
extern void syscall_handler(void);

static const char *exception_messages[] = {
    "Division By Zero",
    "Debug",
    "Non Maskable Interrupt",
    "Breakpoint",
    "Into Detected Overflow",
    "Out of Bounds",
    "Invalid Opcode",
    "No Coprocessor",
    "Double Fault",
    "Coprocessor Segment Overrun",
    "Bad TSS",
    "Segment Not Present",
    "Stack Fault",
    "General Protection Fault",
    "Page Fault",
    "Unknown Interrupt",
    "Coprocessor Fault",
    "Alignment Check",
    "Machine Check",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved"
};

void idt_set_gate(uint8_t num, uint64_t handler, uint16_t selector, uint8_t type) {
    idt[num].offset_low = handler & 0xFFFF;
    idt[num].offset_mid = (handler >> 16) & 0xFFFF;
    idt[num].offset_high = (handler >> 32) & 0xFFFFFFFF;
    
    idt[num].selector = selector;
    idt[num].ist = 0;
    idt[num].type_attr = type;
    idt[num].reserved = 0;
}

void idt_set_handler(uint8_t num, interrupt_handler_t handler) {
    interrupt_handlers[num] = handler;
}

static void pic_remap(int offset1, int offset2) {
    outb(0x20, 0x11);
    io_wait();
    outb(0xA0, 0x11);
    io_wait();
    outb(0x21, offset1);
    io_wait();
    outb(0xA1, offset2);
    io_wait();
    outb(0x21, 0x04);
    io_wait();
    outb(0xA1, 0x02);
    io_wait();
    outb(0x21, 0x01);
    io_wait();
    outb(0xA1, 0x01);
    io_wait();
    outb(0x21, 0x0);
    io_wait();
    outb(0xA1, 0x0);
}

static void pic_send_eoi(uint8_t irq) {
    outb(0x20, 0x20);
    if (irq >= 8) {
        outb(0xA0, 0x20);
    }
}

void exception_handler(struct interrupt_frame *frame) {
    serial_puts(SERIAL_COM1, "\n!!! EXCEPTION: ");
    if (frame->int_no < 32) {
        serial_puts(SERIAL_COM1, exception_messages[frame->int_no]);
    } else {
        serial_puts(SERIAL_COM1, "Unknown");
    }
    serial_puts(SERIAL_COM1, " !!!\n");
    
    serial_puts(SERIAL_COM1, "Interrupt number: 0x");
    serial_put_hex(SERIAL_COM1, frame->int_no);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "Error code: 0x");
    serial_put_hex(SERIAL_COM1, frame->err_code);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "RIP: 0x");
    serial_put_hex(SERIAL_COM1, frame->rip);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "System halted.\n");
    
    while (1) {
        __asm__ volatile ("hlt");
    }
}

void irq_handler(struct interrupt_frame *frame) {
    uint8_t irq = frame->int_no - IRQ_BASE;
    
    if (interrupt_handlers[frame->int_no] != NULL) {
        interrupt_handlers[frame->int_no](frame);
    }
    
    pic_send_eoi(irq);
}

extern uint64_t isr_table[];
extern uint64_t irq_table[];

void idt_init(void) {
    idt_ptr.limit = sizeof(idt) - 1;
    idt_ptr.base = (uint64_t)&idt;

    for (int i = 0; i < IDT_ENTRIES; i++) {
        idt_set_gate(i, 0, 0, 0);
        interrupt_handlers[i] = NULL;
    }

    for (int i = 0; i < 32; i++) {
        idt_set_gate(i, isr_table[i], GDT_KERNEL_CODE, IDT_TYPE_INTERRUPT);
    }

    for (int i = 0; i < 16; i++) {
        idt_set_gate(IRQ_BASE + i, irq_table[i], GDT_KERNEL_CODE, IDT_TYPE_INTERRUPT);
    }
    
    idt_set_gate(0x80, (uint64_t)syscall_handler, GDT_KERNEL_CODE, IDT_TYPE_TRAP | IDT_DPL_USER);

    pic_remap(IRQ_BASE, IRQ_BASE + 8);

    idt_flush((uint64_t)&idt_ptr);
    
    serial_puts(SERIAL_COM1, "IDT initialized\n");
}
