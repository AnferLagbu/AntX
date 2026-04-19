#include "ipc.h"
#include "mm.h"
#include "serial.h"
#include "string.h"
#include "scheduler_ex.h"
#include "thread.h"

static struct ipc_namespace ipc_ns;
static ipc_id_t next_ipc_id = 1;

void ipc_init(void) {
    memset(&ipc_ns, 0, sizeof(struct ipc_namespace));
    next_ipc_id = 1;
    
    for (int i = 0; i < IPC_MAX_PIPES; i++) {
        ipc_ns.pipes[i].id = 0;
        wait_queue_init(&ipc_ns.pipes[i].read_wait);
        wait_queue_init(&ipc_ns.pipes[i].write_wait);
    }
    
    for (int i = 0; i < IPC_MAX_MSG_QUEUES; i++) {
        ipc_ns.msg_queues[i].id = 0;
        wait_queue_init(&ipc_ns.msg_queues[i].send_wait);
        wait_queue_init(&ipc_ns.msg_queues[i].recv_wait);
    }
    
    for (int i = 0; i < IPC_MAX_SEMAPHORES; i++) {
        ipc_ns.semaphores[i].id = 0;
        wait_queue_init(&ipc_ns.semaphores[i].wait);
    }
    
    serial_puts(SERIAL_COM1, "[IPC] IPC subsystem initialized\n");
}

static struct pipe *pipe_find_free(void) {
    for (int i = 0; i < IPC_MAX_PIPES; i++) {
        if (ipc_ns.pipes[i].id == 0) {
            return &ipc_ns.pipes[i];
        }
    }
    return NULL;
}

static struct pipe *pipe_find_by_fd(int fd) {
    for (int i = 0; i < IPC_MAX_PIPES; i++) {
        if (ipc_ns.pipes[i].id != 0) {
            if (ipc_ns.pipes[i].read_fd == fd || ipc_ns.pipes[i].write_fd == fd) {
                return &ipc_ns.pipes[i];
            }
        }
    }
    return NULL;
}

int pipe_create(int pipefd[2]) {
    struct pipe *pipe = pipe_find_free();
    if (pipe == NULL) {
        serial_puts(SERIAL_COM1, "[IPC] No free pipe slots\n");
        return -1;
    }
    
    pipe->id = next_ipc_id++;
    memset(pipe->buffer, 0, PIPE_BUFFER_SIZE);
    pipe->read_pos = 0;
    pipe->write_pos = 0;
    pipe->count = 0;
    
    pid_t current_pid = process_get_current_pid();
    pipe->read_pid = current_pid;
    pipe->write_pid = current_pid;
    
    pipe->read_fd = (int)pipe->id * 2;
    pipe->write_fd = (int)pipe->id * 2 + 1;
    
    pipe->readers = 1;
    pipe->writers = 1;
    pipe->flags = 0;
    
    wait_queue_init(&pipe->read_wait);
    wait_queue_init(&pipe->write_wait);
    
    pipefd[0] = pipe->read_fd;
    pipefd[1] = pipe->write_fd;
    
    serial_puts(SERIAL_COM1, "[IPC] Created pipe: read_fd=");
    serial_put_dec(SERIAL_COM1, pipefd[0]);
    serial_puts(SERIAL_COM1, " write_fd=");
    serial_put_dec(SERIAL_COM1, pipefd[1]);
    serial_puts(SERIAL_COM1, "\n");
    
    return 0;
}

int pipe_read(int fd, void *buf, uint32_t count) {
    struct pipe *pipe = pipe_find_by_fd(fd);
    if (pipe == NULL || pipe->id == 0) {
        return -1;
    }
    
    if (fd != pipe->read_fd) {
        return -1;
    }
    
    char *buffer = (char *)buf;
    uint32_t read_count = 0;
    
    while (read_count < count) {
        if (pipe->count == 0) {
            if (pipe->writers == 0) {
                break;
            }
            if (read_count > 0) {
                break;
            }
            struct thread *current = thread_get_current();
            if (current != NULL) {
                wait_queue_add(&pipe->read_wait, current);
                scheduler_yield_ex();
            }
            continue;
        }
        
        buffer[read_count++] = pipe->buffer[pipe->read_pos];
        pipe->read_pos = (pipe->read_pos + 1) % PIPE_BUFFER_SIZE;
        pipe->count--;
        
        if (pipe->write_wait.count > 0) {
            wait_queue_wake_one(&pipe->write_wait);
        }
    }
    
    return (int)read_count;
}

