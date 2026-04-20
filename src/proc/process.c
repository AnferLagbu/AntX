#include "proc.h"
#include "mm.h"
#include "serial.h"
#include "assert.h"

static struct process process_table[MAX_PROCESSES];

extern void process_switch(struct cpu_context *old_ctx, struct cpu_context *new_ctx);
extern struct scheduler sched;

void process_init(void) {
    for (int i = 0; i < MAX_PROCESSES; i++) {
        process_table[i].pid = 0;
        process_table[i].state = PROC_NEW;
        process_table[i].next = NULL;
        process_table[i].prev = NULL;
    }
    
    serial_puts(SERIAL_COM1, "Process manager initialized\n");
}

struct process* process_create(void (*entry)(void), uint64_t arg, uint64_t flags) {
    int slot = -1;
    for (int i = 0; i < MAX_PROCESSES; i++) {
        if (process_table[i].pid == 0) {
            slot = i;
            break;
        }
    }
    
    if (slot < 0) {
        serial_puts(SERIAL_COM1, "Process table full\n");
        return NULL;
    }
    
    struct process *proc = &process_table[slot];
    
    proc->pid = scheduler_next_pid();
    proc->state = PROC_READY;
    proc->priority = 1;
    proc->time_slice = TIME_SLICE;
    proc->cpu_time = 0;
    proc->parent_pid = (sched.current != NULL) ? sched.current->pid : 0;
    proc->exit_code = 0;
    
    proc->context.rip = (uint64_t)entry;
    proc->context.rsp = (uint64_t)pmm_alloc_page() + PAGE_SIZE;
    proc->context.rflags = 0x202;
    proc->context.cs = 0x08;
    proc->context.ss = 0x10;
    
    proc->cr3 = vmm_create_user_page_table();
    
    proc->next = NULL;
    proc->prev = NULL;
    
    serial_puts(SERIAL_COM1, "Process created: PID=");
    serial_put_dec(SERIAL_COM1, proc->pid);
    serial_puts(SERIAL_COM1, "\n");
    
    return proc;
}

void process_exit(struct process *proc, int exit_code) {
    if (proc == NULL) return;
    
    proc->state = PROC_ZOMBIE;
    proc->exit_code = exit_code;
    
    scheduler_remove(proc);
    
    serial_puts(SERIAL_COM1, "Process exited: PID=");
    serial_put_dec(SERIAL_COM1, proc->pid);
    serial_puts(SERIAL_COM1, ", exit_code=");
    serial_put_dec(SERIAL_COM1, exit_code);
    serial_puts(SERIAL_COM1, "\n");
}

struct process* process_get_current(void) {
    return sched.current;
}

struct process* process_find_by_pid(uint64_t pid) {
    for (int i = 0; i < MAX_PROCESSES; i++) {
        if (process_table[i].pid == pid) {
            return &process_table[i];
        }
    }
    return NULL;
}
