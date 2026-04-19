#include "thread.h"
#include "mm.h"
#include "serial.h"
#include "string.h"
#include "scheduler_ex.h"

static struct thread thread_table[MAX_THREADS_PER_PROCESS * MAX_PROCESSES];
static struct process process_table[MAX_PROCESSES];
static struct thread *current_thread = NULL;
static tid_t next_tid = 1;
static pid_t next_pid = 1;

void thread_init(void) {
    memset(thread_table, 0, sizeof(thread_table));
    memset(process_table, 0, sizeof(process_table));
    current_thread = NULL;
    next_tid = 1;
    next_pid = 1;
    
    serial_puts(SERIAL_COM1, "[THREAD] Thread system initialized\n");
}

static struct thread *thread_alloc(void) {
    for (size_t i = 0; i < sizeof(thread_table) / sizeof(thread_table[0]); i++) {
        if (thread_table[i].state == THREAD_CREATED && thread_table[i].tid == 0) {
            return &thread_table[i];
        }
    }
    return NULL;
}

static struct process *process_alloc(void) {
    for (size_t i = 0; i < MAX_PROCESSES; i++) {
        if (process_table[i].pid == 0) {
            return &process_table[i];
        }
    }
    return NULL;
}

struct thread *thread_create(pid_t pid, void (*entry)(void *), void *arg, enum thread_priority priority) {
    struct thread *thread = thread_alloc();
    if (thread == NULL) {
        serial_puts(SERIAL_COM1, "[THREAD] Failed to allocate thread\n");
        return NULL;
    }
    
    struct process *proc = process_get_by_pid(pid);
    if (proc == NULL) {
        serial_puts(SERIAL_COM1, "[THREAD] Invalid process PID\n");
        return NULL;
    }
    
    thread->tid = next_tid++;
    thread->pid = pid;
    thread->state = THREAD_READY;
    thread->priority = priority;
    thread->block_reason = BLOCK_NONE;
    
    thread->kernel_stack = (uint64_t)pmm_alloc_pages(KERNEL_STACK_SIZE / PAGE_SIZE);
    if (thread->kernel_stack == 0) {
        serial_puts(SERIAL_COM1, "[THREAD] Failed to allocate kernel stack\n");
        return NULL;
    }
    thread->kernel_stack += KERNEL_STACK_SIZE;
    
    thread->user_stack = 0;
    thread->user_stack_base = 0;
    thread->user_stack_size = 0;
    
    memset(&thread->context, 0, sizeof(struct cpu_context));
    thread->context.rip = (uint64_t)entry;
    thread->context.rsp = thread->kernel_stack;
    thread->context.rflags = 0x202;
    thread->context.cr3 = proc->cr3;
    
    thread->cpu_time = 0;
    thread->start_time = 0;
    thread->sleep_until = 0;
    thread->time_slice = DEFAULT_TIME_SLICE;
    
    thread->tls_base = NULL;
    thread->entry_point = entry;
    thread->entry_arg = arg;
    
    thread->next = NULL;
    thread->prev = NULL;
    thread->process_next = NULL;
    
    thread->exit_code = 0;
    thread->wait_tid = 0;
    
    if (proc->thread_list == NULL) {
        proc->thread_list = thread;
        proc->main_thread = thread;
    } else {
        struct thread *t = proc->thread_list;
        while (t->process_next != NULL) {
            t = t->process_next;
        }
        t->process_next = thread;
    }
    proc->thread_count++;
    
    serial_puts(SERIAL_COM1, "[THREAD] Created thread TID=");
    serial_put_dec(SERIAL_COM1, thread->tid);
    serial_puts(SERIAL_COM1, " PID=");
    serial_put_dec(SERIAL_COM1, pid);
    serial_puts(SERIAL_COM1, "\n");
    
    return thread;
}

struct thread *thread_create_user(pid_t pid, uint64_t entry, uint64_t stack_top, enum thread_priority priority) {
    struct thread *thread = thread_alloc();
    if (thread == NULL) {
        return NULL;
    }
    
    struct process *proc = process_get_by_pid(pid);
    if (proc == NULL) {
        return NULL;
    }
    
    thread->tid = next_tid++;
    thread->pid = pid;
    thread->state = THREAD_READY;
    thread->priority = priority;
    thread->block_reason = BLOCK_NONE;
    
    thread->kernel_stack = (uint64_t)pmm_alloc_pages(KERNEL_STACK_SIZE / PAGE_SIZE);
    if (thread->kernel_stack == 0) {
        return NULL;
    }
    thread->kernel_stack += KERNEL_STACK_SIZE;
    
