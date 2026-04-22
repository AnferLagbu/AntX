#include "builtins.h"
#include "user/user.h"

static int running = 1;

int cmd_help(int argc, char **argv) {
    (void)argc;
    (void)argv;
    user_println("");
    user_println("axsh - AntX Shell Commands");
    user_println("==========================");
    user_println("");
    user_println("General:");
    user_println("  help          Show this help");
    user_println("  cls           Clear screen");
    user_println("  echo [text]   Print text");
    user_println("  exit          Exit shell");
    user_println("");
    user_println("File (f*):");
    user_println("  fls [path]    List directory");
    user_println("  fcd <dir>     Change directory");
    user_println("  fpwd          Print working directory");
    user_println("  fcat <file>   Display file");
    user_println("  fmk <file>    Create file");
    user_println("  fmd <dir>     Create directory");
    user_println("  frm <path>    Remove file/dir");
    user_println("  fput <f> <t>  Write text to file");
    user_println("  fsync         Sync to disk");
    user_println("");
    user_println("Identity (i*):");
    user_println("  ilogin <n> <pw>  Login with note and password");
    user_println("  ilogout         Logout");
    user_println("  iwho            Show current PWID");
    user_println("  ipasswd         Change password");
    user_println("");
    user_println("System (s*):");
    user_println("  shost [name]  Show/set hostname");
    user_println("  sver          Show system version");
    user_println("");
    return 0;
}

