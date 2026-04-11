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
    serial_puts(SERIAL_COM1, "\n");
    serial_puts(SERIAL_COM1, "========================================\n");
    serial_puts(SERIAL_COM1, "!!! EXCEPTION: ");
    if (frame->int_no < 32) {
        serial_puts(SERIAL_COM1, exception_messages[frame->int_no]);
    } else {
        serial_puts(SERIAL_COM1, "Unknown");
    }
    serial_puts(SERIAL_COM1, " !!!\n");
    serial_puts(SERIAL_COM1, "========================================\n");
    
    serial_puts(SERIAL_COM1, "  Interrupt: 0x");
    serial_put_hex(SERIAL_COM1, frame->int_no);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "  Error Code: 0x");
    serial_put_hex(SERIAL_COM1, frame->err_code);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "  RIP: 0x");
    serial_put_hex(SERIAL_COM1, frame->rip);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "  CS:  0x");
    serial_put_hex(SERIAL_COM1, frame->cs);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "  RFLAGS: 0x");
    serial_put_hex(SERIAL_COM1, frame->rflags);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "  RSP: 0x");
    serial_put_hex(SERIAL_COM1, frame->rsp);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "  SS:  0x");
    serial_put_hex(SERIAL_COM1, frame->ss);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "\n  Registers:\n");
    serial_puts(SERIAL_COM1, "    RAX: 0x");
    serial_put_hex(SERIAL_COM1, frame->rax);
    serial_puts(SERIAL_COM1, "  RBX: 0x");
    serial_put_hex(SERIAL_COM1, frame->rbx);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "    RCX: 0x");
    serial_put_hex(SERIAL_COM1, frame->rcx);
    serial_puts(SERIAL_COM1, "  RDX: 0x");
    serial_put_hex(SERIAL_COM1, frame->rdx);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "    RSI: 0x");
    serial_put_hex(SERIAL_COM1, frame->rsi);
    serial_puts(SERIAL_COM1, "  RDI: 0x");
    serial_put_hex(SERIAL_COM1, frame->rdi);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "    RBP: 0x");
    serial_put_hex(SERIAL_COM1, frame->rbp);
    serial_puts(SERIAL_COM1, "  R8:  0x");
    serial_put_hex(SERIAL_COM1, frame->r8);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "    R9:  0x");
    serial_put_hex(SERIAL_COM1, frame->r9);
    serial_puts(SERIAL_COM1, "  R10: 0x");
    serial_put_hex(SERIAL_COM1, frame->r10);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "    R11: 0x");
    serial_put_hex(SERIAL_COM1, frame->r11);
    serial_puts(SERIAL_COM1, "  R12: 0x");
    serial_put_hex(SERIAL_COM1, frame->r12);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "    R13: 0x");
    serial_put_hex(SERIAL_COM1, frame->r13);
    serial_puts(SERIAL_COM1, "  R14: 0x");
    serial_put_hex(SERIAL_COM1, frame->r14);
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "    R15: 0x");
    serial_put_hex(SERIAL_COM1, frame->r15);
    serial_puts(SERIAL_COM1, "\n");
    
    if (frame->int_no == 14) {
        uint64_t fault_addr;
        __asm__ volatile ("mov %%cr2, %0" : "=r"(fault_addr));
        
        serial_puts(SERIAL_COM1, "\n  Page Fault Details:\n");
        serial_puts(SERIAL_COM1, "    Fault Address (CR2): 0x");
        serial_put_hex(SERIAL_COM1, fault_addr);
        serial_puts(SERIAL_COM1, "\n");
        
        serial_puts(SERIAL_COM1, "    Access Type: ");
        if (frame->err_code & 0x02) {
            serial_puts(SERIAL_COM1, "Write\n");
        } else {
            serial_puts(SERIAL_COM1, "Read\n");
        }
        
        serial_puts(SERIAL_COM1, "    Mode: ");
        if (frame->err_code & 0x04) {
            serial_puts(SERIAL_COM1, "User\n");
        } else {
            serial_puts(SERIAL_COM1, "Kernel\n");
        }
        
        serial_puts(SERIAL_COM1, "    Cause: ");
        if (frame->err_code & 0x01) {
            serial_puts(SERIAL_COM1, "Protection Violation\n");
        } else {
            serial_puts(SERIAL_COM1, "Page Not Present\n");
        }
        
        if (frame->err_code & 0x08) {
            serial_puts(SERIAL_COM1, "    Reserved bit set in page table\n");
        }
        
        if (frame->err_code & 0x10) {
            serial_puts(SERIAL_COM1, "    Instruction fetch\n");
        }
    }
    
    if (frame->int_no == 13) {
        serial_puts(SERIAL_COM1, "\n  General Protection Fault Details:\n");
        serial_puts(SERIAL_COM1, "    Segment Selector: 0x");
        serial_put_hex(SERIAL_COM1, frame->err_code & 0xFFFF);
        serial_puts(SERIAL_COM1, "\n");
        
        if (frame->err_code & 0x01) {
            serial_puts(SERIAL_COM1, "    External event\n");
        }
        if (frame->err_code & 0x02) {
            serial_puts(SERIAL_COM1, "    IDT flag set\n");
        }
        if (frame->err_code & 0x04) {
            serial_puts(SERIAL_COM1, "    LDT flag set\n");
        }
    }
    
    serial_puts(SERIAL_COM1, "\n========================================\n");
    
    int is_user_mode = (frame->cs & 0x03) == 3;
    
    if (is_user_mode) {
        serial_puts(SERIAL_COM1, "User process crashed. Killing process.\n");
        
        extern struct process* process_get_current(void);
        extern void process_exit(struct process *proc, uint64_t exit_code);
        
        struct process *current = process_get_current();
        if (current != NULL) {
            process_exit(current, 1);
        }
        
        extern void scheduler_yield(void);
        scheduler_yield();
        
        return;
    }
    
    serial_puts(SERIAL_COM1, "Kernel panic! System halted.\n");
    
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

int idt_init(void) {
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
    
    return MODULE_INIT_SUCCESS;
}
