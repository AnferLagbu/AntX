#ifndef QX_RWLOCK_H
#define QX_RWLOCK_H

#include <stdint.h>

typedef struct {
    uint32_t readers;
    uint32_t writer;
    mutex_t  inner;
} rwlock_t;

#endif