    thread->user_stack = stack_top;
    thread->user_stack_base = stack_top - USER_STACK_SIZE;
    thread->user_stack_size = USER_STACK_SIZE;
    
    memset(&thread->context, 0, sizeof(struct cpu_context));
    thread->context.rip = entry;
    thread->context.cs = 0x1B;
    thread->context.rsp = stack_top;
    thread->context.ss = 0x23;
    thread->context.rflags = 0x202;
    thread->context.cr3 = proc->cr3;
    
    thread->cpu_time = 0;
    thread->start_time = 0;
    thread->time_slice = DEFAULT_TIME_SLICE;
    
    thread->next = NULL;
    thread->prev = NULL;
    thread->process_next = NULL;
    
    if (proc->thread_list == NULL) {
        proc->thread_list = thread;
        proc->main_thread = thread;
    } else {
        struct thread *t = proc->thread_list;
        while (t->process_next != NULL) {
            t = t->process_next;
        }
        t->process_next = thread;
    }
    proc->thread_count++;
    
    return thread;
}

void thread_exit(struct thread *thread, int exit_code) {
    if (thread == NULL) return;
    
    thread->exit_code = exit_code;
    thread->state = THREAD_ZOMBIE;
    
    struct process *proc = process_get_by_pid(thread->pid);
    if (proc != NULL) {
        proc->thread_count--;
        
        if (thread == proc->main_thread) {
            process_exit_ex(proc, exit_code);
        }
    }
    
    if (thread->kernel_stack) {
        for (int i = 0; i < KERNEL_STACK_SIZE / PAGE_SIZE; i++) {
            pmm_free_page((void*)(thread->kernel_stack - KERNEL_STACK_SIZE + i * PAGE_SIZE));
        }
        thread->kernel_stack = 0;
    }
    
    serial_puts(SERIAL_COM1, "[THREAD] Thread exited TID=");
    serial_put_dec(SERIAL_COM1, thread->tid);
    serial_puts(SERIAL_COM1, " exit_code=");
    serial_put_dec(SERIAL_COM1, exit_code);
    serial_puts(SERIAL_COM1, "\n");
}

void thread_block(struct thread *thread, enum block_reason reason) {
    if (thread == NULL) return;
    
    thread->state = THREAD_BLOCKED;
    thread->block_reason = reason;
    
    serial_puts(SERIAL_COM1, "[THREAD] Blocked TID=");
    serial_put_dec(SERIAL_COM1, thread->tid);
    serial_puts(SERIAL_COM1, " reason=");
    serial_put_dec(SERIAL_COM1, reason);
    serial_puts(SERIAL_COM1, "\n");
}

void thread_unblock(struct thread *thread) {
    if (thread == NULL) return;
    if (thread->state != THREAD_BLOCKED) return;
    
    thread->state = THREAD_READY;
    thread->block_reason = BLOCK_NONE;
    
    scheduler_add_thread(thread);
    
    serial_puts(SERIAL_COM1, "[THREAD] Unblocked TID=");
    serial_put_dec(SERIAL_COM1, thread->tid);
    serial_puts(SERIAL_COM1, "\n");
}

void thread_yield(void) {
    if (current_thread != NULL) {
        current_thread->state = THREAD_READY;
        scheduler_yield_ex();
    }
}

void thread_sleep(uint64_t ms) {
    if (current_thread == NULL) return;
    
    extern uint64_t timer_get_ticks(void);
    current_thread->sleep_until = timer_get_ticks() + ms;
    thread_block(current_thread, BLOCK_SLEEPING);
    scheduler_yield_ex();
}

struct thread *thread_get_current(void) {
    return current_thread;
}

void thread_set_current(struct thread *thread) {
    current_thread = thread;
}

struct thread *thread_find_by_tid(tid_t tid) {
    for (size_t i = 0; i < sizeof(thread_table) / sizeof(thread_table[0]); i++) {
        if (thread_table[i].tid == tid) {
            return &thread_table[i];
        }
    }
    return NULL;
}

struct process *process_create_ex(const char *name, pid_t parent_pid, uint64_t pwid) {
    struct process *proc = process_alloc();
    if (proc == NULL) {
        serial_puts(SERIAL_COM1, "[PROCESS] Failed to allocate process\n");
        return NULL;
    }
    
    proc->pid = next_pid++;
    proc->parent_pid = parent_pid;
    proc->pwid = pwid;
    proc->session_id = 0;
    
