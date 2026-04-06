#include "user/user.h"

extern void user_install_run(void);
extern int user_install_check_needed(void);

void init_main(void) {
    user_print("\n[INIT] Starting user-space init process\n");
    
    if (user_install_check_needed()) {
        user_print("[INIT] First boot detected, starting installation wizard\n");
        user_install_run();
    }
    
    user_print("[INIT] Starting shell...\n");
    
    int pid = sys_proc_create();
    if (pid < 0) {
        user_print("[INIT] FATAL: Failed to create shell process\n");
        while (1) {
            __asm__ volatile ("hlt");
        }
    }
    
    user_print("[INIT] Shell process created with PID: ");
    user_print_dec(pid);
    user_print("\n");
    
    int status = 0;
    int result = sys_proc_wait(pid, &status);
    
    user_print("[INIT] Shell exited with status: ");
    user_print_dec(status);
    user_print("\n");
    
    while (1) {
        sys_proc_yield_cpu();
    }
}

void _start(void) {
    init_main();
    
    sys_proc_exit(0);
    
    while (1) {
        __asm__ volatile ("hlt");
    }
}
