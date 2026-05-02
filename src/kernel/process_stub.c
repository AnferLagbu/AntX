#include "proc.h"
#include "serial.h"
#include "pwid.h"

#ifndef pid_t
typedef uint64_t pid_t;
#endif

#ifndef tid_t
typedef uint64_t tid_t;
#endif

static struct process init_process = {
    .pid = 1,
    .session_id = 1,
    .parent_pid = 0,
    .pwid = 0x0020F45A8B978417,
    .state = PROC_RUNNING,
    .exit_code = 0,
    .priority = 2,
    .cpu_time = 0,
    .start_time = 0,
    .time_slice = 10,
    .cr3 = 0,
    .kernel_stack = 0,
    .user_stack = 0,
};

static int init_created = 0;
static uint64_t current_pwid = 0x0020F45A8B978417;

struct process* process_get_current(void) {
    if (!init_created) {
        init_created = 1;
        serial_puts(SERIAL_COM1, "[PROC] C-layer init process created\n");
    }
    return &init_process;
}

pid_t process_get_current_pid(void) {
    return (pid_t)init_process.pid;
}

int signal_send(pid_t pid, int sig) {
    if (pid == 0 || sig <= 0) {
        serial_puts(SERIAL_COM1, "[SIGNAL] Invalid parameters\n");
        return -1;
    }

    serial_puts(SERIAL_COM1, "[SIGNAL] Signal ");
    serial_put_dec(SERIAL_COM1, sig);
    serial_puts(SERIAL_COM1, " sent to PID=");
    serial_put_dec(SERIAL_COM1, (uint32_t)pid);

    if (sig >= 1 && sig <= 31) {
        serial_puts(SERIAL_COM1, " [VALID]\n");
        return 0;
    } else {
        serial_puts(SERIAL_COM1, " [INVALID SIGNAL NUMBER]\n");
        return -1;
    }
}

uint64_t pwid_get_current(void) {
    if (!init_created) {
        init_created = 1;
        serial_puts(SERIAL_COM1, "[PWID] C-layer PWID context initialized\n");
    }
    return current_pwid;
}
