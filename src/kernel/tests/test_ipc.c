#include "kernel_test.h"
#include "ipc.h"
#include "syscall.h"
#include "thread.h"
#include "string.h"

extern uint32_t process_get_current_pid(void);

static int test_ipc_pipe_create(void) {
    int pipefd[2];
    int result = pipe_create(pipefd);
    TEST_ASSERT_EQ(result, 0);
    TEST_ASSERT_GE(pipefd[0], 0);
    TEST_ASSERT_GE(pipefd[1], 0);
    TEST_ASSERT_NE(pipefd[0], pipefd[1]);
    
    pipe_close(pipefd[0]);
    pipe_close(pipefd[1]);
    
    return TEST_PASS;
}

static int test_ipc_pipe_write_read(void) {
    int pipefd[2];
    int result = pipe_create(pipefd);
    TEST_ASSERT_EQ(result, 0);
    
    const char *msg = "Pipe test message";
    int len = strlen(msg);
    
    int written = pipe_write(pipefd[1], msg, len);
    TEST_ASSERT_EQ(written, len);
    
    char buffer[64] = {0};
    int read_bytes = pipe_read(pipefd[0], buffer, sizeof(buffer));
    TEST_ASSERT_EQ(read_bytes, len);
    
    pipe_close(pipefd[0]);
    pipe_close(pipefd[1]);
    
    return TEST_PASS;
}

static int test_ipc_signal_send(void) {
    pid_t current_pid = process_get_current_pid();
    if (current_pid == 0) {
        return TEST_SKIP;
    }
    
    int result = signal_send(current_pid, 1);
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_ipc_semaphore(void) {
    ipc_id_t sem_id = sem_create(1, 10);
    TEST_ASSERT_GT(sem_id, 0);
    
    int result = sem_wait(sem_id);
    TEST_ASSERT_EQ(result, 0);
    
    result = sem_post(sem_id);
    TEST_ASSERT_EQ(result, 0);
    
    result = sem_destroy(sem_id);
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_ipc_shared_memory(void) {
    ipc_id_t shm_id = shm_create(4096, 0);
    TEST_ASSERT_GT(shm_id, 0);
    
    void *addr = NULL;
    int result = shm_attach(shm_id, &addr);
    TEST_ASSERT_EQ(result, 0);
    TEST_ASSERT_NOT_NULL(addr);
    
    result = shm_detach(shm_id);
    TEST_ASSERT_EQ(result, 0);
    
    result = shm_destroy(shm_id);
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

static int test_ipc_message_queue(void) {
    ipc_id_t mq_id = msgq_create(0);
    TEST_ASSERT_GT(mq_id, 0);
    
    const char *msg = "Message queue test";
    int result = msgq_send(mq_id, 1, msg, strlen(msg));
    TEST_ASSERT_EQ(result, 0);
    
    char buffer[64] = {0};
    uint64_t type = 0;
    uint64_t size = sizeof(buffer);
    int read_bytes = msgq_recv(mq_id, &type, buffer, &size);
    TEST_ASSERT_GT(read_bytes, 0);
    
    result = msgq_destroy(mq_id);
    TEST_ASSERT_EQ(result, 0);
    
    return TEST_PASS;
}

void test_ipc_register(void) {
    int mod = test_register_module("IPC (Inter-Process Communication)");
    
    test_register_case(mod, "Create pipe", test_ipc_pipe_create);
    test_register_case(mod, "Pipe write/read", test_ipc_pipe_write_read);
    test_register_case(mod, "Send signal", test_ipc_signal_send);
    test_register_case(mod, "Semaphore operations", test_ipc_semaphore);
    test_register_case(mod, "Shared memory", test_ipc_shared_memory);
    test_register_case(mod, "Message queue", test_ipc_message_queue);
}
