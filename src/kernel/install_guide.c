#include "install_guide.h"
#include "keyboard.h"
#include "serial.h"
#include "syscall.h"
#include "pwid.h"
#include "hvfs.h"
#include "string.h"

static void print(const char *s) {
    serial_puts(SERIAL_COM1, s);
}

static void println(const char *s) {
    serial_puts(SERIAL_COM1, s);
    serial_puts(SERIAL_COM1, "\n");
}

static void simple_delay(int count) {
    for (volatile int i = 0; i < count * 1000000; i++);
}

static void welcome_page(void) {
    println("");
    println("========================================");
    println("        AntX Installation Wizard");
    println("========================================");
    println("");
    println("Welcome to AntX Operating System!");
    println("");
    println("This wizard will guide you through the");
    println("initial system setup. This process will");
    println("only run once.");
    println("");
    println("Press ENTER to continue...");
    
    char buf[16];
    keyboard_read_line(buf, sizeof(buf));
}

static int config_root_pwid(void) {
    char pw1[64];
    char pw2[64];
    
    println("");
    println("--- Step 1: Root Account Setup ---");
    println("");
    println("Creating the root (administrator) account.");
    println("This account has full system access.");
    println("");
    
    while (1) {
        print("Enter root password (min 4 chars): ");
        int len1 = keyboard_read_line(pw1, sizeof(pw1));
        
        if (len1 < INSTALL_MIN_PASSWORD_LEN) {
            print("Password too short! Minimum ");
            serial_put_dec(SERIAL_COM1, INSTALL_MIN_PASSWORD_LEN);
            println(" characters required.");
            continue;
        }
        
        print("Confirm root password: ");
        int len2 = keyboard_read_line(pw2, sizeof(pw2));
        
        if (len1 != len2 || strcmp(pw1, pw2) != 0) {
            println("Passwords do not match! Please try again.");
            continue;
        }
        
        println("");
        println("Creating root account...");
        
        int result = pwid_create_original_root(pw1);
        
        if (result == 0) {
            println("Root account created successfully!");
            pwid_login("root", pw1);
            println("Logged in as root.");
            return 0;
        } else {
            println("Failed to create root account. Please try again.");
        }
    }
}

static void config_system(void) {
    char hostname[64];
    
    println("");
    println("--- Step 2: System Configuration ---");
    println("");
    
    print("Enter hostname (default: localhost): ");
    int hostname_len = keyboard_read_line(hostname, sizeof(hostname));
    
    if (hostname_len == 0) {
        strcpy(hostname, INSTALL_DEFAULT_HOSTNAME);
    }
    
    int result = sys_sethostname(hostname, strlen(hostname));
    if (result == 0) {
        print("Hostname set to: ");
        println(hostname);
    } else {
        println("Warning: Failed to set hostname, using default.");
    }
    
    int fd = sys_fs_open(INSTALL_HOSTNAME_FILE, HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, 0644);
    if (fd >= 0) {
        sys_fs_write(fd, hostname, strlen(hostname));
        sys_fs_close(fd);
    }
    
    println("");
    println("System configuration complete!");
}

static int complete_page(void) {
    println("");
    println("--- Step 3: Finalizing Installation ---");
    println("");
    
    println("Syncing filesystem to disk...");
    sys_fs_sync();
    
    println("Creating installation marker...");
    
    int fd = sys_fs_open(INSTALL_MARKER_FILE, HVFS_O_CREAT | HVFS_O_WRONLY, 0600);
    if (fd < 0) {
        println("Error: Failed to create installation marker!");
        return -1;
    }
    
    const char *marker_content = "installed\n";
    sys_fs_write(fd, marker_content, strlen(marker_content));
    sys_fs_close(fd);
    
    sys_fs_sync();
    
    println("");
    println("========================================");
    println("     Installation Complete!");
    println("========================================");
    println("");
    println("AntX is now ready for use.");
    println("");
    println("Starting shell in 3 seconds...");
    
    simple_delay(3);
    
    return 0;
}

int install_guide_check_needed(void) {
    int fd = sys_fs_open(INSTALL_MARKER_FILE, HVFS_O_RDONLY, 0);
    if (fd >= 0) {
        sys_fs_close(fd);
        return 0;
    }
    return 1;
}

int install_guide_create_marker(void) {
    int fd = sys_fs_open(INSTALL_MARKER_FILE, HVFS_O_CREAT | HVFS_O_WRONLY, 0600);
    if (fd < 0) {
        return -1;
    }
    
    const char *marker_content = "installed\n";
    sys_fs_write(fd, marker_content, strlen(marker_content));
    sys_fs_close(fd);
    
    return 0;
}

void install_guide_run(void) {
    welcome_page();
    
    if (config_root_pwid() != 0) {
        println("Installation failed at root account setup.");
        println("Please restart the system.");
        return;
    }
    
    config_system();
    
    if (complete_page() != 0) {
        println("Warning: Installation may be incomplete.");
    }
}
