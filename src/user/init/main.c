#include "user/user.h"
#include "user_install.h"

#define FSTAB_PATH "/cfg/system/fstab"
#define SHELL_PATH "/app/sys/axsh"
#define MAX_LINE_LEN 256

static void mount_filesystems_from_fstab(void) {
    int fd = user_open(FSTAB_PATH, HVFS_O_RDONLY, 0);
    
    if (fd < 0) {
        user_println("[init] No fstab found, using defaults");
        
        user_println("[init] Mounting /dev (devfs)...");
        if (user_mount("none", "/dev", "devfs", "defaults") != 0) {
            user_println("  [WARN] Failed to mount /dev");
        }
        
        user_println("[init] Mounting /proc (procfs)...");
        if (user_mount("none", "/proc", "procfs", "defaults") != 0) {
            user_println("  [WARN] Failed to mount /proc");
        }
        
        user_println("[init] Mounting /temp (ramfs)...");
        if (user_mount("none", "/temp", "ramfs", "defaults") != 0) {
            user_println("  [WARN] Failed to mount /temp");
        }
        
        return;
    }
    
    user_println("[init] Reading fstab...");
    
    char line[MAX_LINE_LEN];
    int line_pos = 0;
    char c;
    int mounts_done = 0;
    
    while (user_read(fd, &c, 1) > 0) {
        if (c == '\n') {
            line[line_pos] = '\0';
            
            if (line_pos > 0 && line[0] != '#' && line[0] != ' ') {
                char source[64] = {0};
                char target[64] = {0};
                char fstype[32] = {0};
                char options[64] = {0};
                
                int spos = 0, tpos = 0, fpos = 0, opos = 0;
                int field = 0;
                
                for (int i = 0; i < line_pos; i++) {
                    if (line[i] == ' ' || line[i] == '\t') {
                        if (field < 3) {
                            while (i < line_pos && (line[i] == ' ' || line[i] == '\t')) i++;
                            i--;
                            field++;
                        }
                    } else {
                        switch (field) {
                            case 0: source[spos++] = line[i]; break;
                            case 1: target[tpos++] = line[i]; break;
                            case 2: fstype[fpos++] = line[i]; break;
                            case 3: options[opos++] = line[i]; break;
                        }
                    }
                }
                
                if (target[0] != '\0' && fstype[0] != '\0') {
                    user_print("[init] Mounting ");
                    user_print(target);
                    user_print(" (");
                    user_print(fstype);
                    user_println(")...");
                    
                    int result = user_mount(source, target, fstype, options);
                    if (result == 0) {
                        user_println("  [OK] Mounted");
                        mounts_done++;
                    } else {
                        user_println("  [WARN] Failed to mount");
                    }
                }
            }
            
            line_pos = 0;
        } else if (line_pos < MAX_LINE_LEN - 1) {
            line[line_pos++] = c;
        }
    }
    
    user_close(fd);
    
    user_print("[init] Mounted ");
    user_print_dec(mounts_done);
    user_println(" filesystems from fstab");
}

static void start_shell(void) {
    user_println("[init] Starting axsh...");
    
    const char *argv[] = {"axsh", NULL};
    
    int64_t result = sys_proc_execute(SHELL_PATH, (char *const *)argv, NULL);
    
    if (result < 0) {
        user_print("[init] ERROR: Failed to start shell (error: ");
        user_print_dec(result);
        user_println(")");
        user_println("[init] System halted.");
        while (1) {
            sys_proc_yield_cpu();
        }
    }
}

void init_main(void) {
    user_println("");
    user_println("[init] AntX init process started");
    user_println("");
    
    if (user_install_check_needed()) {
        user_println("[init] First boot detected, launching installation wizard...");
        user_println("");
        user_install_run();
        user_println("");
        user_println("[init] Installation complete, continuing boot...");
        user_println("");
    }
    
    mount_filesystems_from_fstab();
    user_println("");
    
    start_shell();
    
    while (1) {
        sys_proc_yield_cpu();
    }
}

void _start(void) {
    init_main();
    sys_proc_exit(0);
}