    strncpy(proc->name, name, sizeof(proc->name) - 1);
    proc->name[sizeof(proc->name) - 1] = '\0';
    
    proc->cr3 = vmm_create_user_page_table();
    if (proc->cr3 == 0) {
        serial_puts(SERIAL_COM1, "[PROCESS] Failed to create page table\n");
        proc->pid = 0;
        return NULL;
    }
    
    proc->main_thread = NULL;
    proc->thread_list = NULL;
    proc->thread_count = 0;
    
    proc->parent = NULL;
    proc->children = NULL;
    proc->sibling = NULL;
    
    if (parent_pid != 0) {
        struct process *parent = process_get_by_pid(parent_pid);
        if (parent != NULL) {
            proc->parent = parent;
            proc->sibling = parent->children;
            parent->children = proc;
            
            strncpy(proc->cwd, parent->cwd, sizeof(proc->cwd) - 1);
            strncpy(proc->root, parent->root, sizeof(proc->root) - 1);
        }
    } else {
        strcpy(proc->cwd, "/");
        strcpy(proc->root, "/");
    }
    
    proc->exit_code = 0;
    proc->exit_status = 0;
    
    extern uint64_t timer_get_ticks(void);
    proc->start_time = timer_get_ticks();
    proc->cpu_time = 0;
    
    proc->heap.brk_start = NULL;
    proc->heap.brk_current = NULL;
    proc->heap.brk_end = NULL;
    
    proc->umask = 022;
    
    proc->stdio.stdin_fd = 0;
    proc->stdio.stdout_fd = 1;
    proc->stdio.stderr_fd = 2;
    
    serial_puts(SERIAL_COM1, "[PROCESS] Created PID=");
    serial_put_dec(SERIAL_COM1, proc->pid);
    serial_puts(SERIAL_COM1, " name=");
    serial_puts(SERIAL_COM1, proc->name);
    serial_puts(SERIAL_COM1, "\n");
    
    return proc;
}

void process_exit_ex(struct process *proc, int exit_code) {
    if (proc == NULL) return;
    
    proc->exit_code = exit_code;
    proc->exit_status = 0;
    
    struct thread *thread = proc->thread_list;
    while (thread != NULL) {
        if (thread->state != THREAD_ZOMBIE && thread->state != THREAD_EXITED) {
            thread->state = THREAD_ZOMBIE;
        }
        thread = thread->process_next;
    }
    
    if (proc->parent != NULL) {
        struct thread *parent_thread = proc->parent->main_thread;
        if (parent_thread != NULL && parent_thread->block_reason == BLOCK_WAITPID) {
            thread_unblock(parent_thread);
        }
    }
    
    serial_puts(SERIAL_COM1, "[PROCESS] Exited PID=");
    serial_put_dec(SERIAL_COM1, proc->pid);
    serial_puts(SERIAL_COM1, " exit_code=");
    serial_put_dec(SERIAL_COM1, exit_code);
    serial_puts(SERIAL_COM1, "\n");
}

struct process *process_get_by_pid(pid_t pid) {
    for (size_t i = 0; i < MAX_PROCESSES; i++) {
        if (process_table[i].pid == pid) {
            return &process_table[i];
        }
    }
    return NULL;
}

pid_t process_get_current_pid(void) {
    if (current_thread == NULL) return 0;
    return current_thread->pid;
}

void wait_queue_init(struct wait_queue *wq) {
    wq->head = NULL;
    wq->count = 0;
}

void wait_queue_add(struct wait_queue *wq, struct thread *thread) {
    if (wq == NULL || thread == NULL) return;
    
    thread->next = wq->head;
    thread->prev = NULL;
    if (wq->head != NULL) {
        wq->head->prev = thread;
    }
    wq->head = thread;
    wq->count++;
    
    thread_block(thread, BLOCK_WAITING);
}

void wait_queue_wake_one(struct wait_queue *wq) {
    if (wq == NULL || wq->head == NULL) return;
    
    struct thread *thread = wq->head;
    wq->head = thread->next;
    if (wq->head != NULL) {
        wq->head->prev = NULL;
    }
    wq->count--;
    
    thread_unblock(thread);
}

void wait_queue_wake_all(struct wait_queue *wq) {
    if (wq == NULL) return;
    
    while (wq->head != NULL) {
        wait_queue_wake_one(wq);
    }
}

int wait_queue_wait(struct wait_queue *wq, uint64_t timeout_ms) {
    if (wq == NULL || current_thread == NULL) return -1;
    
    wait_queue_add(wq, current_thread);
    scheduler_yield_ex();
    
    return 0;
}
