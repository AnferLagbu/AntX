#include "idt.h"
#include "gdt.h"
#include "klog.h"
#include "io.h"
#include "recovery.h"
#include "ioapic.h"
#include "smp.h"

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
    idt[num].offset_low = handler & 0xFFFF;
    idt[num].offset_mid = (handler >> 16) & 0xFFFF;
    idt[num].offset_high = (handler >> 32) & 0xFFFFFFFF;

    idt[num].selector = selector;
    idt[num].ist = 0;
    idt[num].type_attr = type;
    idt[num].reserved = 0;
}

int idt_set_handler(uint8_t num, interrupt_handler_t handler, const char *name) {
    (void)name;
    interrupt_handlers[num] = handler;
    return 0;
}

int idt_register_irq(uint8_t irq, interrupt_handler_t handler, const char *name, uint32_t flags) {
    if (irq >= 16 || handler == NULL) return -1;

    uint8_t vector = IRQ_BASE + irq;

    if ((flags & IRQ_FLAG_SHARED) == 0 && interrupt_handlers[vector] != NULL) {
        klog_kern_warn("IRQ: Replacing existing handler for IRQ 0x%x", irq);
    }

    irq_descriptors[irq].handler = handler;
    irq_descriptors[irq].name = name ? name : "unnamed";
    irq_descriptors[irq].description = "";
    irq_descriptors[irq].flags = flags;
    irq_descriptors[irq].call_count = 0;
    irq_descriptors[irq].error_count = 0;

    interrupt_handlers[vector] = handler;

    if (ioapic_is_present()) {
        ioapic_redirect_irq(irq, vector, 0, IOAPIC_DELIVERY_FIXED);
        ioapic_unmask_irq(irq);
    }

    klog_kern("IRQ: Registered handler '%s' for IRQ 0x%x", name ? name : "unnamed", irq);

    return 0;
}

int idt_unregister_irq(uint8_t irq, interrupt_handler_t handler) {
    if (irq >= 16) return -1;

    uint8_t vector = IRQ_BASE + irq;

    if (interrupt_handlers[vector] == handler || handler == NULL) {
        interrupt_handlers[vector] = NULL;
        irq_descriptors[irq].handler = NULL;
        irq_descriptors[irq].name = NULL;

        klog_kern("IRQ: Unregistered handler for IRQ 0x%x", irq);

        return 0;
    }

    return -1;
}

void idt_enable_irq(uint8_t irq) {
    if (ioapic_is_present()) {
        ioapic_unmask_irq(irq);
        return;
    }
    if (irq < 8) {
        uint8_t mask = inb(0x21) & ~(1 << irq);
        outb(0x21, mask);
    } else if (irq < 16) {
        uint8_t slave_mask = inb(0xA1) & ~(1 << (irq - 8));
        outb(0xA1, slave_mask);
        uint8_t master_mask = inb(0x21) & ~(1 << 2);
        outb(0x21, master_mask);
    }
}

void idt_disable_irq(uint8_t irq) {
    if (ioapic_is_present()) {
        ioapic_mask_irq(irq);
        return;
    }
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
    klog_kern("  Stack Trace:");

    uint64_t *rbp_ptr = (uint64_t *)frame->rbp;
    int frame_count = 0;
    const int max_frames = 10;

    while (rbp_ptr != NULL && frame_count < max_frames) {
        uint64_t rip_val = *(rbp_ptr + 1);

        if (rip_val == 0) break;

        klog_kern("    #%d [RIP=0x%lx %s] [RBP=0x%lx]",
                  frame_count, rip_val,
                  rip_val >= 0xFFFFFFFF80000000ULL ? "kernel" : "user",
                  (uint64_t)rbp_ptr);

        rbp_ptr = (uint64_t *)*rbp_ptr;
        frame_count++;
    }
}

static int handle_double_fault(struct interrupt_frame *frame) {
    static int double_fault_count = 0;
    double_fault_count++;

    klog_kern_crit("DOUBLE FAULT DETECTED! Occurrence: %d", double_fault_count);

    print_stack_trace(frame);

    if (double_fault_count <= 3) {
        klog_kern("  Attempting recovery...");

        extern void scheduler_yield(void);
        scheduler_yield();

        return 1;
    }

    klog_kern_err("  Multiple double faults - system unstable");
    return 0;
}

