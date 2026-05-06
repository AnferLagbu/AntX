#ifndef _THREAD_H
#define _THREAD_H

#include "types.h"

#ifndef pid_t
typedef uint64_t pid_t;
#endif

#ifndef tid_t
typedef uint64_t tid_t;
#endif

enum thread_state {
    THREAD_CREATED = 0,
    THREAD_READY,
    THREAD_RUNNING,
    THREAD_BLOCKED,
    THREAD_ZOMBIE,
    THREAD_EXITED
};

enum thread_priority {
    PRIORITY_IDLE = 0,
    PRIORITY_LOW,
    PRIORITY_NORMAL,
    PRIORITY_HIGH,
    PRIORITY_RT
};

enum block_reason {
    BLOCK_NONE = 0,
    BLOCK_SLEEP,
    BLOCK_WAIT,
    BLOCK_IO,
    BLOCK_PIPE,
    BLOCK_SEM,
    BLOCK_MQ
};

struct thread {
    uint64_t tid;
    uint64_t pid;
    uint32_t state;
    uint32_t priority;
    uint32_t block_reason;
    uint64_t reserved[4];
};

struct wait_queue {
    struct thread *head;
    struct thread *tail;
    uint32_t count;
};

void wait_queue_init(struct wait_queue *wq);
void wait_queue_add(struct wait_queue *wq, struct thread *thread);
struct thread *wait_queue_pop(struct wait_queue *wq);
struct thread *wait_queue_peek(struct wait_queue *wq);
int wait_queue_remove(struct wait_queue *wq, struct thread *thread);
void wait_queue_wake_one(struct wait_queue *wq);
void wait_queue_wake_all(struct wait_queue *wq);

struct thread *thread_get_current(void);

#endif
