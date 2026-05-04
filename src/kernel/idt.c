#include "idt.h"
#include "gdt.h"
#include "serial.h"
#include "io.h"

struct idt_entry idt[IDT_ENTRIES];
struct idt_ptr idt_ptr;

interrupt_handler_t interrupt_handlers[IDT_ENTRIES];
static interrupt_descriptor_t irq_descriptors[16];

static uint64_t nested_interrupt_count = 0;
static uint64_t current_interrupt_vector = 0xFFFFFFFFFFFFFFFFULL;
static uint64_t exception_counts[32] = {0};
static uint64_t irq_counts[16] = {0};

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
    "SIMD Floating-Point Exception",
    "Virtualization Exception",
    "Control Protection Exception",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Hypervisor Injection Exception",
    "VMM Communication Exception",
    "Security Exception"
};

void idt_set_gate(uint8_t num, uint64_t handler, uint16_t selector, uint8_t type) {
    if (num >= IDT_ENTRIES) return;

    idt[num].offset_low = handler & 0xFFFF;
    idt[num].offset_mid = (handler >> 16) & 0xFFFF;
    idt[num].offset_high = (handler >> 32) & 0xFFFFFFFF;

    idt[num].selector = selector;
    idt[num].ist = 0;
    idt[num].type_attr = type;
    idt[num].reserved = 0;
}

int idt_set_handler(uint8_t num, interrupt_handler_t handler, const char *name) {
    if (num >= IDT_ENTRIES) return -1;

    interrupt_handlers[num] = handler;
    return 0;
}

int idt_register_irq(uint8_t irq, interrupt_handler_t handler, const char *name, uint32_t flags) {
    if (irq >= 16 || handler == NULL) return -1;

    uint8_t vector = IRQ_BASE + irq;

    if ((flags & IRQ_FLAG_SHARED) == 0 && interrupt_handlers[vector] != NULL) {
        serial_puts(SERIAL_COM1, "[IRQ] Warning: Replacing existing handler for IRQ ");
        serial_put_hex(SERIAL_COM1, irq);
        serial_puts(SERIAL_COM1, "\n");
    }

    irq_descriptors[irq].handler = handler;
    irq_descriptors[irq].name = name ? name : "unnamed";
    irq_descriptors[irq].description = "";
    irq_descriptors[irq].flags = flags;
    irq_descriptors[irq].call_count = 0;
    irq_descriptors[irq].error_count = 0;

    interrupt_handlers[vector] = handler;

    serial_puts(SERIAL_COM1, "[IRQ] Registered handler '");
    serial_puts(SERIAL_COM1, name ? name : "unnamed");
    serial_puts(SERIAL_COM1, "' for IRQ ");
    serial_put_hex(SERIAL_COM1, irq);
    serial_puts(SERIAL_COM1, "\n");

    return 0;
}

int idt_unregister_irq(uint8_t irq, interrupt_handler_t handler) {
    if (irq >= 16) return -1;

    uint8_t vector = IRQ_BASE + irq;

    if (interrupt_handlers[vector] == handler || handler == NULL) {
        interrupt_handlers[vector] = NULL;
        irq_descriptors[irq].handler = NULL;
        irq_descriptors[irq].name = NULL;

        serial_puts(SERIAL_COM1, "[IRQ] Unregistered handler for IRQ ");
        serial_put_hex(SERIAL_COM1, irq);
        serial_puts(SERIAL_COM1, "\n");

        return 0;
    }

    return -1;
}

void idt_enable_irq(uint8_t irq) {
    if (irq < 8) {
        uint8_t mask = inb(0x21) & ~(1 << irq);
        outb(0x21, mask);
    } else if (irq < 16) {
        uint8_t mask = inb(0xA1) & ~(1 << (irq - 8));
        outb(0xA1, mask);
    }
}

void idt_disable_irq(uint8_t irq) {
    if (irq < 8) {
        uint8_t mask = inb(0x21) | (1 << irq);
        outb(0x21, mask);
    } else if (irq < 16) {
        uint8_t mask = inb(0xA1) | (1 << (irq - 8));
        outb(0xA1, mask);
    }
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
    if (irq >= 8) {
        outb(0xA0, 0x20);
    }
    outb(0x20, 0x20);
}

