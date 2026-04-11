#include "builtins.h"
#include "user/user.h"

static int running = 1;

int cmd_help(int argc, char **argv) {
    (void)argc;
    (void)argv;
    user_println("Available commands:");
    user_println("  help           - Show this help message");
    user_println("  clear          - Clear screen");
    user_println("  echo [text]    - Echo text");
    user_println("  exit           - Exit shell");
    user_println("");
    user_println("Authentication:");
    user_println("  pwid_login     - Login (usage: pwid_login \"note\" \"password\")");
    user_println("  pwid_logout    - Logout current session");
    user_println("  pwid_whoami    - Show current PWID");
    user_println("  pwid_passwd    - Change password");
    user_println("");
    user_println("Filesystem:");
    user_println("  ls [path]      - List directory contents");
    user_println("  cd <dir>       - Change directory");
    user_println("  pwd            - Print working directory");
    user_println("  cat <file>     - Display file contents");
    user_println("  touch <file>   - Create empty file");
    user_println("  mkdir <dir>    - Create directory");
    user_println("  rm <file>      - Remove file");
    user_println("  write <file> <text> - Write text to file");
    user_println("  sync           - Sync filesystem to disk");
    user_println("");
    user_println("System:");
    user_println("  hostname       - Show/set hostname (Root only to set)");
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
    } else if (result == -104) {
        user_println("Login failed: incorrect password");
        return 1;
    } else if (result == -101) {
        user_println("Login failed: user not found");
        return 1;
    } else {
        user_println("Login failed: unknown error");
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
    if (pwid == 0) {
        user_println("Not logged in");
        return 0;
    }
    
    user_print("Current PWID: ");
    user_print_hex(pwid);
    user_print("\n");
    return 0;
}

static void prompt_password_change(void) {
    char old_pw[64];
    char new_pw[64];
    char confirm_pw[64];
    
    user_println("");
    user_println("=== CHANGE PASSWORD ===");
    user_println("");
    
    user_print("Current password: ");
    user_read_line(old_pw, sizeof(old_pw));
    
    user_print("New password: ");
    user_read_line(new_pw, sizeof(new_pw));
    
    user_print("Confirm new password: ");
    user_read_line(confirm_pw, sizeof(confirm_pw));
    
    if (user_strcmp(new_pw, confirm_pw) != 0) {
        user_println("Passwords do not match.");
        return;
    }
    
    int result = user_auth_change_password(old_pw, new_pw);
    if (result == 0) {
        user_println("Password changed successfully!");
    } else if (result == -104) {
        user_println("Incorrect current password.");
    } else {
        user_println("Failed to change password.");
    }
}

int cmd_pwid_passwd(int argc, char **argv) {
    (void)argc;
    (void)argv;
    uint64_t pwid = sys_proc_get_pwid();
    if (pwid == 0) {
        user_println("Not logged in");
        return 1;
    }
    
    prompt_password_change();
    return 0;
}