int pipe_write(int fd, const void *buf, uint32_t count) {
    struct pipe *pipe = pipe_find_by_fd(fd);
    if (pipe == NULL || pipe->id == 0) {
        return -1;
    }
    
    if (fd != pipe->write_fd) {
        return -1;
    }
    
    if (pipe->readers == 0) {
        return -1;
    }
    
    const char *buffer = (const char *)buf;
    uint32_t written = 0;
    
    while (written < count) {
        if (pipe->count >= PIPE_BUFFER_SIZE) {
            struct thread *current = thread_get_current();
            if (current != NULL) {
                wait_queue_add(&pipe->write_wait, current);
                scheduler_yield_ex();
            }
            continue;
        }
        
        pipe->buffer[pipe->write_pos] = buffer[written++];
        pipe->write_pos = (pipe->write_pos + 1) % PIPE_BUFFER_SIZE;
        pipe->count++;
        
        if (pipe->read_wait.count > 0) {
            wait_queue_wake_one(&pipe->read_wait);
        }
    }
    
    return (int)written;
}

int pipe_close(int fd) {
    struct pipe *pipe = pipe_find_by_fd(fd);
    if (pipe == NULL) {
        return -1;
    }
    
    if (fd == pipe->read_fd) {
        pipe->readers--;
        if (pipe->readers == 0) {
            wait_queue_wake_all(&pipe->write_wait);
        }
    } else if (fd == pipe->write_fd) {
        pipe->writers--;
        if (pipe->writers == 0) {
            wait_queue_wake_all(&pipe->read_wait);
        }
    }
    
    if (pipe->readers == 0 && pipe->writers == 0) {
        pipe->id = 0;
        pipe->read_fd = 0;
        pipe->write_fd = 0;
    }
    
    return 0;
}

static struct signal_pending *signal_get_pending(pid_t pid) {
    struct process *proc = process_get_by_pid(pid);
    if (proc == NULL) {
        return NULL;
    }
    return NULL;
}

int signal_send(pid_t pid, int sig) {
    if (sig < 1 || sig > IPC_MAX_SIGNALS) {
        return -1;
    }
    
    struct process *proc = process_get_by_pid(pid);
    if (proc == NULL) {
        return -1;
    }
    
    serial_puts(SERIAL_COM1, "[IPC] Signal ");
    serial_put_dec(SERIAL_COM1, sig);
    serial_puts(SERIAL_COM1, " sent to PID ");
    serial_put_dec(SERIAL_COM1, pid);
    serial_puts(SERIAL_COM1, "\n");
    
    return 0;
}

int signal_register(int sig, void (*handler)(int), uint32_t flags) {
    if (sig < 1 || sig > IPC_MAX_SIGNALS) {
        return -1;
    }
    
    serial_puts(SERIAL_COM1, "[IPC] Registered signal handler for sig=");
    serial_put_dec(SERIAL_COM1, sig);
    serial_puts(SERIAL_COM1, "\n");
    
    return 0;
}

int signal_block(int sig) {
    if (sig < 1 || sig > IPC_MAX_SIGNALS) {
        return -1;
    }
    return 0;
}

int signal_unblock(int sig) {
    if (sig < 1 || sig > IPC_MAX_SIGNALS) {
        return -1;
    }
    return 0;
}

void signal_dispatch(void) {
}

static struct shm_segment *shm_find_free(void) {
    for (int i = 0; i < IPC_MAX_SHM_SEGS; i++) {
        if (ipc_ns.shm_segs[i].id == 0) {
            return &ipc_ns.shm_segs[i];
        }
    }
    return NULL;
}

static struct shm_segment *shm_find_by_id(ipc_id_t id) {
    for (int i = 0; i < IPC_MAX_SHM_SEGS; i++) {
        if (ipc_ns.shm_segs[i].id == id) {
            return &ipc_ns.shm_segs[i];
        }
    }
    return NULL;
}

ipc_id_t shm_create(uint64_t size, int perm) {
    if (size == 0 || size > SHM_MAX_SIZE) {
        return 0;
    }
    
    struct shm_segment *shm = shm_find_free();
    if (shm == NULL) {
        serial_puts(SERIAL_COM1, "[IPC] No free SHM slots\n");
        return 0;
    }
    
    uint64_t pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;
    uint64_t phys = (uint64_t)pmm_alloc_pages(pages);
    if (phys == 0) {
        serial_puts(SERIAL_COM1, "[IPC] Failed to allocate SHM memory\n");
        return 0;
    }
    
    shm->id = next_ipc_id++;
    shm->phys_addr = phys;
    shm->size = size;
    shm->creator = process_get_current_pid();
    shm->ref_count = 0;
    shm->attach_count = 0;
    shm->flags = 0;
    shm->perm = perm;
    
    memset(shm->attached_pids, 0, sizeof(shm->attached_pids));
    
    serial_puts(SERIAL_COM1, "[IPC] Created SHM segment id=");
    serial_put_dec(SERIAL_COM1, shm->id);
    serial_puts(SERIAL_COM1, " size=");
    serial_put_dec(SERIAL_COM1, (uint32_t)size);
    serial_puts(SERIAL_COM1, "\n");
    
    return shm->id;
}

