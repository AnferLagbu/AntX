#include "user/user.h"

extern int user_install_check_needed(void);
extern void user_install_run(void);

void init_main(void) {
    user_print("\n[INIT] Starting user-space init process\n");
    
    if (user_install_check_needed()) {
        user_print("[INIT] First boot detected, starting installation wizard\n");
        user_install_run();
    }
    
    user_print("[INIT] System ready. Running in single-user mode.\n");
    user_print("[INIT] Use 'help' for available commands.\n");
    user_print("[INIT] (Shell not yet implemented - kernel shell available)\n");
    
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
