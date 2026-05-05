#ifndef _SCHEDULER_H
#define _SCHEDULER_H

/* 前向声明: 确保 pid_t 和 tid_t 在 thread.h 的 #ifdef _PROC_H 之前可用 */
#ifndef pid_t
typedef uint64_t pid_t;
#endif
#ifndef tid_t
typedef uint64_t tid_t;
#endif

#include "thread.h"
#include "types.h"

#define SCHED_LEVELS           4
#define SCHED_LEVEL_0_QUANTUM  2
#define SCHED_LEVEL_1_QUANTUM  4
#define SCHED_LEVEL_2_QUANTUM  8
#define SCHED_LEVEL_3_QUANTUM  16

#define SCHED_BOOST_INTERVAL   1000

/* 实时调度策略 (与Linux兼容) */
#define SCHED_NORMAL    0
#define SCHED_FIFO      1
#define SCHED_RR        2
#define SCHED_IDLE      3

#define RT_PRIORITY_MAX     99
#define RT_PRIORITY_MIN     1
#define RT_TIME_SLICE       5

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

/* 注意: struct scheduler 已在 proc.h 中定义，此处不重复定义 */

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

/* 实时调度扩展接口 */
extern void scheduler_add_rt_task(uint32_t pid, uint8_t rt_priority, uint32_t policy);
extern int scheduler_set_sched_policy(uint32_t pid, uint32_t policy, uint8_t rt_priority);
extern size_t scheduler_get_rt_count(void);
extern void scheduler_boost_priority(void);

extern struct scheduler sched_ex;

#endif
