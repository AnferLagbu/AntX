#ifndef QX_MUTEX_H
#define QX_MUTEX_H

#include <stdint.h>

typedef struct {
    uint32_t locked;
    int32_t  owner;
    uint32_t depth;
    uint64_t acquire_time;
    uint32_t inner_spinlock_locked;
    uint32_t inner_spinlock_irq_flags;
} mutex_t;

void mutex_init(mutex_t *m);
void mutex_lock(const mutex_t *m);
void mutex_unlock(const mutex_t *m);
int  mutex_trylock(const mutex_t *m);
int  mutex_is_locked(const mutex_t *m);

#endif
