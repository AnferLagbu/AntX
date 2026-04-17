#include "user/user.h"

extern int user_install_check_needed(void);
extern void user_install_run(void);

extern unsigned char build_user_antxsh_bin[];
extern unsigned int build_user_antxsh_bin_len;

static void print_banner(void) {
    user_print("\n");
    user_println("========================================");
    user_println("antxsh - AntX Shell (User Mode)");
    user_println("Type 'help' for available commands.");
    user_println("========================================");
    user_print("\n");
}

static void print_prompt(void) {
    uint64_t pwid = sys_proc_get_pwid();
    if (pwid != 0) {
        user_print("antxsh@");
        user_print_hex(pwid);
        user_print("> ");
    } else {
        user_print("antxsh> ");
    }
}

extern int shell_is_running(void);
extern int execute_builtin(int argc, char **argv);

void init_main(void) {
    user_print("\n[INIT] Starting user-space init process\n");
    
    if (user_install_check_needed()) {
        user_print("[INIT] First boot detected, starting installation wizard\n");
        user_install_run();
    }
    
    user_print("[INIT] System ready. Starting shell...\n\n");
    
    print_banner();
    
    char line[256];
    int argc;
    char **argv;
    
    while (shell_is_running()) {
        print_prompt();
        
        int len = user_read_line(line, sizeof(line));
        if (len == 0) continue;
        
        argv = user_parse_args(line, &argc);
        if (argc == 0) continue;
        
        execute_builtin(argc, argv);
    }
    
    user_println("\n[INIT] Shell exited. System shutting down.");
}

void _start(void) {
    __asm__ volatile(
        "mov $0x23, %%ax\n"
        "mov %%ax, %%ds\n"
        "mov %%ax, %%es\n"
        "mov %%ax, %%fs\n"
        "mov %%ax, %%gs\n"
        ".byte 0xEB, 0xFE\n"  
        : : : "ax", "memory"
    );
    
    init_main();
    
    sys_proc_exit(0);
    
    while (1) {
        sys_proc_yield_cpu();
    }
}