int cmd_hostname(int argc, char **argv) {
    char hostname[64];
    
    if (argc == 1) {
        int result = user_get_hostname(hostname, sizeof(hostname));
        if (result == 0) {
            user_println(hostname);
        } else {
            user_println("Error getting hostname");
        }
    } else {
        int len = user_strlen(argv[1]);
        int result = user_set_hostname(argv[1], len);
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

int cmd_ls(int argc, char **argv) {
    const char *path = "/";
    if (argc > 1) {
        path = argv[1];
    }
    
    int fd = user_open(path, HVFS_O_RDONLY, 0);
    if (fd < 0) {
        user_print("ls: cannot access '");
        user_print(path);
        user_println("': No such file or directory");
        return 1;
    }
    
    struct user_dirent entry;
    int count = 0;
    
    while (sys_fs_read_dir(fd, &entry) > 0) {
        if (entry.inode != 0) {
            if (entry.file_type == HVFS_TYPE_DIR) {
                user_print("[DIR]  ");
            } else {
                user_print("[FILE] ");
            }
            user_println(entry.name);
            count++;
        }
    }
    
    user_close(fd);
    
    if (count == 0) {
        user_println("(empty directory)");
    }
    
    return 0;
}

int cmd_cd(int argc, char **argv) {
    if (argc < 2) {
        user_println("cd: missing operand");
        return 1;
    }
    
    int result = user_chdir(argv[1]);
    if (result < 0) {
        user_print("cd: cannot change to '");
        user_print(argv[1]);
        user_println("': No such directory");
        return 1;
    }
    
    return 0;
}

int cmd_pwd(int argc, char **argv) {
    (void)argc;
    (void)argv;
    
    char cwd[128];
    int result = user_getcwd(cwd, sizeof(cwd));
    if (result >= 0) {
        user_println(cwd);
    } else {
        user_println("/");
    }
    return 0;
}

int cmd_cat(int argc, char **argv) {
    if (argc < 2) {
        user_println("cat: missing file operand");
        return 1;
    }
    
    int fd = user_open(argv[1], HVFS_O_RDONLY, 0);
    if (fd < 0) {
        user_print("cat: cannot open '");
        user_print(argv[1]);
        user_println("': No such file");
        return 1;
    }
    
    char buf[512];
    int n;
    
    while ((n = user_read(fd, buf, sizeof(buf) - 1)) > 0) {
        buf[n] = '\0';
        user_print(buf);
    }
    
    user_close(fd);
    return 0;
}

int cmd_touch(int argc, char **argv) {
    if (argc < 2) {
        user_println("touch: missing file operand");
        return 1;
    }
    
    int fd = user_open(argv[1], HVFS_O_CREAT | HVFS_O_WRONLY, 0);
    if (fd < 0) {
        user_print("touch: cannot create '");
        user_print(argv[1]);
        user_println("': Permission denied");
        return 1;
    }
    
    user_close(fd);
    user_print("Created: ");
    user_println(argv[1]);
    return 0;
}

int cmd_mkdir(int argc, char **argv) {
    if (argc < 2) {
        user_println("mkdir: missing operand");
        return 1;
    }
    
    int result = user_mkdir(argv[1], 0755);
    if (result < 0) {
        user_print("mkdir: cannot create directory '");
        user_print(argv[1]);
        user_println("'");
        return 1;
    }
    
    user_print("Created directory: ");
    user_println(argv[1]);
    return 0;
}

int cmd_rm(int argc, char **argv) {
    if (argc < 2) {
        user_println("rm: missing operand");
        return 1;
    }
    
    int result = user_unlink(argv[1]);
    if (result < 0) {
        user_print("rm: cannot remove '");
        user_print(argv[1]);
        user_println("': No such file or permission denied");
        return 1;
    }
    
    user_print("Removed: ");
    user_println(argv[1]);
    return 0;
}

int cmd_write(int argc, char **argv) {
    if (argc < 3) {
        user_println("write: usage: write <file> <text>");
        return 1;
    }
    
    int fd = user_open(argv[1], HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, 0);
    if (fd < 0) {
        user_print("write: cannot open '");
        user_print(argv[1]);
        user_println("'");
        return 1;
    }
    
    int len = user_strlen(argv[2]);
    int n = user_write(fd, argv[2], len);
    user_close(fd);
    
    if (n == len) {
        user_print("Wrote ");
        user_print_dec(n);
        user_println(" bytes.");
    } else {
        user_println("Write error.");
    }
    
    return 0;
}

int cmd_sync(int argc, char **argv) {
    (void)argc;
    (void)argv;
    
    user_sync();
    user_println("Filesystem synced to disk.");
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
    { "pwid_passwd", cmd_pwid_passwd },
    { "hostname",    cmd_hostname },
    { "ls",          cmd_ls },
    { "cd",          cmd_cd },
    { "pwd",         cmd_pwd },
    { "cat",         cmd_cat },
    { "touch",       cmd_touch },
    { "mkdir",       cmd_mkdir },
    { "rm",          cmd_rm },
    { "write",       cmd_write },
    { "sync",        cmd_sync },
    { NULL,          NULL }
};

int shell_is_running(void) {
    return running;
}

int execute_builtin(int argc, char **argv) {
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
