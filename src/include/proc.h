#ifndef _PROC_H
#define _PROC_H

#include "types.h"

#define MAX_PROCESSES   256
#define MAX_SESSIONS    16
#define TIME_SLICE      10

#define STACK_SIZE      65536

enum process_state {
    PROC_NEW = 0,
    PROC_READY,
    PROC_RUNNING,
    PROC_BLOCKED,
    PROC_EXITED,
    PROC_ZOMBIE
};

enum session_state {
    SESSION_ACTIVE = 0,
    SESSION_IDLE,
    SESSION_LOGGING_IN,
    SESSION_LOGGING_OUT,
    SESSION_ZOMBIE
};

struct cpu_context {
    uint64_t r15, r14, r13, r12, r11, r10, r9, r8;
    uint64_t rdi, rsi, rbp, rbx, rdx, rcx, rax;
    uint64_t rip, cs, rflags, rsp, ss;
    uint64_t cr3;
};

struct process {
    uint64_t pid;
    uint64_t session_id;
    uint64_t parent_pid;
    uint64_t pwid;
    
    enum process_state state;
    uint64_t exit_code;
    
    int priority;
    uint64_t cpu_time;
    uint64_t start_time;
    uint64_t time_slice;

    uint64_t cr3;
    uint64_t kernel_stack;
    uint64_t user_stack;

    struct cpu_context context;

    struct process *next;
    struct process *prev;
    struct process *parent;
    struct process *children;
    struct process *sibling;
};

struct session {
    uint64_t session_id;
    uint64_t pwid;
    uint64_t parent_sid;
    uint64_t terminal;
    uint64_t create_time;
    
    enum session_state state;
    
    struct process *process_list;
    uint64_t process_count;
    
    struct session *next;
};

struct process* process_get_current(void);
struct process* process_get_by_pid(uint64_t pid);
void process_exit(struct process *proc, int exit_code);

bool proc_has_runnable(void);
uint64_t scheduler_next_pid(void);

#endif
