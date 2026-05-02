#include "proc.h"
#include "serial.h"

static struct process init_process = {
    .pid = 1,
    .session_id = 0,
    .parent_pid = 0,
    .pwid = 0,
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

struct process* process_get_current(void) {
    if (!init_created) {
        init_created = 1;
        serial_puts(SERIAL_COM1, "[PROC] C-layer init process created\n");
    }
    return &init_process;
}
