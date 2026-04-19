#include "user_install.h"
#include "user/user.h"

#define INSTALL_MIN_PASSWORD_LEN  4
#define INSTALL_DEFAULT_HOSTNAME  "localhost"
#define INSTALL_MARKER_FILE       "/.antx_installed"
#define INSTALL_HOSTNAME_FILE     "/etc/hostname"

static void welcome_page(void) {
    user_print("\n");
    user_println("========================================");
    user_println("        AntX Installation Wizard");
    user_println("========================================");
    user_print("\n");
    user_println("Welcome to AntX Operating System!");
    user_print("\n");
    user_println("This wizard will guide you through the");
    user_println("initial system setup. This process will");
    user_println("only run once.");
    user_print("\n");
    user_println("Press ENTER to continue...");
    
    char buf[16];
    user_read_line(buf, sizeof(buf));
}

static int config_root_pwid(void) {
    char pw1[64];
    char pw2[64];
    
    user_print("\n");
    user_println("--- Step 1: Root Account Setup ---");
    user_print("\n");
    user_println("Creating the root (administrator) account.");
    user_println("This account has full system access.");
    user_println("Note: Root account note is fixed as 'root' and cannot be changed.");
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
        user_println("Creating root account...");
        
        int result = user_auth_create_original_root(pw1);
        
        if (result >= 0) {
            user_println("Root account created successfully!");
            return 0;
        } else {
            user_print("Failed to create root account (error: ");
            user_print_dec(result);
            user_println("). Please try again.");
        }
    }
}

static void config_system(void) {
    char hostname[64];
    
    user_print("\n");
    user_println("--- Step 2: System Configuration ---");
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
    user_println("--- Step 3: Finalizing Installation ---");
    user_print("\n");
    
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
    user_println("AntX is now ready for use.");
    user_print("\n");
    user_println("Starting shell in 3 seconds...");
    
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
    
    if (config_root_pwid() != 0) {
        user_println("Installation failed at root account setup.");
        user_println("Please restart the system.");
        return;
    }
    
    config_system();
    
    if (complete_page() != 0) {
        user_println("Warning: Installation may be incomplete.");
    }
}