static int handle_page_fault(struct interrupt_frame *frame) {
    uint64_t fault_addr;
    __asm__ volatile ("mov %%cr2, %0" : "=r"(fault_addr));

    klog_kern_err("  Page Fault Details:");
    klog_kern("    Fault Address (CR2): 0x%lx", fault_addr);
    klog_kern("    Access Type: %s", (frame->err_code & 0x02) ? "Write" : "Read");
    klog_kern("    Mode: %s", (frame->err_code & 0x04) ? "User" : "Kernel");
    klog_kern("    Cause: %s", (frame->err_code & 0x01) ? "Protection Violation" : "Page Not Present");
    klog_kern("    Location: %s", fault_addr >= 0xFFFFFFFF80000000ULL ? "Kernel space" : "User space");

    /* 使用双重判断：CS 段选择子 + RIP 地址范围 */
    int is_user_mode_cs = (frame->cs & 0x03) == 3;
    int is_user_mode_rip = frame->rip < 0xFFFFFFFF80000000ULL && frame->rip > 0xFFFF;
    int is_user_mode = is_user_mode_cs || is_user_mode_rip;

    if (is_user_mode) {
        klog_kern_warn("  User page fault - killing process");
        return 1;
    }

    klog_kern("  Kernel page fault - attempting recovery...");

    if (fault_addr < 0x1000) {
        klog_kern("  -> Null pointer access detected");
        klog_kern("  -> Skipping faulty instruction (RIP += 2)");

        frame->rip += 2;

        return 1;
    }

    if (frame->rip < 0xFFFFFFFF80000000ULL && frame->rip > 0xFFFFF) {
        klog_kern("  -> Invalid function pointer detected");
        klog_kern("  -> Returning to caller (simulated)");

        frame->rsp += 8;

        return 0;
    }

    klog_kern_err("  -> Unknown kernel page fault pattern");
    return 0;
}

static int handle_general_protection_fault(struct interrupt_frame *frame) {
    klog_kern_err("  General Protection Fault Details:");
    klog_kern("    Segment Selector: 0x%x", frame->err_code & 0xFFFF);

    if (frame->err_code & 0x01) {
        klog_kern("    External event");
    }
    if (frame->err_code & 0x02) {
        klog_kern("    IDT flag set (interrupt from gate)");
    }
    if (frame->err_code & 0x04) {
        klog_kern("    LDT/GDT reference");
    }

    /* 使用双重判断：CS 段选择子 + RIP 地址范围 */
    int is_user_mode_cs = (frame->cs & 0x03) == 3;
    int is_user_mode_rip = frame->rip < 0xFFFFFFFF80000000ULL && frame->rip > 0xFFFF;
    int is_user_mode = is_user_mode_cs || is_user_mode_rip;

    if (is_user_mode) {
        klog_kern_warn("  User GPF - killing process (RIP=0x%lx, CS=0x%x)", frame->rip, frame->cs);
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

    const char *exc_name = (frame->int_no < 32) ? exception_messages[frame->int_no] : "Unknown";
    klog_kern_crit("EXCEPTION: %s", exc_name);
    klog_kern("  Interrupt: 0x%x (%d)", frame->int_no, frame->int_no);
    klog_kern("  Error Code: 0x%lx", frame->err_code);
    klog_kern("  RIP: 0x%lx", frame->rip);
    klog_kern("  CS:  0x%x (DPL=%d)", frame->cs & 0xFFFF, frame->cs & 0x03);
    klog_kern("  RFLAGS: 0x%lx", frame->rflags);
    klog_kern("  RSP: 0x%lx", frame->rsp);
    klog_kern("  SS:  0x%x", frame->ss & 0xFFFF);
    klog_kern("  Nested Level: %d", nested_interrupt_count);

    klog_kern("  Registers:");
    klog_kern("    RAX: 0x%lx  RBX: 0x%lx", frame->rax, frame->rbx);
    klog_kern("    RCX: 0x%lx  RDX: 0x%lx", frame->rcx, frame->rdx);
    klog_kern("    RSI: 0x%lx  RDI: 0x%lx", frame->rsi, frame->rdi);
    klog_kern("    RBP: 0x%lx  R8:  0x%lx", frame->rbp, frame->r8);
    klog_kern("    R9: 0x%lx  R10: 0x%lx", frame->r9, frame->r10);
    klog_kern("    R11: 0x%lx  R12: 0x%lx", frame->r11, frame->r12);
    klog_kern("    R13: 0x%lx  R14: 0x%lx", frame->r13, frame->r14);
    klog_kern("    R15: 0x%lx", frame->r15);

    int can_recover = 0;

    /* int 0x82: dedicated recovery interrupt from Rust panic_handler */
    if (frame->int_no == 0x82) {
        int recov = recovery_try_recover_from_idt();
        if (recov == 0) {
            klog_kern("RECOVERY: Domain-level recovery succeeded, continuing execution");
            nested_interrupt_count--;
            current_interrupt_vector = 0xFFFFFFFFFFFFFFFFULL;
            return;
        }
        if (recov == -2) {
            klog_kern_err("RECOVERY: Already attempted, refusing to loop");
        }
        /* Recovery failed — fall through to normal panic path */
    }

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
        case 0: {
            /* Division By Zero - 检查是否由 user态 代码触发 */
            /* 注意: 某些情况下 frame->cs 可能不准确，
             * 因此使用 RIP 地址范围作为备用判断 */
            int is_user_rip = frame->rip < 0xFFFFFFFF80000000ULL && frame->rip > 0xFFFF;
            int is_user_cs = (frame->cs & 0x03) == 3;

            if (is_user_rip || is_user_cs) {
                klog_kern_warn("  User-mode Division By Zero detected");
                klog_kern("    RIP=0x%lx (user space), CS=0x%x", frame->rip, frame->cs);
                klog_kern("  Killing user process to prevent system crash");
                can_recover = 1; /* 允许恢复：杀死 user 进程 */
            } else {
                print_stack_trace(frame);
            }
            break;
        }
        default:
            print_stack_trace(frame);
            break;
    }

    /* 使用双重判断：CS 段选择子 + RIP 地址范围 */
    int is_user_mode_cs = (frame->cs & 0x03) == 3;
    int is_user_mode_rip = frame->rip < 0xFFFFFFFF80000000ULL && frame->rip > 0xFFFF;
    int is_user_mode = is_user_mode_cs || is_user_mode_rip;

    if (can_recover && is_user_mode) {
        klog_kern("User process crashed. Killing process.");

        extern void process_exit(uint32_t exit_code);
        process_exit(1);

        extern void scheduler_yield(void);
        scheduler_yield();

        nested_interrupt_count--;
        current_interrupt_vector = 0xFFFFFFFFFFFFFFFFULL;
        return;
    }

    if (!can_recover) {
        /* v4 barrier-stack: attempt domain-level recovery before halting */
        int recov = recovery_try_recover_from_idt();
        if (recov == 0) {
            klog_kern("RECOVERY: Domain-level recovery succeeded, continuing execution");
            nested_interrupt_count--;
            current_interrupt_vector = 0xFFFFFFFFFFFFFFFFULL;
            return;
        }
        if (recov == -2) {
            klog_kern_err("RECOVERY: Already attempted, refusing to loop");
        }

        klog_kern_crit("Kernel panic! System halted.");

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
            if (irq != 0 && irq != 7 && irq != 14 && irq != 15) {
                klog_kern_warn("IRQ: Spurious IRQ: 0x%x", irq);
            }
        }
    }

    if (ioapic_is_present()) {
        lapic_send_eoi();
    } else {
        pic_send_eoi(irq);
    }
}