int shm_attach(ipc_id_t id, void **addr) {
    struct shm_segment *shm = shm_find_by_id(id);
    if (shm == NULL) {
        return -1;
    }
    
    pid_t current_pid = process_get_current_pid();
    
    for (uint32_t i = 0; i < shm->attach_count; i++) {
        if (shm->attached_pids[i] == current_pid) {
            return 0;
        }
    }
    
    if (shm->attach_count >= 16) {
        return -1;
    }
    
    shm->attached_pids[shm->attach_count++] = current_pid;
    shm->ref_count++;
    
    if (addr != NULL) {
        *addr = (void *)shm->phys_addr;
    }
    
    serial_puts(SERIAL_COM1, "[IPC] Attached to SHM id=");
    serial_put_dec(SERIAL_COM1, id);
    serial_puts(SERIAL_COM1, "\n");
    
    return 0;
}

int shm_detach(ipc_id_t id) {
    struct shm_segment *shm = shm_find_by_id(id);
    if (shm == NULL) {
        return -1;
    }
    
    pid_t current_pid = process_get_current_pid();
    
    for (uint32_t i = 0; i < shm->attach_count; i++) {
        if (shm->attached_pids[i] == current_pid) {
            shm->attached_pids[i] = 0;
            shm->ref_count--;
            return 0;
        }
    }
    
    return -1;
}

int shm_destroy(ipc_id_t id) {
    struct shm_segment *shm = shm_find_by_id(id);
    if (shm == NULL) {
        return -1;
    }
    
    if (shm->ref_count > 0) {
        return -1;
    }
    
    uint64_t pages = (shm->size + PAGE_SIZE - 1) / PAGE_SIZE;
    for (uint64_t i = 0; i < pages; i++) {
        pmm_free_page((void *)(shm->phys_addr + i * PAGE_SIZE));
    }
    
    shm->id = 0;
    shm->phys_addr = 0;
    shm->size = 0;
    
    serial_puts(SERIAL_COM1, "[IPC] Destroyed SHM id=");
    serial_put_dec(SERIAL_COM1, id);
    serial_puts(SERIAL_COM1, "\n");
    
    return 0;
}

static struct msg_queue *msgq_find_free(void) {
    for (int i = 0; i < IPC_MAX_MSG_QUEUES; i++) {
        if (ipc_ns.msg_queues[i].id == 0) {
            return &ipc_ns.msg_queues[i];
        }
    }
    return NULL;
}

static struct msg_queue *msgq_find_by_id(ipc_id_t id) {
    for (int i = 0; i < IPC_MAX_MSG_QUEUES; i++) {
        if (ipc_ns.msg_queues[i].id == id) {
            return &ipc_ns.msg_queues[i];
        }
    }
    return NULL;
}

ipc_id_t msgq_create(int perm) {
    struct msg_queue *mq = msgq_find_free();
    if (mq == NULL) {
        serial_puts(SERIAL_COM1, "[IPC] No free message queue slots\n");
        return 0;
    }
    
    mq->id = next_ipc_id++;
    mq->owner = process_get_current_pid();
    mq->head = NULL;
    mq->tail = NULL;
    mq->count = 0;
    mq->max_msgs = MSG_QUEUE_MAX_MSGS;
    mq->max_size = MSG_MAX_SIZE;
    mq->flags = 0;
    mq->perm = perm;
    
    wait_queue_init(&mq->send_wait);
    wait_queue_init(&mq->recv_wait);
    
    serial_puts(SERIAL_COM1, "[IPC] Created message queue id=");
    serial_put_dec(SERIAL_COM1, mq->id);
    serial_puts(SERIAL_COM1, "\n");
    
    return mq->id;
}

int msgq_send(ipc_id_t id, uint64_t type, const void *data, uint64_t size) {
    struct msg_queue *mq = msgq_find_by_id(id);
    if (mq == NULL) {
        return -1;
    }
    
    if (size > MSG_MAX_SIZE) {
        return -1;
    }
    
    if (mq->count >= mq->max_msgs) {
        struct thread *current = thread_get_current();
        if (current != NULL) {
            wait_queue_add(&mq->send_wait, current);
            scheduler_yield_ex();
        }
    }
    
    struct message *msg = (struct message *)pmm_alloc_page();
    if (msg == NULL) {
        return -1;
    }
    
    msg->type = type;
    msg->sender = process_get_current_pid();
    msg->size = size;
    msg->next = NULL;
    
    if (data != NULL && size > 0) {
        memcpy(msg->data, data, size);
    }
    
    if (mq->tail == NULL) {
        mq->head = msg;
        mq->tail = msg;
    } else {
        mq->tail->next = msg;
        mq->tail = msg;
    }
    mq->count++;
    
    if (mq->recv_wait.count > 0) {
        wait_queue_wake_one(&mq->recv_wait);
    }
    
    return 0;
}

