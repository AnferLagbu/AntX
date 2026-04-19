#ifndef _SCHEDULER_H
#define _SCHEDULER_H

#include "thread.h"

#define SCHED_LEVELS           4
#define SCHED_LEVEL_0_QUANTUM  2
#define SCHED_LEVEL_1_QUANTUM  4
#define SCHED_LEVEL_2_QUANTUM  8
#define SCHED_LEVEL_3_QUANTUM  16

#define SCHED_BOOST_INTERVAL   1000

struct run_queue {
    struct thread *queues[SCHED_LEVELS];
    uint32_t counts[SCHED_LEVELS];
    uint32_t total;
};

struct scheduler_stats {
    uint64_t context_switches;
    uint64_t total_cpu_time;
    uint64_t idle_time;
    uint32_t runnable_threads;
    uint32_t blocked_threads;
};

struct scheduler {
    struct run_queue runq;
    struct thread *current;
    struct thread *idle_thread;
    
    uint64_t tick_count;
    uint64_t last_boost;
    
    struct scheduler_stats stats;
    
    int need_reschedule;
};

void scheduler_init_ex(void);
void scheduler_add_thread(struct thread *thread);
void scheduler_remove_thread(struct thread *thread);
void scheduler_tick_ex(void);
void scheduler_schedule_ex(void);
void scheduler_yield_ex(void);

struct thread *scheduler_get_current(void);
struct thread *scheduler_pick_next(void);

void scheduler_boost_all(void);
void scheduler_update_priority(struct thread *thread, enum thread_priority new_priority);

void scheduler_block_current(enum block_reason reason);
void scheduler_unblock_thread(struct thread *thread);

int scheduler_set_thread_priority(tid_t tid, enum thread_priority priority);
enum thread_priority scheduler_get_thread_priority(tid_t tid);

void scheduler_dump_state(void);

extern struct scheduler sched_ex;

#endif
