#include "user_install.h"
#include "user/user.h"

#define INSTALL_MIN_PASSWORD_LEN  4
#define INSTALL_DEFAULT_HOSTNAME  "localhost"
#define INSTALL_MARKER_FILE       "/.antx_installed"
#define INSTALL_HOSTNAME_FILE     "/cfg/system/hostname"
#define INSTALL_FSTAB_FILE        "/cfg/system/fstab"
#define INSTALL_PWID_DB_PATH      "/cfg/system/pwid.db"

#define MAX_DISKS 4

static int selected_disk = -1;
static struct user_disk_info disk_list[MAX_DISKS];
static int disk_count = 0;

static void create_directory(const char *path) {
    user_mkdir(path, 0755);
}

static void create_directory_structure(void) {
    user_println("Creating directory structure...");
    
    create_directory("/cfg");
    create_directory("/cfg/boot");
    create_directory("/cfg/kernel");
    create_directory("/cfg/system");
    create_directory("/cfg/gui");
    
    create_directory("/app");
    create_directory("/app/bin");
    create_directory("/app/sys");
    
    create_directory("/data");
    create_directory("/data/id");
    create_directory("/data/share");
    create_directory("/data/var");
    create_directory("/data/var/log");
    create_directory("/data/var/run");
    
    create_directory("/gui");
    create_directory("/gui/font");
    create_directory("/gui/theme");
    create_directory("/gui/wallpaper");
    create_directory("/gui/cursor");
    
    create_directory("/dev");
    create_directory("/proc");
    create_directory("/temp");
    create_directory("/mnt");
    
    user_println("  [OK] Directory structure created");
}

static void create_fstab(void) {
    int fd = user_open(INSTALL_FSTAB_FILE, HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, 0644);
    if (fd < 0) {
        user_println("  [WARN] Failed to create fstab");
        return;
    }
    
    const char *fstab_content = 
        "# AntX Filesystem Configuration\n"
        "# Format: source mountpoint type options\n"
        "\n"
        "none    /dev    devfs   defaults\n"
        "none    /proc   procfs  defaults\n"
        "none    /temp   ramfs   defaults,size=64M\n";
    
    user_write(fd, fstab_content, user_strlen(fstab_content));
    user_close(fd);
    
    user_println("  [OK] fstab created");
}

static void welcome_page(void) {
    user_print("\n");
    user_println("========================================");
    user_println("        AntX Installation Wizard");
    user_println("========================================");
    user_print("\n");
    user_println("Welcome to AntX Operating System!");
    user_print("\n");
    user_println("This wizard will guide you through the");
    user_println("system installation process.");
    user_print("\n");
    user_println("Press ENTER to continue...");
    
    char buf[16];
    user_read_line(buf, sizeof(buf));
}

static int detect_disks(void) {
    user_print("\n");
    user_println("--- Step 1: Disk Detection ---");
    user_print("\n");
    user_println("Scanning for available disks...");
    
    uint64_t disks[MAX_DISKS];
    int64_t count = sys_disk_list(disks, MAX_DISKS);
    
    if (count <= 0) {
        user_println("  [ERROR] No disks detected!");
        user_println("  Please ensure a disk is connected.");
        return -1;
    }
    
    disk_count = (int)count;
    
    user_print("\n");
    user_print("Detected ");
    user_print_dec(disk_count);
    user_println(" disk(s):");
    user_println("");
    
    for (int i = 0; i < disk_count; i++) {
        sys_disk_info((uint32_t)disks[i], &disk_list[i]);
        
        user_print("  [");
        user_print_dec(i);
        user_print("] Disk ");
        user_print_dec(disk_list[i].disk_id);
        user_print(": ");
        user_print(disk_list[i].model);
        user_print(" (");
        
        uint32_t size_mb = disk_list[i].sectors / 2 / 1024;
        if (size_mb >= 1024) {
            user_print_dec(size_mb / 1024);
            user_print(" GB)");
        } else {
            user_print_dec(size_mb);
            user_print(" MB)");
        }
        
        user_println("");
    }
    
    return 0;
}

