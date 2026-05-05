#ifndef _TIMER_H
#define _TIMER_H

#include "types.h"
#include "io.h"

void timer_init(void);
uint64_t timer_get_ticks(void);
void timer_sleep(uint64_t ms);
void timer_sleep_busy(uint64_t ms);

#endif