extern uint64_t isr_table[];
extern uint64_t irq_table[];
extern void isr0x82(void);

int idt_init(void) {
    klog_kern("Initializing Interrupt Descriptor Table...");

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

    /* v4 barrier-stack: dedicated recovery interrupt (int 0x82) */
    idt_set_gate(0x82, (uint64_t)isr0x82, GDT_KERNEL_CODE, IDT_TYPE_TRAP);

    pic_remap(IRQ_BASE, IRQ_BASE + 8);

    idt_flush((uint64_t)&idt_ptr);

    klog_kern("IDT initialized successfully");
    klog_kern("  Total entries: %d", IDT_ENTRIES);
    klog_kern("  Exceptions: 0-31");
    klog_kern("  IRQs: 32-47 (base %d)", IRQ_BASE);
    klog_kern("  Syscall: 0x80");

    return MODULE_INIT_SUCCESS;
}

void idt_dump_state(void) {
    klog_kern("=== IDT State Dump ===");
    klog_kern("Nested interrupts: %d", nested_interrupt_count);

    if (current_interrupt_vector != 0xFFFFFFFFFFFFFFFFULL) {
        klog_kern("Current interrupt: %d", current_interrupt_vector);
    }

    klog_kern("Registered IRQ handlers:");
    for (int i = 0; i < 16; i++) {
        if (irq_descriptors[i].handler != NULL) {
            klog_kern("  IRQ %d: %s [calls=%d, errors=%d]",
                      i,
                      irq_descriptors[i].name ? irq_descriptors[i].name : "(anonymous)",
                      irq_descriptors[i].call_count,
                      irq_descriptors[i].error_count);
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
    klog_kern("=== Interrupt Statistics ===");

    klog_kern("Exception counts:");
    for (int i = 0; i < 32; i++) {
        if (exception_counts[i] > 0) {
            klog_kern("  #%d (%s): %d", i, exception_messages[i], exception_counts[i]);
        }
    }

    klog_kern("IRQ counts:");
    for (int i = 0; i < 16; i++) {
        if (irq_counts[i] > 0) {
            const char *irq_names[] = {
                "Timer", "Keyboard", "Cascade", "COM2", "COM1",
                "LPT2", "Floppy", "LPT1/Spurious", "CMOS",
                "ACPI", "PCI", "NIC", "CoProcessor", "Primary ATA",
                "Secondary ATA", "Spurious"
            };

            if (irq_descriptors[i].handler != NULL) {
                klog_kern("  IRQ%d (%s): %d [handler='%s']",
                          i, irq_names[i], irq_counts[i],
                          irq_descriptors[i].name ? irq_descriptors[i].name : "?");
            } else {
                klog_kern("  IRQ%d (%s): %d", i, irq_names[i], irq_counts[i]);
            }
        }
    }

    klog_kern("Total nested interrupts: %d", nested_interrupt_count);
}