static void print_stack_trace(struct interrupt_frame *frame) {
    serial_puts(SERIAL_COM1, "\n  Stack Trace:\n");
    
    uint64_t *rbp_ptr = (uint64_t *)frame->rbp;
    int frame_count = 0;
    const int max_frames = 10;

    while (rbp_ptr != NULL && frame_count < max_frames) {
        uint64_t rip_val = *(rbp_ptr + 1);

        if (rip_val == 0) break;

        serial_puts(SERIAL_COM1, "    #");
        serial_put_dec(SERIAL_COM1, frame_count);
        serial_puts(SERIAL_COM1, " [RIP=0x");
        serial_put_hex(SERIAL_COM1, rip_val);
        
        if (rip_val >= 0xFFFFFFFF80000000ULL) {
            serial_puts(SERIAL_COM1, " kernel]");
        } else {
            serial_puts(SERIAL_COM1, " user]");
        }
        
        serial_puts(SERIAL_COM1, " [RBP=0x");
        serial_put_hex(SERIAL_COM1, (uint64_t)rbp_ptr);
        serial_puts(SERIAL_COM1, "]\n");

        rbp_ptr = (uint64_t *)*rbp_ptr;
        frame_count++;
    }
}

static int handle_double_fault(struct interrupt_frame *frame) {
    static int double_fault_count = 0;
    double_fault_count++;

    serial_puts(SERIAL_COM1, "\n!!! DOUBLE FAULT DETECTED !!!\n");
    serial_puts(SERIAL_COM1, "  Occurrence: ");
    serial_put_dec(SERIAL_COM1, double_fault_count);
    serial_puts(SERIAL_COM1, "\n");

    print_stack_trace(frame);

    if (double_fault_count <= 3) {
        serial_puts(SERIAL_COM1, "  Attempting recovery...\n");

        extern void scheduler_yield(void);
        scheduler_yield();

        return 1;
    }

    serial_puts(SERIAL_COM1, "  Multiple double faults - system unstable\n");
    return 0;
}

static int handle_page_fault(struct interrupt_frame *frame) {
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

    if (fault_addr >= 0xFFFFFFFF80000000ULL) {
        serial_puts(SERIAL_COM1, "    Location: Kernel space\n");
    } else {
        serial_puts(SERIAL_COM1, "    Location: User space\n");
    }

    int is_user_mode = (frame->cs & 0x03) == 3;

    if (is_user_mode) {
        serial_puts(SERIAL_COM1, "  User page fault - killing process\n");
        return 1;
    }

    /*
     * 内核态 Page Fault 恢复策略:
     * 1. 检查是否是空指针解引用 (地址接近 NULL)
     * 2. 检查 RIP 是否在无效区域
     * 3. 尝试跳过导致故障的指令
     */
    serial_puts(SERIAL_COM1, "  Kernel page fault - attempting recovery...\n");

    /* 情况 1: 空指针或接近空指针的访问 */
    if (fault_addr < 0x1000) {
        serial_puts(SERIAL_COM1, "  → Null pointer access detected\n");
        serial_puts(SERIAL_COM1, "  → Skipping faulty instruction (RIP += 2)\n");

        /* 尝试跳过当前指令 (假设典型指令长度为 2-15 字节) */
        frame->rip += 2;  /* 最小指令长度 */

        return 1;  /* 告诉调用者可以恢复 */
    }

    /* 情况 2: RIP 在低地址区域 (可能是函数指针损坏) */
    if (frame->rip < 0xFFFFFFFF80000000ULL && frame->rip > 0xFFFFF) {
        serial_puts(SERIAL_COM1, "  → Invalid function pointer detected\n");
        serial_puts(SERIAL_COM1, "  → Returning to caller (simulated)\n");

        /* 尝试从 RSP 恢复返回地址 */
        frame->rsp += 8;  /* 弹出损坏的返回地址 */
        /* 这里我们无法知道真正的返回地址，只能标记不可恢复 */

        return 0;
    }

    /* 默认: 无法自动恢复 */
    serial_puts(SERIAL_COM1, "  → Unknown kernel page fault pattern\n");
    return 0;
}

