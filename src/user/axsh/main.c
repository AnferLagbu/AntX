#include "user/user.h"
#include "builtins.h"

static void print_banner(void) {
    user_print("\n");
    user_println("  ___  _  _ ___");
    user_println(" / _ \\| || | __|");
    user_println("| (_) | || |__ \\");
    user_println(" \\___/|_||_|___/");
    user_println("");
    user_println("axsh - AntX Shell");
    user_println("Type 'help' for commands");
    user_println("");
}

static void print_prompt(void) {
    char cwd[64];
    user_getcwd(cwd, sizeof(cwd));
    
    uint64_t pwid = sys_proc_get_pwid();
    if (pwid != 0) {
        user_print("[");
        user_print_hex(pwid);
        user_print("]");
    }
    
    user_print(cwd);
    user_print("> ");
}

void shell_main(void) {
    char line[MAX_LINE];
    int argc;
    char **argv;
    
    print_banner();
    
    while (shell_is_running()) {
        print_prompt();
        
        int len = user_read_line(line, sizeof(line));
        if (len == 0) continue;
        
        argv = user_parse_args(line, &argc);
        if (argc == 0) continue;
        
        execute_builtin(argc, argv);
    }
}

__attribute__((naked)) void _start(void) {
    __asm__ volatile(
        "mov $0x23, %%ax\n"
        "mov %%ax, %%ds\n"
        "mov %%ax, %%es\n"
        "mov %%ax, %%fs\n"
        "mov %%ax, %%gs\n"
        "xor %%rbp, %%rbp\n"
        "call shell_main\n"
        "mov $2, %%rax\n"
        "xor %%rdi, %%rdi\n"
        "int $0x80\n"
        "1: hlt\n"
        "jmp 1b\n"
        : : : "ax", "memory"
    );
}
