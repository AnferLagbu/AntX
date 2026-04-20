#include "proc.h"
#include "serial.h"
#include "mm.h"
#include "proc_rust.h"

struct scheduler sched;

uint64_t user_entry_target = 0;
uint64_t user_entry_cr3 = 0;

void scheduler_init(void) {
    sched.ready_queue = NULL;
    sched.current = NULL;
    sched.tick_count = 0;
    sched.next_pid = 1;
    
    serial_puts(SERIAL_COM1, "Scheduler initialized\n");
}

uint64_t scheduler_next_pid(void) {
    return sched.next_pid++;
}

void scheduler_add(struct process *proc) {
    if (proc == NULL) return;
    
    proc->state = PROC_READY;
    proc->time_slice = TIME_SLICE;
    
    if (sched.ready_queue == NULL) {
        sched.ready_queue = proc;
        proc->next = proc;
        proc->prev = proc;
    } else {
        struct process *tail = sched.ready_queue->prev;
        tail->next = proc;
        proc->prev = tail;
        proc->next = sched.ready_queue;
        sched.ready_queue->prev = proc;
    }
    
    rust_sched_add((uint32_t)proc->pid);
    
    serial_puts(SERIAL_COM1, "Scheduler: added PID=");
    serial_put_dec(SERIAL_COM1, proc->pid);
    serial_puts(SERIAL_COM1, "\n");
}

void scheduler_remove(struct process *proc) {
    if (proc == NULL || sched.ready_queue == NULL) return;
    
    if (proc->next == proc) {
        sched.ready_queue = NULL;
    } else {
        proc->prev->next = proc->next;
        proc->next->prev = proc->prev;
        if (sched.ready_queue == proc) {
            sched.ready_queue = proc->next;
        }
    }
    
    proc->next = NULL;
    proc->prev = NULL;
}

void scheduler_tick(void) {
    sched.tick_count++;
    
    if (sched.current != NULL) {
        sched.current->time_slice--;
        sched.current->cpu_time++;
        
        if (sched.current->time_slice <= 0) {
            scheduler_schedule();
        }
    }
}

