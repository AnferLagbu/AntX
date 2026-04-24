#ifndef _PROC_FFI_H
#define _PROC_FFI_H

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

uint32_t proc_create_internal(const char *name, uint32_t parent_pid);
void proc_exit_internal(uint32_t exit_code);
uint32_t proc_get_current_pid_internal(void);
void proc_yield_internal(void);
void proc_block(uint32_t reason);
void proc_unblock(uint32_t pid);
int32_t proc_set_priority(uint32_t pid, uint32_t priority);
uint32_t proc_get_state(uint32_t pid);
int32_t proc_get_exit_code(uint32_t pid);
int32_t proc_is_initialized(void);

void sched_init_internal(void);
void sched_add_internal(uint32_t pid);
uint32_t sched_schedule_internal(void);
int32_t sched_should_reschedule(void);
void sched_set_current(uint32_t pid);
uint32_t sched_get_current(void);

void kernel_init(void);

#ifdef __cplusplus
}
#endif

#endif
