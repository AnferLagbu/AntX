#include "scheduler_ex.h"
#include "thread.h"
#include "serial.h"
#include "timer.h"
#include "gdt.h"
#include "string.h"

struct scheduler sched_ex;

static int priority_to_level(enum thread_priority priority) {
    if (priority >= PRIORITY_REALTIME) return 0;
    if (priority >= PRIORITY_HIGH) return 1;
    if (priority >= PRIORITY_NORMAL) return 2;
    return 3;
}

static int level_to_quantum(int level) {
    switch (level) {
        case 0: return SCHED_LEVEL_0_QUANTUM;
        case 1: return SCHED_LEVEL_1_QUANTUM;
        case 2: return SCHED_LEVEL_2_QUANTUM;
        case 3: return SCHED_LEVEL_3_QUANTUM;
        default: return SCHED_LEVEL_3_QUANTUM;
    }
}

void scheduler_init_ex(void) {
    memset(&sched_ex, 0, sizeof(struct scheduler));
    
    for (int i = 0; i < SCHED_LEVELS; i++) {
        sched_ex.runq.queues[i] = NULL;
        sched_ex.runq.counts[i] = 0;
    }
    sched_ex.runq.total = 0;
    
    sched_ex.current = NULL;
    sched_ex.idle_thread = NULL;
    sched_ex.tick_count = 0;
    sched_ex.last_boost = 0;
    sched_ex.need_reschedule = 0;
    
    memset(&sched_ex.stats, 0, sizeof(struct scheduler_stats));
    
    serial_puts(SERIAL_COM1, "[SCHED] Multi-level feedback queue scheduler initialized\n");
}

static void run_queue_add(struct run_queue *runq, struct thread *thread, int level) {
    if (level < 0 || level >= SCHED_LEVELS) level = SCHED_LEVELS - 1;
    
    thread->time_slice = level_to_quantum(level);
    
    if (runq->queues[level] == NULL) {
        runq->queues[level] = thread;
        thread->next = thread;
        thread->prev = thread;
    } else {
        struct thread *tail = runq->queues[level]->prev;
        tail->next = thread;
        thread->prev = tail;
        thread->next = runq->queues[level];
        runq->queues[level]->prev = thread;
    }
    
    runq->counts[level]++;
    runq->total++;
}

static struct thread *run_queue_pop(struct run_queue *runq, int level) {
    if (level < 0 || level >= SCHED_LEVELS) return NULL;
    if (runq->queues[level] == NULL) return NULL;
    
    struct thread *thread = runq->queues[level];
    
    if (thread->next == thread) {
        runq->queues[level] = NULL;
    } else {
        thread->prev->next = thread->next;
        thread->next->prev = thread->prev;
        runq->queues[level] = thread->next;
    }
    
    thread->next = NULL;
    thread->prev = NULL;
    
    runq->counts[level]--;
    runq->total--;
    
    return thread;
}

static struct thread *run_queue_pop_highest(struct run_queue *runq) {
    for (int level = 0; level < SCHED_LEVELS; level++) {
        while (runq->queues[level] != NULL) {
            struct thread *thread = run_queue_pop(runq, level);
            if (thread != NULL && thread->state == THREAD_READY) {
                return thread;
            }
        }
    }
    return NULL;
}

void scheduler_add_thread(struct thread *thread) {
    if (thread == NULL) return;
    
    thread->state = THREAD_READY;
    
    int level = priority_to_level(thread->priority);
    run_queue_add(&sched_ex.runq, thread, level);
    
    serial_puts(SERIAL_COM1, "[SCHED] Added thread TID=");
    serial_put_dec(SERIAL_COM1, thread->tid);
    serial_puts(SERIAL_COM1, " to level ");
    serial_put_dec(SERIAL_COM1, level);
    serial_puts(SERIAL_COM1, "\n");
}

void scheduler_remove_thread(struct thread *thread) {
    if (thread == NULL) return;
    
    if (thread->next != NULL && thread->prev != NULL) {
        if (thread->next == thread) {
            for (int level = 0; level < SCHED_LEVELS; level++) {
                if (sched_ex.runq.queues[level] == thread) {
                    sched_ex.runq.queues[level] = NULL;
                    sched_ex.runq.counts[level]--;
                    break;
                }
            }
        } else {
            thread->prev->next = thread->next;
            thread->next->prev = thread->prev;
            
            for (int level = 0; level < SCHED_LEVELS; level++) {
                if (sched_ex.runq.queues[level] == thread) {
                    sched_ex.runq.queues[level] = thread->next;
                    sched_ex.runq.counts[level]--;
                    break;
                }
            }
        }
        
        thread->next = NULL;
        thread->prev = NULL;
        sched_ex.runq.total--;
    }
}

void scheduler_tick_ex(void) {
    sched_ex.tick_count++;
    
    if (sched_ex.current != NULL) {
        sched_ex.current->time_slice--;
        sched_ex.current->cpu_time++;
        
        if (sched_ex.current->sleep_until != 0) {
            extern uint64_t timer_get_ticks(void);
            if (timer_get_ticks() >= sched_ex.current->sleep_until) {
                sched_ex.current->sleep_until = 0;
                thread_unblock(sched_ex.current);
            }
        }
        
        if (sched_ex.current->time_slice <= 0) {
            sched_ex.need_reschedule = 1;
        }
    }
    
    if (sched_ex.tick_count - sched_ex.last_boost >= SCHED_BOOST_INTERVAL) {
        scheduler_boost_all();
        sched_ex.last_boost = sched_ex.tick_count;
    }
    
    if (sched_ex.need_reschedule) {
        scheduler_schedule_ex();
    }
}