void scheduler_schedule(void) {
    struct process *prev = sched.current;
    struct process *next = NULL;
    
    if (prev != NULL && prev->state == PROC_BLOCKED) {
        sched.current = NULL;
        prev = NULL;
    }
    
    if (sched.ready_queue != NULL) {
        next = sched.ready_queue;
        scheduler_remove(next);
    }
    
    if (next == NULL) {
        return;
    }
    
    if (prev != NULL && prev->state == PROC_RUNNING) {
        prev->state = PROC_READY;
        scheduler_add(prev);
    }
    
    next->state = PROC_RUNNING;
    sched.current = next;
    rust_sched_set_current((uint32_t)next->pid);
    
    serial_puts(SERIAL_COM1, "Schedule: switch to PID=");
    serial_put_dec(SERIAL_COM1, next->pid);
    serial_puts(SERIAL_COM1, " entry=0x");
    serial_put_hex(SERIAL_COM1, next->context.rip);
    serial_puts(SERIAL_COM1, " cr3=0x");
    serial_put_hex(SERIAL_COM1, next->cr3);
    serial_puts(SERIAL_COM1, "\n");
    
    extern void tss_set_kernel_stack(uint64_t rsp0);
    tss_set_kernel_stack(next->kernel_stack);
    
    if (prev == NULL) {
        serial_puts(SERIAL_COM1, "[DEBUG] First process launch:\n");
        serial_puts(SERIAL_COM1, "  kernel_stack=0x");
        serial_put_hex(SERIAL_COM1, next->kernel_stack);
        serial_puts(SERIAL_COM1, "\n");
        serial_puts(SERIAL_COM1, "  ctx.rip=0x");
        serial_put_hex(SERIAL_COM1, next->context.rip);
        serial_puts(SERIAL_COM1, " cs=0x");
        serial_put_hex(SERIAL_COM1, next->context.cs);
        serial_puts(SERIAL_COM1, "\n");
        serial_puts(SERIAL_COM1, "  ctx.rsp=0x");
        serial_put_hex(SERIAL_COM1, next->context.rsp);
        serial_puts(SERIAL_COM1, " ss=0x");
        serial_put_hex(SERIAL_COM1, next->context.ss);
        serial_puts(SERIAL_COM1, "\n");
        serial_puts(SERIAL_COM1, "  ctx.rflags=0x");
        serial_put_hex(SERIAL_COM1, next->context.rflags);
        serial_puts(SERIAL_COM1, "\n");
        serial_puts(SERIAL_COM1, "  ctx.cr3=0x");
        serial_put_hex(SERIAL_COM1, next->context.cr3);
        serial_puts(SERIAL_COM1, "\n");
        
        if (next->context.rsp & 0xF) {
            serial_puts(SERIAL_COM1, "[ERROR] User stack NOT 16-byte aligned! rsp=0x");
            serial_put_hex(SERIAL_COM1, next->context.rsp);
            serial_puts(SERIAL_COM1, "\n");
        } else {
            serial_puts(SERIAL_COM1, "[OK] User stack is 16-byte aligned\n");
        }
        
        serial_puts(SERIAL_COM1, "[DEBUG] First process launch - using inline asm iretq...\n");
        
        uint64_t current_rsp;
        __asm__ volatile ("mov %%rsp, %0" : "=r"(current_rsp));
        serial_puts(SERIAL_COM1, "[DEBUG] Current RSP=0x");
        serial_put_hex(SERIAL_COM1, current_rsp);
        serial_puts(SERIAL_COM1, " kernel_stack=0x");
        serial_put_hex(SERIAL_COM1, next->kernel_stack);
        serial_puts(SERIAL_COM1, "\n");
        
        uint64_t rsp_page = current_rsp & ~0xFFF;
        vmm_map_page_in_table(next->cr3, rsp_page, rsp_page, PAGE_PRESENT | PAGE_WRITABLE);
        
        serial_puts(SERIAL_COM1, "[DEBUG] Mapped kernel stack page 0x");
        serial_put_hex(SERIAL_COM1, rsp_page);
        serial_puts(SERIAL_COM1, " in user page table\n");
        
        uint64_t ss_val = next->context.ss;
        uint64_t rsp_val = next->context.rsp;
        uint64_t cs_val = next->context.cs;
        uint64_t rip_val = next->context.rip;
        
        tss_set_kernel_stack(next->kernel_stack);
        
        serial_puts(SERIAL_COM1, "[DEBUG] TSS check done\n");
        
        __asm__ volatile (
            "cli\n"
            "mov $0x23, %%ax\n"
            "mov %%ax, %%ds\n"
            "mov %%ax, %%es\n"
            "mov %%ax, %%fs\n"
            "mov %%ax, %%gs\n"
            "movq %0, %%rax\n"
            "pushq %%rax\n"
            "movq %1, %%rax\n"
            "pushq %%rax\n"
            "pushq $0x202\n"
            "movq %2, %%rax\n"
            "pushq %%rax\n"
            "movq %3, %%rax\n"
            "pushq %%rax\n"
            "mov %4, %%cr3\n"
            "iretq\n"
            :
            : "r"(ss_val), "r"(rsp_val), "r"(cs_val), "r"(rip_val), "r"(next->cr3)
            : "rax", "memory"
        );
        
        serial_puts(SERIAL_COM1, "[DEBUG] iretq RETURNED!\n");
    } else {
        __asm__ volatile ("mov %0, %%cr3" : : "r"(next->cr3));
        
        if (prev != next) {
            extern void process_switch_asm(struct cpu_context *old, struct cpu_context *new_ctx);
            process_switch_asm(&prev->context, &next->context);
        }
    }
}

void scheduler_yield(void) {
    if (sched.current != NULL) {
        scheduler_schedule();
    }
}

bool proc_has_runnable(void) {
    return sched.ready_queue != NULL;
}