static int select_disk(void) {
    user_print("\n");
    user_println("Select a disk for installation:");
    user_print("Enter disk number (0-");
    user_print_dec(disk_count - 1);
    user_print("): ");
    
    char buf[8];
    int len = user_read_line(buf, sizeof(buf));
    
    if (len == 0) {
        user_println("  [ERROR] No selection made.");
        return -1;
    }
    
    int selection = 0;
    for (int i = 0; i < len; i++) {
        if (buf[i] >= '0' && buf[i] <= '9') {
            selection = selection * 10 + (buf[i] - '0');
        }
    }
    
    if (selection < 0 || selection >= disk_count) {
        user_println("  [ERROR] Invalid selection.");
        return -1;
    }
    
    selected_disk = selection;
    
    user_print("\n");
    user_println("  [WARNING] ALL DATA ON THIS DISK WILL BE ERASED!");
    user_print("  Selected: ");
    user_print(disk_list[selection].model);
    user_println("");
    user_print("\n");
    user_print("Type 'yes' to confirm: ");
    
    char confirm[8];
    user_read_line(confirm, sizeof(confirm));
    
    if (confirm[0] != 'y' || confirm[1] != 'e' || confirm[2] != 's') {
        user_println("  Installation cancelled.");
        return -1;
    }
    
    return 0;
}

static int format_disk(void) {
    user_print("\n");
    user_println("--- Step 2: Disk Formatting ---");
    user_print("\n");
    user_print("Formatting disk ");
    user_print_dec(disk_list[selected_disk].disk_id);
    user_println(" with HvFS...");
    
    int64_t result = sys_disk_format(disk_list[selected_disk].disk_id, "hvfs");
    
    if (result != 0) {
        user_print("  [ERROR] Format failed (error: ");
        user_print_dec(result);
        user_println(")");
        return -1;
    }
    
    user_println("  [OK] Disk formatted successfully");
    return 0;
}

static int install_system_files(void) {
    user_print("\n");
    user_println("--- Step 3: System File Installation ---");
    user_print("\n");
    user_println("Installing system files...");
    
    int src_kernel = user_open("/boot/kernel.bin", HVFS_O_RDONLY, 0);
    if (src_kernel >= 0) {
        int dst_kernel = user_open("/cfg/boot/kernel.bin", HVFS_O_CREAT | HVFS_O_WRONLY, 0644);
        if (dst_kernel >= 0) {
            char buf[4096];
            int n;
            while ((n = user_read(src_kernel, buf, sizeof(buf))) > 0) {
                user_write(dst_kernel, buf, n);
            }
            user_close(dst_kernel);
            user_println("  [OK] Kernel installed");
        }
        user_close(src_kernel);
    }
    
    int src_init = user_open("/bin/init", HVFS_O_RDONLY, 0);
    if (src_init >= 0) {
        int dst_init = user_open("/app/sys/init", HVFS_O_CREAT | HVFS_O_WRONLY, 0755);
        if (dst_init >= 0) {
            char buf[4096];
            int n;
            while ((n = user_read(src_init, buf, sizeof(buf))) > 0) {
                user_write(dst_init, buf, n);
            }
            user_close(dst_init);
            user_println("  [OK] Init process installed");
        }
        user_close(src_init);
    }
    
    int src_xsh = user_open("/bin/axsh", HVFS_O_RDONLY, 0);
    if (src_xsh >= 0) {
        int dst_xsh = user_open("/app/sys/axsh", HVFS_O_CREAT | HVFS_O_WRONLY, 0755);
        if (dst_xsh >= 0) {
            char buf[4096];
            int n;
            while ((n = user_read(src_xsh, buf, sizeof(buf))) > 0) {
                user_write(dst_xsh, buf, n);
            }
            user_close(dst_xsh);
            user_println("  [OK] axsh installed");
        }
        user_close(src_xsh);
    }
    
    int src_install = user_open("/bin/install", HVFS_O_RDONLY, 0);
    if (src_install >= 0) {
        int dst_install = user_open("/app/sys/installguide", HVFS_O_CREAT | HVFS_O_WRONLY, 0755);
        if (dst_install >= 0) {
            char buf[4096];
            int n;
            while ((n = user_read(src_install, buf, sizeof(buf))) > 0) {
                user_write(dst_install, buf, n);
            }
            user_close(dst_install);
            user_println("  [OK] Install guide installed");
        }
        user_close(src_install);
    }
    
    return 0;
}

