#ifndef _THREAD_H
#define _THREAD_H

#include "types.h"

#ifndef _PROC_H
#define MAX_THREADS_PER_PROCESS  256
#define MAX_PROCESSES            256
#define DEFAULT_TIME_SLICE       10
#define MAX_PRIORITY             31
#define MIN_PRIORITY             0

#define KERNEL_STACK_SIZE        65536
#define USER_STACK_SIZE          1048576

typedef uint64_t tid_t;
typedef uint64_t pid_t;
#endif

enum thread_state {
    THREAD_CREATED = 0,
    THREAD_READY,
    THREAD_RUNNING,
    THREAD_BLOCKED,
    THREAD_EXITED,
    THREAD_ZOMBIE
};

enum thread_priority {
    PRIORITY_IDLE = 0,
    PRIORITY_LOW = 8,
    PRIORITY_NORMAL = 16,
    PRIORITY_HIGH = 24,
    PRIORITY_REALTIME = 31
};

enum block_reason {
    BLOCK_NONE = 0,
    BLOCK_WAITING,
    BLOCK_SLEEPING,
    BLOCK_IO,
    BLOCK_MUTEX,
    BLOCK_SEMAPHORE,
    BLOCK_WAITPID,
    BLOCK_READ,
    BLOCK_WRITE
};

#ifndef _PROC_H
struct cpu_context {
    uint64_t r15, r14, r13, r12, r11, r10, r9, r8;
    uint64_t rdi, rsi, rbp, rbx, rdx, rcx, rax;
    uint64_t rip, cs, rflags, rsp, ss;
    uint64_t cr3;
    uint64_t fs_base;
    uint64_t gs_base;
};
#endif

struct thread {
    tid_t tid;
    pid_t pid;
    
    enum thread_state state;
    enum thread_priority priority;
    enum block_reason block_reason;
    
    uint64_t kernel_stack;
    uint64_t user_stack;
    uint64_t user_stack_base;
    uint64_t user_stack_size;
    
    struct cpu_context context;
    
    uint64_t cpu_time;
    uint64_t start_time;
    uint64_t sleep_until;
    int32_t time_slice;
    
    void *tls_base;
    void *entry_point;
    void *entry_arg;
    
    struct thread *next;
    struct thread *prev;
    struct thread *process_next;
    
    int exit_code;
    uint64_t wait_tid;
};

#ifndef _PROC_H
struct process {
    pid_t pid;
    pid_t parent_pid;
    uint64_t pwid;
    uint64_t session_id;
    
    char name[64];
    
    uint64_t cr3;
    
    struct thread *main_thread;
    struct thread *thread_list;
    uint32_t thread_count;
    
    struct process *parent;
    struct process *children;
    struct process *sibling;
    
    int exit_code;
    int exit_status;
    
    uint64_t start_time;
    uint64_t cpu_time;
    
    struct {
        void *brk_start;
        void *brk_current;
        void *brk_end;
    } heap;
    
    char cwd[256];
    char root[256];
    
    uint64_t umask;
    
    struct {
        int stdin_fd;
        int stdout_fd;
        int stderr_fd;
    } stdio;
};
#endif

struct wait_queue {
    struct thread *head;
    uint32_t count;
};

struct thread *thread_create(pid_t pid, void (*entry)(void *), void *arg, enum thread_priority priority);
void thread_exit(struct thread *thread, int exit_code);
void thread_block(struct thread *thread, enum block_reason reason);
void thread_unblock(struct thread *thread);
void thread_yield(void);
void thread_sleep(uint64_t ms);
struct thread *thread_get_current(void);
void thread_set_current(struct thread *thread);
struct thread *thread_find_by_tid(tid_t tid);

struct process *process_create_ex(const char *name, pid_t parent_pid, uint64_t pwid);
void process_exit_ex(struct process *proc, int exit_code);
struct process *process_get_by_pid(pid_t pid);
pid_t process_get_current_pid(void);

void wait_queue_init(struct wait_queue *wq);
void wait_queue_add(struct wait_queue *wq, struct thread *thread);
void wait_queue_wake_one(struct wait_queue *wq);
void wait_queue_wake_all(struct wait_queue *wq);
int wait_queue_wait(struct wait_queue *wq, uint64_t timeout_ms);

#endif
