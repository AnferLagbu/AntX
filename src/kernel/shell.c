#include "shell.h"
#include "keyboard.h"
#include "serial.h"
#include "syscall.h"
#include "pwid.h"
#include "hvfs.h"

#define MAX_LINE 256
#define MAX_ARGS 16

static int running = 1;
static char line[MAX_LINE];
static char *argv[MAX_ARGS];
static char hostname[64] = "localhost";

static void print(const char *s) {
    serial_puts(SERIAL_COM1, s);
}

static void println(const char *s) {
    serial_puts(SERIAL_COM1, s);
    serial_puts(SERIAL_COM1, "\n");
}

static void print_hex(uint64_t val) {
    serial_puts(SERIAL_COM1, "0x");
    serial_put_hex(SERIAL_COM1, val);
}

static void print_banner(void) {
    println("");
    println("========================================");
    println("antxsh v0.1.0 - AntX Shell");
    println("Type 'help' for available commands.");
    println("========================================");
    println("");
}

static void print_prompt(void) {
    uint64_t pwid = sys_proc_getpwid();
    if (pwid != 0) {
        struct pwid_entry *entry = pwid_find(pwid);
        if (entry) {
            print("[");
            print(entry->note);
            print("@");
            print(hostname);
            print("] ");
            if (entry->level == PWID_LEVEL_ROOT) {
                print("# ");
            } else if (entry->level == PWID_LEVEL_TRUSTWORTHY) {
                print("$ ");
            } else {
                print("% ");
            }
        } else {
            print("antxsh> ");
        }
    } else {
        print("[>] ");
    }
}

static int strcmp_local(const char *s1, const char *s2) {
    while (*s1 && *s2 && *s1 == *s2) {
        s1++;
        s2++;
    }
    return *s1 - *s2;
}

static int strlen_local(const char *s) {
    int len = 0;
    while (s[len]) len++;
    return len;
}

static int parse_args(char *line, int *argc) {
    *argc = 0;
    int in_arg = 0;
    int in_quote = 0;
    char *p = line;
    
    while (*p && *argc < MAX_ARGS - 1) {
        if (*p == '"') {
            in_quote = !in_quote;
            *p = '\0';
            p++;
        } else if (*p == ' ' && !in_quote) {
            if (in_arg) {
                *p = '\0';
                in_arg = 0;
            }
            p++;
        } else {
            if (!in_arg) {
                argv[*argc] = p;
                (*argc)++;
                in_arg = 1;
            }
            p++;
        }
    }
    
    argv[*argc] = NULL;
    return *argc;
}

static int prompt_set_password(void) {
    char new_pw[64];
    char confirm_pw[64];
    
    println("");
    println("=== FIRST TIME SETUP ===");
    println("This is your first login.");
    println("You MUST set a password for the root account.");
    println("=========================");
    println("");
    
    print("New password: ");
    int new_len = keyboard_read_line(new_pw, sizeof(new_pw));
    if (new_len < 4) {
        println("Password too short (minimum 4 characters).");
        return -1;
    }
    
    print("Confirm new password: ");
    int confirm_len = keyboard_read_line(confirm_pw, sizeof(confirm_pw));
    
    if (new_len != confirm_len || strcmp_local(new_pw, confirm_pw) != 0) {
        println("Passwords do not match.");
        return -1;
    }
    
    int result = sys_auth_changepw("", new_pw);
    if (result == 0) {
        println("Password set successfully!");
        return 0;
    } else {
        println("Failed to set password.");
        return -1;
    }
}

static int prompt_password_change(void) {
    char old_pw[64];
    char new_pw[64];
    char confirm_pw[64];
    
    println("");
    println("=== CHANGE PASSWORD ===");
    println("");
    
    print("Current password: ");
    int old_len = keyboard_read_line(old_pw, sizeof(old_pw));
    if (old_len == 0) {
        println("Password change cancelled.");
        return -1;
    }
    
    print("New password: ");
    int new_len = keyboard_read_line(new_pw, sizeof(new_pw));
    if (new_len < 4) {
        println("Password too short (minimum 4 characters).");
        return -1;
    }
    
    print("Confirm new password: ");
    int confirm_len = keyboard_read_line(confirm_pw, sizeof(confirm_pw));
    
    if (new_len != confirm_len || strcmp_local(new_pw, confirm_pw) != 0) {
        println("Passwords do not match.");
        return -1;
    }
    
    int result = sys_auth_changepw(old_pw, new_pw);
    if (result == 0) {
        println("Password changed successfully!");
        return 0;
    } else if (result == E_AUTH_PWERR) {
        println("Incorrect current password.");
        return -1;
    } else {
        println("Failed to change password.");
        return -1;
    }
}

