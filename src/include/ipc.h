#ifndef _IPC_H
#define _IPC_H

#include "types.h"
#include "thread.h"

#define IPC_MAX_PIPES          64
#define IPC_MAX_SIGNALS        32
#define IPC_MAX_SHM_SEGS       16
#define IPC_MAX_MSG_QUEUES     32
#define IPC_MAX_SEMAPHORES     64

#define PIPE_BUFFER_SIZE       4096
#define SHM_MAX_SIZE           (16 * 1024 * 1024)
#define MSG_MAX_SIZE           4096
#define MSG_QUEUE_MAX_MSGS     64

typedef uint32_t ipc_id_t;

enum ipc_type {
    IPC_TYPE_PIPE = 1,
    IPC_TYPE_SIGNAL,
    IPC_TYPE_SHM,
    IPC_TYPE_MSGQ,
    IPC_TYPE_SEM
};

enum signal_num {
    SIG_NONE = 0,
    SIG_INT = 1,
    SIG_ILL = 2,
    SIG_FPE = 3,
    SIG_SEGV = 4,
    SIG_TERM = 5,
    SIG_KILL = 6,
    SIG_STOP = 7,
    SIG_CONT = 8,
    SIG_CHLD = 9,
    SIG_USR1 = 10,
    SIG_USR2 = 11,
    SIG_ALARM = 12,
    SIG_PIPE = 13
};

enum signal_action {
    SIG_ACT_DEFAULT = 0,
    SIG_ACT_IGNORE,
    SIG_ACT_HANDLER,
    SIG_ACT_BLOCK
};

struct pipe {
    ipc_id_t id;
    char buffer[PIPE_BUFFER_SIZE];
    uint32_t read_pos;
    uint32_t write_pos;
    uint32_t count;
    
    pid_t read_pid;
    pid_t write_pid;
    
    int read_fd;
    int write_fd;
    
    int readers;
    int writers;
    
    struct wait_queue read_wait;
    struct wait_queue write_wait;
    
    int flags;
};

struct signal_handler {
    void (*handler)(int);
    uint64_t handler_addr;
    uint64_t stack_addr;
    uint32_t flags;
    uint32_t mask;
};

struct signal_pending {
    uint32_t pending;
    uint32_t blocked;
    struct signal_handler handlers[IPC_MAX_SIGNALS];
};

struct shm_segment {
    ipc_id_t id;
    uint64_t phys_addr;
    uint64_t size;
    
    pid_t creator;
    uint32_t ref_count;
    
    pid_t attached_pids[16];
    uint32_t attach_count;
    
    int flags;
    int perm;
};

struct message {
    uint64_t type;
    uint64_t sender;
    uint64_t size;
    char data[MSG_MAX_SIZE];
    struct message *next;
};

struct msg_queue {
    ipc_id_t id;
    pid_t owner;
    
    struct message *head;
    struct message *tail;
    uint32_t count;
    uint32_t max_msgs;
    uint32_t max_size;
    
    struct wait_queue send_wait;
    struct wait_queue recv_wait;
    
    int flags;
    int perm;
};

struct semaphore {
    ipc_id_t id;
    pid_t owner;
    
    int32_t count;
    uint32_t max_count;
    
    struct wait_queue wait;
    
    int flags;
    int perm;
};

struct ipc_namespace {
    struct pipe pipes[IPC_MAX_PIPES];
    struct shm_segment shm_segs[IPC_MAX_SHM_SEGS];
    struct msg_queue msg_queues[IPC_MAX_MSG_QUEUES];
    struct semaphore semaphores[IPC_MAX_SEMAPHORES];
};

void ipc_init(void);

int pipe_create(int pipefd[2]);
int pipe_read(int fd, void *buf, uint32_t count);
int pipe_write(int fd, const void *buf, uint32_t count);
int pipe_close(int fd);

int signal_send(pid_t pid, int sig);
int signal_register(int sig, void (*handler)(int), uint32_t flags);
int signal_block(int sig);
int signal_unblock(int sig);
void signal_dispatch(void);

ipc_id_t shm_create(uint64_t size, int perm);
int shm_attach(ipc_id_t id, void **addr);
int shm_detach(ipc_id_t id);
int shm_destroy(ipc_id_t id);

ipc_id_t msgq_create(int perm);
int msgq_send(ipc_id_t id, uint64_t type, const void *data, uint64_t size);
int msgq_recv(ipc_id_t id, uint64_t *type, void *data, uint64_t *size);
int msgq_destroy(ipc_id_t id);

ipc_id_t sem_create(uint32_t count, uint32_t max_count);
int sem_wait(ipc_id_t id);
int sem_post(ipc_id_t id);
int sem_destroy(ipc_id_t id);

#endif