static int handle_general_protection_fault(struct interrupt_frame *frame) {
    serial_puts(SERIAL_COM1, "\n  General Protection Fault Details:\n");
    serial_puts(SERIAL_COM1, "    Segment Selector: 0x");
    serial_put_hex(SERIAL_COM1, frame->err_code & 0xFFFF);
    serial_puts(SERIAL_COM1, "\n");

    if (frame->err_code & 0x01) {
        serial_puts(SERIAL_COM1, "    External event\n");
    }
    if (frame->err_code & 0x02) {
        serial_puts(SERIAL_COM1, "    IDT flag set (interrupt from gate)\n");
    }
    if (frame->err_code & 0x04) {
        serial_puts(SERIAL_COM1, "    LDT/GDT reference\n");
    }

    int is_user_mode = (frame->cs & 0x03) == 3;

    if (is_user_mode) {
        serial_puts(SERIAL_COM1, "  User GPF - killing process\n");
        return 1;
    }

    print_stack_trace(frame);
    return 0;
}

void exception_handler(struct interrupt_frame *frame) {
    nested_interrupt_count++;
    current_interrupt_vector = frame->int_no;

    if (frame->int_no < 32) {
        exception_counts[frame->int_no]++;
    }

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
    serial_puts(SERIAL_COM1, " (");
    serial_put_dec(SERIAL_COM1, frame->int_no);
    serial_puts(SERIAL_COM1, ")\n");

    serial_puts(SERIAL_COM1, "  Error Code: 0x");
    serial_put_hex(SERIAL_COM1, frame->err_code);
    serial_puts(SERIAL_COM1, "\n");

    serial_puts(SERIAL_COM1, "  RIP: 0x");
    serial_put_hex(SERIAL_COM1, frame->rip);
    serial_puts(SERIAL_COM1, "\n");

    serial_puts(SERIAL_COM1, "  CS:  0x");
    serial_put_hex(SERIAL_COM1, frame->cs);
    serial_puts(SERIAL_COM1, " (DPL=");
    serial_put_dec(SERIAL_COM1, frame->cs & 0x03);
    serial_puts(SERIAL_COM1, ")\n");

    serial_puts(SERIAL_COM1, "  RFLAGS: 0x");
    serial_put_hex(SERIAL_COM1, frame->rflags);
    serial_puts(SERIAL_COM1, "\n");

    serial_puts(SERIAL_COM1, "  RSP: 0x");
    serial_put_hex(SERIAL_COM1, frame->rsp);
    serial_puts(SERIAL_COM1, "\n");

    serial_puts(SERIAL_COM1, "  SS:  0x");
    serial_put_hex(SERIAL_COM1, frame->ss);
    serial_puts(SERIAL_COM1, "\n");

    serial_puts(SERIAL_COM1, "  Nested Level: ");
    serial_put_dec(SERIAL_COM1, nested_interrupt_count);
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

    int can_recover = 0;

    switch (frame->int_no) {
        case 8:
            can_recover = handle_double_fault(frame);
            break;
        case 14:
            can_recover = handle_page_fault(frame);
            break;
        case 13:
            can_recover = handle_general_protection_fault(frame);
            break;
        default:
            print_stack_trace(frame);
            break;
    }

    serial_puts(SERIAL_COM1, "\n========================================\n");

    int is_user_mode = (frame->cs & 0x03) == 3;

    if (can_recover && is_user_mode) {
        serial_puts(SERIAL_COM1, "User process crashed. Killing process.\n");

        extern struct process* process_get_current(void);
        extern void process_exit(struct process *proc, uint64_t exit_code);

        struct process *current = process_get_current();
        if (current != NULL) {
            process_exit(current, 1);
        }

        extern void scheduler_yield(void);
        scheduler_yield();

        nested_interrupt_count--;
        current_interrupt_vector = 0xFFFFFFFFFFFFFFFFULL;
        return;
    }

    if (!can_recover) {
        serial_puts(SERIAL_COM1, "Kernel panic! System halted.\n");

        __asm__ volatile ("cli");

        while (1) {
            __asm__ volatile ("hlt");
        }
    }

    nested_interrupt_count--;
    current_interrupt_vector = 0xFFFFFFFFFFFFFFFFULL;
}