static int cmd_help(int argc, char **argv) {
    (void)argc;
    (void)argv;
    println("Available commands:");
    println("  help           - Show this help message");
    println("  clear          - Clear screen");
    println("  echo [text]    - Echo text");
    println("  exit           - Exit shell");
    println("");
    println("Authentication:");
    println("  auth_login     - Login (usage: auth_login \"note\" \"password\")");
    println("  auth_logout    - Logout current session");
    println("  auth_whoami    - Show current PWID");
    println("  auth_passwd    - Change password");
    println("");
    println("Filesystem:");
    println("  ls [path]      - List directory contents");
    println("  cd <dir>       - Change directory");
    println("  pwd            - Print working directory");
    println("  cat <file>     - Display file contents");
    println("  touch <file>   - Create empty file");
    println("  mkdir <dir>    - Create directory");
    println("  rm <file>      - Remove file");
    println("  write <file> <text> - Write text to file");
    println("  sync           - Sync filesystem to disk");
    println("");
    println("System:");
    println("  hostname       - Show/set hostname (Root only to set)");
    return 0;
}

static int cmd_clear(int argc, char **argv) {
    (void)argc;
    (void)argv;
    print("\033[2J\033[H");
    return 0;
}

static int cmd_echo(int argc, char **argv) {
    for (int i = 1; i < argc; i++) {
        if (i > 1) print(" ");
        print(argv[i]);
    }
    print("\n");
    return 0;
}

static int cmd_exit(int argc, char **argv) {
    (void)argc;
    (void)argv;
    println("Goodbye!");
    running = 0;
    return 0;
}

static int cmd_auth_login(int argc, char **argv) {
    if (argc < 3) {
        println("Usage: auth_login \"note\" \"password\"");
        return 1;
    }
    
    int result = sys_auth_login(argv[2], argv[1]);
    if (result > 0) {
        print("Login successful! PWID: ");
        print_hex(sys_proc_getpwid());
        print("\n");
        
        uint64_t pwid = sys_proc_getpwid();
        if (pwid_has_default_password(pwid)) {
            while (prompt_set_password() != 0) {
                println("Please try again.");
            }
        }
        
        return 0;
    } else if (result == E_AUTH_PWERR) {
        println("Login failed: incorrect password");
        return 1;
    } else if (result == E_AUTH_NOTFOUND) {
        println("Login failed: user not found");
        return 1;
    } else {
        println("Login failed: unknown error");
        return 1;
    }
}

static int cmd_auth_logout(int argc, char **argv) {
    (void)argc;
    (void)argv;
    sys_auth_logout();
    println("Logged out.");
    return 0;
}

static int cmd_auth_whoami(int argc, char **argv) {
    (void)argc;
    (void)argv;
    uint64_t pwid = sys_proc_getpwid();
    if (pwid == 0) {
        println("Not logged in");
        return 0;
    }
    
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry) {
        print("PWID: ");
        print_hex(pwid);
        print(" Note: ");
        print(entry->note);
        print(" Level: ");
        serial_put_dec(SERIAL_COM1, entry->level);
        if (entry->flags & PWID_FLAG_DEFAULT_PW) {
            print(" [NEEDS_PASSWORD]");
        }
        print("\n");
    } else {
        print("PWID: ");
        print_hex(pwid);
        print("\n");
    }
    return 0;
}

static int cmd_auth_passwd(int argc, char **argv) {
    (void)argc;
    (void)argv;
    uint64_t pwid = sys_proc_getpwid();
    if (pwid == 0) {
        println("Not logged in");
        return 1;
    }
    
    while (prompt_password_change() != 0) {
        println("Please try again or press Ctrl+C to cancel.");
    }
    
    return 0;
}

static int cmd_hostname(int argc, char **argv) {
    if (argc == 1) {
        println(hostname);
    } else {
        if (!pwid_is_root(sys_proc_getpwid())) {
            println("Error: Root permission required to set hostname");
            return 1;
        }
        
        int len = strlen_local(argv[1]);
        int result = sys_sethostname(argv[1], len);
        if (result == 0) {
            for (int i = 0; i < len && i < 63; i++) {
                hostname[i] = argv[1][i];
            }
            hostname[len] = '\0';
            print("Hostname set to: ");
            println(hostname);
        } else if (result == E_INVAL) {
            println("Error: Invalid hostname");
            return 1;
        } else {
            println("Error setting hostname");
            return 1;
        }
    }
    return 0;
}

static int cmd_ls(int argc, char **argv) {
    const char *path = "/";
    if (argc > 1) {
        path = argv[1];
    }
    
    int fd = sys_fs_open(path, HVFS_O_RDONLY, 0);
    if (fd < 0) {
        print("ls: cannot access '");
        print(path);
        println("': No such file or directory");
        return 1;
    }
    
    struct dir_entry entry;
    int count = 0;
    
    while (sys_fs_readdir(fd, &entry) > 0) {
        if (entry.inode != 0) {
            if (entry.file_type == HVFS_TYPE_DIR) {
                print("[DIR]  ");
            } else {
                print("[FILE] ");
            }
            println(entry.name);
            count++;
        }
    }
    
    sys_fs_close(fd);
    
    if (count == 0) {
        println("(empty directory)");
    }
    
    return 0;
}

