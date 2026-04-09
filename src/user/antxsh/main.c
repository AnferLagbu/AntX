#include "user/user.h"
#include "builtins.h"

static void print_banner(void) {
    user_print("\n");
    user_println("========================================");
    user_println("antxsh - AntX Shell");
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

static int execute_builtin(int argc, char **argv) {
    if (argc == 0) return 0;
    
    for (int i = 0; builtins[i].name != NULL; i++) {
        if (user_strcmp(argv[0], builtins[i].name) == 0) {
            return builtins[i].func(argc, argv);
        }
    }
    
    user_print("Unknown command: ");
    user_println(argv[0]);
    return 1;
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

void _start(void) {
    shell_main();
    sys_proc_exit(0);
}