int cmd_cls(int argc, char **argv) {
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

int cmd_fls(int argc, char **argv) {
    const char *path = "/";
    if (argc > 1) path = argv[1];
    
    int fd = user_open(path, HVFS_O_RDONLY, 0);
    if (fd < 0) {
        user_print("fls: '");
        user_print(path);
        user_println("' not found");
        return 1;
    }
    
    struct user_dirent entry;
    int count = 0;
    
    while (sys_fs_read_dir(fd, &entry) > 0) {
        if (entry.inode != 0) {
            if (entry.file_type == HVFS_TYPE_DIR) {
                user_print("  [D] ");
            } else {
                user_print("  [F] ");
            }
            user_println(entry.name);
            count++;
        }
    }
    
    user_close(fd);
    
    if (count == 0) {
        user_println("  (empty)");
    }
    
    return 0;
}

int cmd_fcd(int argc, char **argv) {
    if (argc < 2) {
        user_println("fcd: missing path");
        return 1;
    }
    
    int result = user_chdir(argv[1]);
    if (result < 0) {
        user_print("fcd: '");
        user_print(argv[1]);
        user_println("' not found");
        return 1;
    }
    
    return 0;
}

int cmd_fpwd(int argc, char **argv) {
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

int cmd_fcat(int argc, char **argv) {
    if (argc < 2) {
        user_println("fcat: missing file");
        return 1;
    }
    
    int fd = user_open(argv[1], HVFS_O_RDONLY, 0);
    if (fd < 0) {
        user_print("fcat: '");
        user_print(argv[1]);
        user_println("' not found");
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

int cmd_fmk(int argc, char **argv) {
    if (argc < 2) {
        user_println("fmk: missing file name");
        return 1;
    }
    
    int fd = user_open(argv[1], HVFS_O_CREAT | HVFS_O_WRONLY, 0);
    if (fd < 0) {
        user_print("fmk: cannot create '");
        user_print(argv[1]);
        user_println("'");
        return 1;
    }
    
    user_close(fd);
    user_print("Created: ");
    user_println(argv[1]);
    return 0;
}

int cmd_fmd(int argc, char **argv) {
    if (argc < 2) {
        user_println("fmd: missing directory name");
        return 1;
    }
    
    int result = user_mkdir(argv[1], 0755);
    if (result < 0) {
        user_print("fmd: cannot create '");
        user_print(argv[1]);
        user_println("'");
        return 1;
    }
    
    user_print("Created: ");
    user_println(argv[1]);
    return 0;
}

int cmd_frm(int argc, char **argv) {
    if (argc < 2) {
        user_println("frm: missing path");
        return 1;
    }
    
    int result = user_unlink(argv[1]);
    if (result < 0) {
        user_print("frm: cannot remove '");
        user_print(argv[1]);
        user_println("'");
        return 1;
    }
    
    user_print("Removed: ");
    user_println(argv[1]);
    return 0;
}

int cmd_fput(int argc, char **argv) {
    if (argc < 3) {
        user_println("fput: usage: fput <file> <text>");
        return 1;
    }
    
    int fd = user_open(argv[1], HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, 0);
    if (fd < 0) {
        user_print("fput: cannot open '");
        user_print(argv[1]);
        user_println("'");
        return 1;
    }
    
    int len = user_strlen(argv[2]);
    int n = user_write(fd, argv[2], len);
    user_close(fd);
    
    user_print("Wrote ");
    user_print_dec(n);
    user_println(" bytes");
    
    return 0;
}

int cmd_fsync(int argc, char **argv) {
    (void)argc;
    (void)argv;
    
    user_sync();
    user_println("Synced");
    return 0;
}

int cmd_ilogin(int argc, char **argv) {
    if (argc < 3) {
        user_println("ilogin: usage: ilogin <note> <password>");
        return 1;
    }
    
    int result = user_auth_login(argv[2], argv[1]);
    if (result > 0) {
        user_print("Logged in: ");
        user_print_hex(sys_proc_get_pwid());
        user_print("\n");
        return 0;
    } else if (result == -104) {
        user_println("ilogin: wrong password");
        return 1;
    } else if (result == -101) {
        user_println("ilogin: not found");
        return 1;
    } else {
        user_println("ilogin: failed");
        return 1;
    }
}

int cmd_ilogout(int argc, char **argv) {
    (void)argc;
    (void)argv;
    user_auth_logout();
    user_println("Logged out");
    return 0;
}

int cmd_iwho(int argc, char **argv) {
    (void)argc;
    (void)argv;
    uint64_t pwid = sys_proc_get_pwid();
    if (pwid == 0) {
        user_println("Not logged in");
        return 0;
    }
    
    user_print("PWID: ");
    user_print_hex(pwid);
    user_print("\n");
    return 0;
}

static void prompt_password_change(void) {
    char old_pw[64];
    char new_pw[64];
    char confirm_pw[64];
    
    user_print("Current password: ");
    user_read_line(old_pw, sizeof(old_pw));
    
    user_print("New password: ");
    user_read_line(new_pw, sizeof(new_pw));
    
    user_print("Confirm: ");
    user_read_line(confirm_pw, sizeof(confirm_pw));
    
    if (user_strcmp(new_pw, confirm_pw) != 0) {
        user_println("Mismatch");
        return;
    }
    
    int result = user_auth_change_password(old_pw, new_pw);
    if (result == 0) {
        user_println("Password changed");
    } else if (result == -104) {
        user_println("Wrong current password");
    } else {
        user_println("Failed");
    }
}

int cmd_ipasswd(int argc, char **argv) {
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

int cmd_shost(int argc, char **argv) {
    char hostname[64];
    
    if (argc == 1) {
        int result = user_get_hostname(hostname, sizeof(hostname));
        if (result == 0) {
            user_println(hostname);
        } else {
            user_println("Error");
        }
    } else {
        int len = user_strlen(argv[1]);
        int result = user_set_hostname(argv[1], len);
        if (result == 0) {
            user_print("Host: ");
            user_println(argv[1]);
        } else if (result == -105) {
            user_println("Root required");
        } else {
            user_println("Error");
        }
    }
    return 0;
}

int cmd_sver(int argc, char **argv) {
    (void)argc;
    (void)argv;
    user_println("AntX v0.1.0");
    user_println("Kernel: QueenX");
    user_println("Shell: axsh");
    return 0;
}

struct builtin_cmd builtins[] = {
    { "help",    cmd_help },
    { "cls",     cmd_cls },
    { "echo",    cmd_echo },
    { "exit",    cmd_exit },
    
    { "fls",     cmd_fls },
    { "fcd",     cmd_fcd },
    { "fpwd",    cmd_fpwd },
    { "fcat",    cmd_fcat },
    { "fmk",     cmd_fmk },
    { "fmd",     cmd_fmd },
    { "frm",     cmd_frm },
    { "fput",    cmd_fput },
    { "fsync",   cmd_fsync },
    
    { "ilogin",  cmd_ilogin },
    { "ilogout", cmd_ilogout },
    { "iwho",    cmd_iwho },
    { "ipasswd", cmd_ipasswd },
    
    { "shost",   cmd_shost },
    { "sver",    cmd_sver },
    
    { NULL,      NULL }
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
    
    user_print("Unknown: ");
    user_println(argv[0]);
    return 1;
}
