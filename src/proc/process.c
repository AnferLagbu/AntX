#include "proc.h"
#include "mm.h"
#include "serial.h"
#include "assert.h"

static struct process process_table[MAX_PROCESSES];
static struct process *current_process = NULL;

extern void process_switch(struct cpu_context *old_ctx, struct cpu_context *new_ctx);

void process_init(void) {
    for (int i = 0; i < MAX_PROCESSES; i++) {
        process_table[i].pid = 0;
        process_table[i].state = PROC_NEW;
        process_table[i].next = NULL;
        process_table[i].prev = NULL;
    }
    
    serial_puts(SERIAL_COM1, "Process manager initialized\n");
}

static struct process* process_alloc(void) {
    for (int i = 0; i < MAX_PROCESSES; i++) {
        if (process_table[i].state == PROC_NEW || process_table[i].pid == 0) {
            return &process_table[i];
        }
    }
    return NULL;
}

struct process* process_create(void (*entry)(void), uint64_t session_id, uint64_t pwid) {
    struct process *proc = process_alloc();
    if (proc == NULL) {
        serial_puts(SERIAL_COM1, "Failed to allocate process\n");
        return NULL;
    }
    
    static uint64_t next_pid = 1;
    proc->pid = next_pid++;
    proc->session_id = session_id;
    proc->pwid = pwid;
    proc->parent_pid = (current_process != NULL) ? current_process->pid : 0;
    
    proc->state = PROC_READY;
    proc->exit_code = 0;
    proc->priority = 1;
    proc->cpu_time = 0;
    proc->start_time = 0;
    proc->time_slice = TIME_SLICE;
    
    proc->kernel_stack = (uint64_t)pmm_alloc_page();
    ASSERT(proc->kernel_stack != 0);
    proc->kernel_stack += PAGE_SIZE;
    
    proc->cr3 = (uint64_t)pmm_alloc_page();
    ASSERT(proc->cr3 != 0);
    
    proc->context.rip = (uint64_t)entry;
    proc->context.cs = 0x08;
    proc->context.rflags = 0x202;
    proc->context.rsp = proc->kernel_stack;
    proc->context.ss = 0x10;
    
    proc->next = NULL;
    proc->prev = NULL;
    proc->parent = NULL;
    proc->children = NULL;
    proc->sibling = NULL;
    
    serial_puts(SERIAL_COM1, "Process created: PID=");
    serial_put_dec(SERIAL_COM1, proc->pid);
    serial_puts(SERIAL_COM1, "\n");
    
    return proc;
}

void process_exit(struct process *proc, uint64_t exit_code) {
    if (proc == NULL) return;
    
    proc->exit_code = exit_code;
    proc->state = PROC_ZOMBIE;
    
    if (proc->kernel_stack) {
        pmm_free_page((void*)(proc->kernel_stack - PAGE_SIZE));
        proc->kernel_stack = 0;
    }
    
    serial_puts(SERIAL_COM1, "Process exited: PID=");
    serial_put_dec(SERIAL_COM1, proc->pid);
    serial_puts(SERIAL_COM1, ", exit_code=");
    serial_put_dec(SERIAL_COM1, exit_code);
    serial_puts(SERIAL_COM1, "\n");
}

struct process* process_get_current(void) {
    return current_process;
}

struct process* process_find_by_pid(uint64_t pid) {
    for (int i = 0; i < MAX_PROCESSES; i++) {
        if (process_table[i].pid == pid) {
            return &process_table[i];
        }
    }
    return NULL;
}