static int cmd_cd(int argc, char **argv) {
    if (argc < 2) {
        println("cd: missing operand");
        return 1;
    }
    
    int result = sys_env_chdir(argv[1]);
    if (result < 0) {
        print("cd: cannot change to '");
        print(argv[1]);
        println("': No such directory");
        return 1;
    }
    
    return 0;
}

static int cmd_cat(int argc, char **argv) {
    if (argc < 2) {
        println("cat: missing file operand");
        return 1;
    }
    
    int fd = sys_fs_open(argv[1], HVFS_O_RDONLY, 0);
    if (fd < 0) {
        print("cat: cannot open '");
        print(argv[1]);
        println("': No such file");
        return 1;
    }
    
    char buf[512];
    int n;
    
    while ((n = sys_fs_read(fd, buf, sizeof(buf) - 1)) > 0) {
        buf[n] = '\0';
        print(buf);
    }
    
    sys_fs_close(fd);
    return 0;
}

static int cmd_touch(int argc, char **argv) {
    if (argc < 2) {
        println("touch: missing file operand");
        return 1;
    }
    
    int fd = sys_fs_open(argv[1], HVFS_O_CREAT | HVFS_O_WRONLY, 0);
    if (fd < 0) {
        print("touch: cannot create '");
        print(argv[1]);
        println("': Permission denied");
        return 1;
    }
    
    sys_fs_close(fd);
    print("Created: ");
    println(argv[1]);
    return 0;
}

static int cmd_mkdir(int argc, char **argv) {
    if (argc < 2) {
        println("mkdir: missing operand");
        return 1;
    }
    
    int result = sys_fs_mkdir(argv[1], 0755);
    if (result < 0) {
        print("mkdir: cannot create directory '");
        print(argv[1]);
        println("'");
        return 1;
    }
    
    print("Created directory: ");
    println(argv[1]);
    return 0;
}

static int cmd_rm(int argc, char **argv) {
    if (argc < 2) {
        println("rm: missing operand");
        return 1;
    }
    
    int result = sys_fs_unlink(argv[1]);
    if (result < 0) {
        print("rm: cannot remove '");
        print(argv[1]);
        println("': No such file or permission denied");
        return 1;
    }
    
    print("Removed: ");
    println(argv[1]);
    return 0;
}

static int cmd_sync(int argc, char **argv) {
    (void)argc;
    (void)argv;
    
    int result = sys_fs_sync();
    if (result == 0) {
        println("Filesystem synced to disk.");
    } else {
        println("Sync failed or not in disk mode.");
    }
    return 0;
}

static int cmd_pwd(int argc, char **argv) {
    (void)argc;
    (void)argv;
    
    char cwd[128];
    int result = sys_env_getcwd(cwd, sizeof(cwd));
    if (result >= 0) {
        println(cwd);
    } else {
        println("/");
    }
    return 0;
}

static int cmd_write(int argc, char **argv) {
    if (argc < 3) {
        println("write: usage: write <file> <text>");
        return 1;
    }
    
    int fd = sys_fs_open(argv[1], HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, 0);
    if (fd < 0) {
        print("write: cannot open '");
        print(argv[1]);
        println("'");
        return 1;
    }
    
    int len = strlen_local(argv[2]);
    int n = sys_fs_write(fd, argv[2], len);
    sys_fs_close(fd);
    
    if (n == len) {
        print("Wrote ");
        serial_put_dec(SERIAL_COM1, n);
        println(" bytes.");
    } else {
        println("Write error.");
    }
    
    return 0;
}

struct builtin {
    const char *name;
    int (*func)(int argc, char **argv);
};

static struct builtin builtins[] = {
    { "help",        cmd_help },
    { "clear",       cmd_clear },
    { "echo",        cmd_echo },
    { "exit",        cmd_exit },
    { "auth_login",  cmd_auth_login },
    { "auth_logout", cmd_auth_logout },
    { "auth_whoami", cmd_auth_whoami },
    { "auth_passwd", cmd_auth_passwd },
    { "hostname",    cmd_hostname },
    { "ls",          cmd_ls },
    { "cd",          cmd_cd },
    { "cat",         cmd_cat },
    { "touch",       cmd_touch },
    { "mkdir",       cmd_mkdir },
    { "rm",          cmd_rm },
    { "sync",        cmd_sync },
    { "pwd",         cmd_pwd },
    { "write",       cmd_write },
    { NULL,          NULL }
};

static int execute_command(int argc, char **argv) {
    if (argc == 0) return 0;
    
    for (int i = 0; builtins[i].name != NULL; i++) {
        if (strcmp_local(argv[0], builtins[i].name) == 0) {
            return builtins[i].func(argc, argv);
        }
    }
    
    print("Unknown command: ");
    println(argv[0]);
    return 1;
}

void shell_run(void) {
    int argc;
    
    print_banner();
    
    while (running) {
        print_prompt();
        
        int len = keyboard_read_line(line, sizeof(line));
        if (len == 0) continue;
        
        parse_args(line, &argc);
        if (argc == 0) continue;
        
        execute_command(argc, argv);
    }
}
