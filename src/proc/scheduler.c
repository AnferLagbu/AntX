#include "proc.h"
#include "serial.h"

struct scheduler sched;

void scheduler_init(void) {
    sched.ready_queue = NULL;
    sched.current = NULL;
    sched.tick_count = 0;
    sched.next_pid = 1;
    
    serial_puts(SERIAL_COM1, "Scheduler initialized\n");
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
    
    if (sched.ready_queue != NULL) {
        next = sched.ready_queue;
        scheduler_remove(next);
    }
    
    if (next == NULL) {
        if (prev != NULL && prev->state == PROC_RUNNING) {
            return;
        }
        return;
    }
    
    if (prev != NULL && prev->state == PROC_RUNNING) {
        prev->state = PROC_READY;
        scheduler_add(prev);
    }
    
    next->state = PROC_RUNNING;
    sched.current = next;
    
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
        serial_puts(SERIAL_COM1, "[DEBUG] Calling process_start_user_asm...\n");
        
        extern void process_start_user_asm(uint64_t kernel_stack, struct cpu_context *ctx);
        process_start_user_asm(next->kernel_stack, &next->context);
        
        serial_puts(SERIAL_COM1, "[DEBUG] process_start_user_asm RETURNED!\n");
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
