#ifndef _TIME_H
#define _TIME_H

#include "types.h"

typedef uint64_t time_t;

static inline const char *ctime(const time_t *t) {
    (void)t;
    return "Thu Jan 01 00:00:00 1970\n";
}

#endif
