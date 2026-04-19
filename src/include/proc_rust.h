#ifndef _PROC_RUST_H
#define _PROC_RUST_H

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

// 进程管理接口
uint32_t rust_proc_create(const char *name, uint32_t parent_pid);
void rust_proc_exit(uint32_t exit_code);
uint32_t rust_proc_get_current(void);
void rust_proc_yield(void);
void rust_proc_block(uint32_t reason);
void rust_proc_unblock(uint32_t pid);
int32_t rust_proc_set_priority(uint32_t pid, uint32_t priority);
uint32_t rust_proc_get_state(uint32_t pid);
int32_t rust_proc_get_exit_code(uint32_t pid);
int32_t rust_proc_is_initialized(void);

// 调度器接口
void rust_sched_init(void);
void rust_sched_add(uint32_t pid);
uint32_t rust_sched_schedule(void);
int32_t rust_sched_should_reschedule(void);
void rust_sched_set_current(uint32_t pid);
uint32_t rust_sched_get_current(void);

// 内核初始化
void rust_kernel_init(void);

#ifdef __cplusplus
}
#endif

#endif // _PROC_RUST_H