static int config_root_pwid(void) {
    char pw1[64];
    char pw2[64];
    
    user_print("\n");
    user_println("--- Step 4: Root PWID Setup ---");
    user_print("\n");
    user_println("Creating the root (administrator) identity.");
    user_println("This identity has full system access.");
    user_println("Note: Root note is fixed as 'root'.");
    user_print("\n");
    
    while (1) {
        user_print("Enter root password (min 4 chars): ");
        int len1 = user_read_line(pw1, sizeof(pw1));
        
        if (len1 < INSTALL_MIN_PASSWORD_LEN) {
            user_print("Password too short! Minimum ");
            user_print_dec(INSTALL_MIN_PASSWORD_LEN);
            user_println(" characters required.");
            continue;
        }
        
        user_print("Confirm root password: ");
        int len2 = user_read_line(pw2, sizeof(pw2));
        
        if (len1 != len2 || user_strcmp(pw1, pw2) != 0) {
            user_println("Passwords do not match! Please try again.");
            continue;
        }
        
        user_print("\n");
        user_println("Creating root identity...");
        
        int result = user_auth_create_original_root(pw1);
        
        if (result >= 0) {
            user_println("Root identity created successfully!");
            return 0;
        } else {
            user_print("Failed to create root identity (error: ");
            user_print_dec(result);
            user_println("). Please try again.");
        }
    }
}

static void config_system(void) {
    char hostname[64];
    
    user_print("\n");
    user_println("--- Step 5: System Configuration ---");
    user_print("\n");
    
    user_print("Enter hostname (default: localhost): ");
    int hostname_len = user_read_line(hostname, sizeof(hostname));
    
    if (hostname_len == 0) {
        user_strcpy(hostname, INSTALL_DEFAULT_HOSTNAME);
    }
    
    int result = user_set_hostname(hostname, user_strlen(hostname));
    if (result == 0) {
        user_print("Hostname set to: ");
        user_println(hostname);
    } else {
        user_println("Warning: Failed to set hostname, using default.");
    }
    
    int fd = user_open(INSTALL_HOSTNAME_FILE, HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, 0644);
    if (fd >= 0) {
        user_write(fd, hostname, user_strlen(hostname));
        user_close(fd);
    }
    
    user_print("\n");
    user_println("System configuration complete!");
}

static int complete_page(void) {
    user_print("\n");
    user_println("--- Step 6: Finalizing Installation ---");
    user_print("\n");
    
    create_directory_structure();
    create_fstab();
    
    user_println("Syncing filesystem to disk...");
    user_sync();
    
    user_println("Creating installation marker...");
    
    int fd = user_open(INSTALL_MARKER_FILE, HVFS_O_CREAT | HVFS_O_WRONLY, 0600);
    if (fd < 0) {
        user_println("Error: Failed to create installation marker!");
        return -1;
    }
    
    const char *marker_content = "installed\n";
    user_write(fd, marker_content, user_strlen(marker_content));
    user_close(fd);
    
    user_sync();
    
    user_print("\n");
    user_println("========================================");
    user_println("     Installation Complete!");
    user_println("========================================");
    user_print("\n");
    user_println("AntX has been installed to your disk.");
    user_println("Please remove the installation media");
    user_println("and reboot your system.");
    user_print("\n");
    user_println("Starting axsh in 3 seconds...");
    
    user_delay(3);
    
    return 0;
}

int user_install_check_needed(void) {
    int fd = user_open(INSTALL_MARKER_FILE, HVFS_O_RDONLY, 0);
    if (fd >= 0) {
        user_close(fd);
        return 0;
    }
    return 1;
}

int user_install_create_marker(void) {
    int fd = user_open(INSTALL_MARKER_FILE, HVFS_O_CREAT | HVFS_O_WRONLY, 0600);
    if (fd < 0) {
        return -1;
    }
    
    const char *marker_content = "installed\n";
    user_write(fd, marker_content, user_strlen(marker_content));
    user_close(fd);
    
    return 0;
}

void user_install_run(void) {
    welcome_page();
    
    if (detect_disks() != 0) {
        user_println("Installation aborted: No disks available.");
        return;
    }
    
    if (select_disk() != 0) {
        user_println("Installation cancelled by user.");
        return;
    }
    
    if (format_disk() != 0) {
        user_println("Installation failed: Disk formatting error.");
        return;
    }
    
    if (install_system_files() != 0) {
        user_println("Installation failed: File copy error.");
        return;
    }
    
    if (config_root_pwid() != 0) {
        user_println("Installation failed at root identity setup.");
        return;
    }
    
    config_system();
    
    if (complete_page() != 0) {
        user_println("Warning: Installation may be incomplete.");
    }
}