int msgq_recv(ipc_id_t id, uint64_t *type, void *data, uint64_t *size) {
    struct msg_queue *mq = msgq_find_by_id(id);
    if (mq == NULL) {
        return -1;
    }
    
    while (mq->head == NULL) {
        struct thread *current = thread_get_current();
        if (current != NULL) {
            wait_queue_add(&mq->recv_wait, current);
            scheduler_yield_ex();
        }
    }
    
    struct message *msg = mq->head;
    mq->head = msg->next;
    if (mq->head == NULL) {
        mq->tail = NULL;
    }
    mq->count--;
    
    if (type != NULL) {
        *type = msg->type;
    }
    
    if (data != NULL && msg->size > 0) {
        memcpy(data, msg->data, msg->size);
    }
    
    if (size != NULL) {
        *size = msg->size;
    }
    
    pmm_free_page(msg);
    
    if (mq->send_wait.count > 0) {
        wait_queue_wake_one(&mq->send_wait);
    }
    
    return 0;
}

int msgq_destroy(ipc_id_t id) {
    struct msg_queue *mq = msgq_find_by_id(id);
    if (mq == NULL) {
        return -1;
    }
    
    while (mq->head != NULL) {
        struct message *msg = mq->head;
        mq->head = msg->next;
        pmm_free_page(msg);
    }
    
    mq->id = 0;
    
    serial_puts(SERIAL_COM1, "[IPC] Destroyed message queue id=");
    serial_put_dec(SERIAL_COM1, id);
    serial_puts(SERIAL_COM1, "\n");
    
    return 0;
}

static struct semaphore *sem_find_free(void) {
    for (int i = 0; i < IPC_MAX_SEMAPHORES; i++) {
        if (ipc_ns.semaphores[i].id == 0) {
            return &ipc_ns.semaphores[i];
        }
    }
    return NULL;
}

static struct semaphore *sem_find_by_id(ipc_id_t id) {
    for (int i = 0; i < IPC_MAX_SEMAPHORES; i++) {
        if (ipc_ns.semaphores[i].id == id) {
            return &ipc_ns.semaphores[i];
        }
    }
    return NULL;
}

ipc_id_t sem_create(uint32_t count, uint32_t max_count) {
    struct semaphore *sem = sem_find_free();
    if (sem == NULL) {
        serial_puts(SERIAL_COM1, "[IPC] No free semaphore slots\n");
        return 0;
    }
    
    sem->id = next_ipc_id++;
    sem->owner = process_get_current_pid();
    sem->count = (int32_t)count;
    sem->max_count = max_count;
    sem->flags = 0;
    sem->perm = 0666;
    
    wait_queue_init(&sem->wait);
    
    serial_puts(SERIAL_COM1, "[IPC] Created semaphore id=");
    serial_put_dec(SERIAL_COM1, sem->id);
    serial_puts(SERIAL_COM1, " count=");
    serial_put_dec(SERIAL_COM1, count);
    serial_puts(SERIAL_COM1, "\n");
    
    return sem->id;
}

int sem_wait(ipc_id_t id) {
    struct semaphore *sem = sem_find_by_id(id);
    if (sem == NULL) {
        return -1;
    }
    
    while (sem->count <= 0) {
        struct thread *current = thread_get_current();
        if (current != NULL) {
            wait_queue_add(&sem->wait, current);
            scheduler_yield_ex();
        }
    }
    
    sem->count--;
    return 0;
}

int sem_post(ipc_id_t id) {
    struct semaphore *sem = sem_find_by_id(id);
    if (sem == NULL) {
        return -1;
    }
    
    if ((uint32_t)sem->count < sem->max_count) {
        sem->count++;
    }
    
    if (sem->wait.count > 0) {
        wait_queue_wake_one(&sem->wait);
    }
    
    return 0;
}

int sem_destroy(ipc_id_t id) {
    struct semaphore *sem = sem_find_by_id(id);
    if (sem == NULL) {
        return -1;
    }
    
    wait_queue_wake_all(&sem->wait);
    sem->id = 0;
    
    serial_puts(SERIAL_COM1, "[IPC] Destroyed semaphore id=");
    serial_put_dec(SERIAL_COM1, id);
    serial_puts(SERIAL_COM1, "\n");
    
    return 0;
}
