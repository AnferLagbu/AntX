#include "smart_mount.h"
#include "config.h"
#include "kernel.h"
#include "vfs.h"
#include "hvfs.h"  /* Contains all HVFS declarations */
#include "klog.h"

/* No need for extern declarations - they're in the headers */

static int detect_persistent_storage(void) {
    int status = hvfs_check_disk();
    
    switch (status) {
        case HVFS_DISK_OK:
            klog_boot("[SMART] Disk detected (formatted)");
            return 1;
        case HVFS_DISK_UNFORMATTED:
            klog_boot("[SMART] Disk detected (unformatted)");
            return 1;
        case HVFS_DISK_NO_DISK:
            klog_boot("[SMART] No disk found");
            return 0;
        default:
            return -1;
    }
}

#if CONFIG_MODE_RELEASE

int smart_mount_root(void) {
    klog_boot("[SMART] Release mode: requiring disk");
    
    if (detect_persistent_storage() <= 0) {
        panic("RELEASE mode requires persistent storage!");
    }
    
    int status = hvfs_check_disk();
    if (status == HVFS_DISK_UNFORMATTED) {
        #if CONFIG_PERSISTENT_AUTO_FORMAT
        klog_boot("[SMART] Auto-formatting...");
        hvfs_format();
        #else
        panic("Disk not formatted!");
        #endif
    }
    
    if (vfs_mount("/", "diskfs") != 0) {
        panic("Failed to mount persistent root!");
    }
    
    return 0;
}

char get_persistent_mode(void) { return 'R'; }

#else

int smart_mount_root(void) {
    int disk_available = detect_persistent_storage();
    
    if (disk_available > 0) {
        int status = hvfs_check_disk();
        
        if (status == HVFS_DISK_OK) {
            klog_boot("[SMART] Auto-detecting disk...");
            if (vfs_mount("/", "diskfs") == 0) {
                klog_boot("[SMART] Mounted from disk");
                return 0;
            }
        } else if (status == HVFS_DISK_UNFORMATTED) {
            #if CONFIG_PERSISTENT_AUTO_FORMAT
            klog_boot("[SMART] Auto-formatting...");
            hvfs_format();
            if (vfs_mount("/", "diskfs") == 0) {
                klog_boot("[SMART] Mounted from formatted disk");
                return 0;
            }
            #endif
        }
        
        klog_boot("[SMART] Disk present but unreadable — using RamFS");
    } else {
        klog_boot("[SMART] Using RamFS (default)");
    }
    
    if (vfs_mount("/", "ramfs") != 0) {
        panic("Failed to mount RamFS!");
    }
    
    return 0;
}

char get_persistent_mode(void) { 
#if CONFIG_MODE_TEST
    return 'T'; 
#else
    return 'D'; 
#endif
}

#endif
