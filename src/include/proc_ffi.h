#ifndef _PROC_FFI_H
#define _PROC_FFI_H

#include "types.h"
#include "proc.h"

#ifdef __cplusplus
extern "C" {
#endif

void process_init(void);
void session_init(void);
void scheduler_init(void);
void kernel_init(void);
void scheduler_add(uint32_t pid);
void scheduler_yield(void);
void process_exit(uint32_t exit_code);

uint32_t process_create(const char *name, uint32_t parent_pid);

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

void sched_add_internal(uint32_t pid);
uint32_t sched_schedule_internal(void);
void scheduler_yield(void);

struct process* process_find_by_pid(uint64_t pid);

#ifdef __cplusplus
}
#endif

#endif
