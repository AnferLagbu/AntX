#include "builtins.h"
#include "user/user.h"

static int running = 1;

int cmd_help(int argc, char **argv) {
    (void)argc;
    (void)argv;
    user_println("Available commands:");
    user_println("  help         - Show this help message");
    user_println("  clear        - Clear screen");
    user_println("  echo [text]  - Echo text");
    user_println("  exit         - Exit shell");
    user_println("  pwid_login   - Login (usage: pwid_login \"note\" \"password\")");
    user_println("  pwid_logout  - Logout current session");
    user_println("  pwid_whoami  - Show current PWID");
    user_println("  hostname     - Show/set hostname (Root only to set)");
    return 0;
}

int cmd_clear(int argc, char **argv) {
    (void)argc;
    (void)argv;
    user_print("\033[2J\033[H");
    return 0;
}

int cmd_echo(int argc, char **argv) {
    for (int i = 1; i < argc; i++) {
        if (i > 1) user_print(" ");
        user_print(argv[i]);
    }
    user_print("\n");
    return 0;
}

int cmd_exit(int argc, char **argv) {
    (void)argc;
    (void)argv;
    user_println("Goodbye!");
    running = 0;
    return 0;
}

int cmd_pwid_login(int argc, char **argv) {
    if (argc < 3) {
        user_println("Usage: pwid_login \"note\" \"password\"");
        return 1;
    }
    
    int result = user_auth_login(argv[2], argv[1]);
    if (result > 0) {
        user_print("Login successful! PWID: ");
        user_print_hex(sys_proc_get_pwid());
        user_print("\n");
        return 0;
    } else {
        user_println("Login failed: invalid credentials");
        return 1;
    }
}

int cmd_pwid_logout(int argc, char **argv) {
    (void)argc;
    (void)argv;
    user_auth_logout();
    user_println("Logged out.");
    return 0;
}

int cmd_pwid_whoami(int argc, char **argv) {
    (void)argc;
    (void)argv;
    uint64_t pwid = sys_proc_get_pwid();
    user_print("Current PWID: ");
    user_print_hex(pwid);
    user_print("\n");
    return 0;
}

int cmd_hostname(int argc, char **argv) {
    char hostname[64];
    
    if (argc == 1) {
        int result = sys_get_hostname(hostname, sizeof(hostname));
        if (result == 0) {
            user_println(hostname);
        } else {
            user_println("Error getting hostname");
        }
    } else {
        int len = user_strlen(argv[1]);
        int result = sys_set_hostname(argv[1], len);
        if (result == 0) {
            user_print("Hostname set to: ");
            user_println(argv[1]);
        } else if (result == -105) {
            user_println("Error: Root permission required to set hostname");
        } else {
            user_println("Error setting hostname");
        }
    }
    return 0;
}

struct builtin_cmd builtins[] = {
    { "help",        cmd_help },
    { "clear",       cmd_clear },
    { "echo",        cmd_echo },
    { "exit",        cmd_exit },
    { "pwid_login",  cmd_pwid_login },
    { "pwid_logout", cmd_pwid_logout },
    { "pwid_whoami", cmd_pwid_whoami },
    { "hostname",    cmd_hostname },
    { NULL,          NULL }
};

int shell_is_running(void) {
    return running;
}