void irq_handler(struct interrupt_frame *frame) {
    uint8_t irq = frame->int_no - IRQ_BASE;

    if (irq < 16) {
        irq_counts[irq]++;

        if (irq_descriptors[irq].handler != NULL) {
            irq_descriptors[irq].call_count++;

            interrupt_handlers[frame->int_no](frame);
        } else if (interrupt_handlers[frame->int_no] != NULL) {
            interrupt_handlers[frame->int_no](frame);
        } else {
            /* Silent: IRQ 7/15 are standard PIC spurious vectors.
             * IRQ 0 (timer) and IRQ 14 (primary ATA) may fire during
             * idle when the handler is registered via idt_set_handler
             * rather than idt_register_irq. */
            if (irq != 0 && irq != 7 && irq != 14 && irq != 15) {
                serial_puts(SERIAL_COM1, "[IRQ] Spurious IRQ: ");
                serial_put_hex(SERIAL_COM1, irq);
                serial_puts(SERIAL_COM1, "\n");
            }
        }
    }

    pic_send_eoi(irq);
}

extern uint64_t isr_table[];
extern uint64_t irq_table[];

int idt_init(void) {
    serial_puts(SERIAL_COM1, "[IDT] Initializing Interrupt Descriptor Table...\n");

    idt_ptr.limit = sizeof(idt) - 1;
    idt_ptr.base = (uint64_t)&idt;

    for (int i = 0; i < IDT_ENTRIES; i++) {
        idt_set_gate(i, 0, 0, 0);
        interrupt_handlers[i] = NULL;
    }

    for (int i = 0; i < 16; i++) {
        irq_descriptors[i].handler = NULL;
        irq_descriptors[i].name = NULL;
        irq_descriptors[i].description = NULL;
        irq_descriptors[i].flags = 0;
        irq_descriptors[i].call_count = 0;
        irq_descriptors[i].error_count = 0;
    }

    for (int i = 0; i < 32; i++) {
        idt_set_gate(i, isr_table[i], GDT_KERNEL_CODE, IDT_TYPE_INTERRUPT);
        exception_counts[i] = 0;
    }

    for (int i = 0; i < 16; i++) {
        idt_set_gate(IRQ_BASE + i, irq_table[i], GDT_KERNEL_CODE, IDT_TYPE_INTERRUPT);
        irq_counts[i] = 0;
    }

    idt_set_gate(0x80, (uint64_t)syscall_handler, GDT_KERNEL_CODE, IDT_TYPE_TRAP | IDT_DPL_USER);

    pic_remap(IRQ_BASE, IRQ_BASE + 8);

    idt_flush((uint64_t)&idt_ptr);

    serial_puts(SERIAL_COM1, "[IDT] IDT initialized successfully\n");
    serial_puts(SERIAL_COM1, "[IDT]   Total entries: ");
    serial_put_dec(SERIAL_COM1, IDT_ENTRIES);
    serial_puts(SERIAL_COM1, "\n");
    serial_puts(SERIAL_COM1, "[IDT]   Exceptions: 0-31\n");
    serial_puts(SERIAL_COM1, "[IDT]   IRQs: 32-47 (base ");
    serial_put_dec(SERIAL_COM1, IRQ_BASE);
    serial_puts(SERIAL_COM1, ")\n");
    serial_puts(SERIAL_COM1, "[IDT]   Syscall: 0x80\n");

    return MODULE_INIT_SUCCESS;
}