void scheduler_schedule_ex(void) {
    struct thread *prev = sched_ex.current;
    struct thread *next = NULL;
    
    if (prev != NULL) {
        if (prev->state == THREAD_BLOCKED) {
            scheduler_remove_thread(prev);
        } else if (prev->state == THREAD_RUNNING) {
            int level = priority_to_level(prev->priority);
            if (level < SCHED_LEVELS - 1) {
                level++;
            }
            prev->state = THREAD_READY;
            run_queue_add(&sched_ex.runq, prev, level);
        }
    }
    
    next = run_queue_pop_highest(&sched_ex.runq);
    
    if (next == NULL) {
        if (sched_ex.idle_thread != NULL) {
            next = sched_ex.idle_thread;
        } else if (prev != NULL && prev->state == THREAD_RUNNING) {
            return;
        }
        return;
    }
    
    next->state = THREAD_RUNNING;
    sched_ex.current = next;
    thread_set_current(next);
    
    sched_ex.need_reschedule = 0;
    sched_ex.stats.context_switches++;
    
    serial_puts(SERIAL_COM1, "[SCHED] Switch to TID=");
    serial_put_dec(SERIAL_COM1, next->tid);
    serial_puts(SERIAL_COM1, " PID=");
    serial_put_dec(SERIAL_COM1, next->pid);
    serial_puts(SERIAL_COM1, "\n");
    
    extern void tss_set_kernel_stack(uint64_t rsp0);
    tss_set_kernel_stack(next->kernel_stack);
    
    if (prev == NULL || prev == next) {
        if (next->context.cs == 0x1B) {
            uint64_t ss_val = next->context.ss;
            uint64_t rsp_val = next->context.rsp;
            uint64_t cs_val = next->context.cs;
            uint64_t rip_val = next->context.rip;
            uint64_t rflags_val = next->context.rflags;
            
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
                "pushq %2\n"
                "movq %3, %%rax\n"
                "pushq %%rax\n"
                "movq %4, %%rax\n"
                "pushq %%rax\n"
                "mov %5, %%cr3\n"
                "iretq\n"
                :
                : "r"(ss_val), "r"(rsp_val), "r"(rflags_val), "r"(cs_val), "r"(rip_val), "r"(next->context.cr3)
                : "rax", "memory"
            );
        }
        return;
    }
    
    if (prev->context.cr3 != next->context.cr3) {
        __asm__ volatile ("mov %0, %%cr3" : : "r"(next->context.cr3));
    }
    
    extern void process_switch_asm(struct cpu_context *old, struct cpu_context *new_ctx);
    process_switch_asm(&prev->context, &next->context);
}

void scheduler_yield_ex(void) {
    sched_ex.need_reschedule = 1;
    scheduler_schedule_ex();
}

struct thread *scheduler_get_current(void) {
    return sched_ex.current;
}

struct thread *scheduler_pick_next(void) {
    return run_queue_pop_highest(&sched_ex.runq);
}

void scheduler_boost_all(void) {
    serial_puts(SERIAL_COM1, "[SCHED] Priority boost\n");
    
    for (int level = 1; level < SCHED_LEVELS; level++) {
        while (sched_ex.runq.queues[level] != NULL) {
            struct thread *thread = run_queue_pop(&sched_ex.runq, level);
            if (thread != NULL) {
                run_queue_add(&sched_ex.runq, thread, 0);
            }
        }
    }
}

void scheduler_update_priority(struct thread *thread, enum thread_priority new_priority) {
    if (thread == NULL) return;
    
    thread->priority = new_priority;
    
    if (thread->state == THREAD_READY && thread->next != NULL) {
        scheduler_remove_thread(thread);
        scheduler_add_thread(thread);
    }
}

void scheduler_block_current(enum block_reason reason) {
    if (sched_ex.current == NULL) return;
    
    thread_block(sched_ex.current, reason);
    sched_ex.need_reschedule = 1;
}

void scheduler_unblock_thread(struct thread *thread) {
    if (thread == NULL) return;
    thread_unblock(thread);
}

int scheduler_set_thread_priority(tid_t tid, enum thread_priority priority) {
    struct thread *thread = thread_find_by_tid(tid);
    if (thread == NULL) return -1;
    
    scheduler_update_priority(thread, priority);
    return 0;
}

enum thread_priority scheduler_get_thread_priority(tid_t tid) {
    struct thread *thread = thread_find_by_tid(tid);
    if (thread == NULL) return PRIORITY_NORMAL;
    
    return thread->priority;
}

void scheduler_dump_state(void) {
    serial_puts(SERIAL_COM1, "=== Scheduler State ===\n");
    serial_puts(SERIAL_COM1, "Current TID: ");
    if (sched_ex.current) {
        serial_put_dec(SERIAL_COM1, sched_ex.current->tid);
    } else {
        serial_puts(SERIAL_COM1, "none");
    }
    serial_puts(SERIAL_COM1, "\n");
    
    serial_puts(SERIAL_COM1, "Run queues:\n");
    for (int i = 0; i < SCHED_LEVELS; i++) {
        serial_puts(SERIAL_COM1, "  Level ");
        serial_put_dec(SERIAL_COM1, i);
        serial_puts(SERIAL_COM1, ": ");
        serial_put_dec(SERIAL_COM1, sched_ex.runq.counts[i]);
        serial_puts(SERIAL_COM1, " threads\n");
    }
    
    serial_puts(SERIAL_COM1, "Total: ");
    serial_put_dec(SERIAL_COM1, sched_ex.runq.total);
    serial_puts(SERIAL_COM1, " runnable threads\n");
    
    serial_puts(SERIAL_COM1, "Context switches: ");
    serial_put_dec(SERIAL_COM1, sched_ex.stats.context_switches);
    serial_puts(SERIAL_COM1, "\n");
}