void idt_dump_state(void) {
    serial_puts(SERIAL_COM1, "\n=== IDT State Dump ===\n");
    serial_puts(SERIAL_COM1, "Nested interrupts: ");
    serial_put_dec(SERIAL_COM1, nested_interrupt_count);
    serial_puts(SERIAL_COM1, "\n");

    if (current_interrupt_vector != 0xFFFFFFFFFFFFFFFFULL) {
        serial_puts(SERIAL_COM1, "Current interrupt: ");
        serial_put_dec(SERIAL_COM1, current_interrupt_vector);
        serial_puts(SERIAL_COM1, "\n");
    }

    serial_puts(SERIAL_COM1, "\nRegistered IRQ handlers:\n");
    for (int i = 0; i < 16; i++) {
        if (irq_descriptors[i].handler != NULL) {
            serial_puts(SERIAL_COM1, "  IRQ ");
            serial_put_dec(SERIAL_COM1, i);
            serial_puts(SERIAL_COM1, ": ");

            if (irq_descriptors[i].name) {
                serial_puts(SERIAL_COM1, irq_descriptors[i].name);
            } else {
                serial_puts(SERIAL_COM1, "(anonymous)");
            }

            serial_puts(SERIAL_COM1, " [calls=");
            serial_put_dec(SERIAL_COM1, irq_descriptors[i].call_count);
            serial_puts(SERIAL_COM1, ", errors=");
            serial_put_dec(SERIAL_COM1, irq_descriptors[i].error_count);
            serial_puts(SERIAL_COM1, "]\n");
        }
    }
}

uint64_t idt_get_interrupt_count(uint8_t vector) {
    if (vector < 32) {
        return exception_counts[vector];
    } else if (vector >= IRQ_BASE && vector < IRQ_BASE + 16) {
        return irq_counts[vector - IRQ_BASE];
    }
    return 0;
}

const char* idt_get_exception_name(uint8_t vector) {
    if (vector < 32) {
        return exception_messages[vector];
    }
    return "Unknown";
}

void idt_print_interrupt_stats(void) {
    serial_puts(SERIAL_COM1, "\n=== Interrupt Statistics ===\n");

    serial_puts(SERIAL_COM1, "\nException counts:\n");
    for (int i = 0; i < 32; i++) {
        if (exception_counts[i] > 0) {
            serial_puts(SERIAL_COM1, "  #");
            serial_put_dec(SERIAL_COM1, i);
            serial_puts(SERIAL_COM1, " (");
            serial_puts(SERIAL_COM1, exception_messages[i]);
            serial_puts(SERIAL_COM1, "): ");
            serial_put_dec(SERIAL_COM1, exception_counts[i]);
            serial_puts(SERIAL_COM1, "\n");
        }
    }

    serial_puts(SERIAL_COM1, "\nIRQ counts:\n");
    for (int i = 0; i < 16; i++) {
        if (irq_counts[i] > 0) {
            const char *irq_names[] = {
                "Timer", "Keyboard", "Cascade", "COM2", "COM1",
                "LPT2", "Floppy", "LPT1/Spurious", "CMOS",
                "ACPI", "PCI", "NIC", "CoProcessor", "Primary ATA",
                "Secondary ATA", "Spurious"
            };

            serial_puts(SERIAL_COM1, "  IRQ");
            serial_put_dec(SERIAL_COM1, i);
            serial_puts(SERIAL_COM1, " (");
            serial_puts(SERIAL_COM1, irq_names[i]);
            serial_puts(SERIAL_COM1, "): ");
            serial_put_dec(SERIAL_COM1, irq_counts[i]);

            if (irq_descriptors[i].handler != NULL) {
                serial_puts(SERIAL_COM1, " [handler='");
                serial_puts(SERIAL_COM1, irq_descriptors[i].name ? irq_descriptors[i].name : "?");
                serial_puts(SERIAL_COM1, "']");
            }

            serial_puts(SERIAL_COM1, "\n");
        }
    }

    serial_puts(SERIAL_COM1, "\nTotal nested interrupts: ");
    serial_put_dec(SERIAL_COM1, nested_interrupt_count);
    serial_puts(SERIAL_COM1, "\n");
}
