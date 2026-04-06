#include "vfs.h"
#include "serial.h"
#include "pwid.h"

#define DEVFS_MAX_DEVICES      16
#define DEVFS_MAX_NAME         32

#define DEV_TYPE_NULL          0
#define DEV_TYPE_ZERO          1
#define DEV_TYPE_CONSOLE       2
#define DEV_TYPE_TTY           3

struct devfs_device {
    char name[DEVFS_MAX_NAME];
    uint8_t type;
    uint8_t used;
};

static struct devfs_device devfs_devices[DEVFS_MAX_DEVICES];
static int devfs_device_count = 0;
static struct vfs_filesystem devfs_fs;

static int str_len(const char *s) {
    int len = 0;
    while (s[len]) len++;
    return len;
}

static int str_cmp(const char *s1, const char *s2) {
    while (*s1 && *s2 && *s1 == *s2) {
        s1++; s2++;
    }
    return *s1 - *s2;
}

static void str_cpy(char *dest, const char *src) {
    while (*src) {
        *dest++ = *src++;
    }
    *dest = '\0';
}

static void mem_set(void *dest, uint8_t val, uint32_t count) {
    uint8_t *d = (uint8_t *)dest;
    for (uint32_t i = 0; i < count; i++) {
        d[i] = val;
    }
}

static struct devfs_device* find_device(const char *name) {
    for (int i = 0; i < DEVFS_MAX_DEVICES; i++) {
        if (devfs_devices[i].used && str_cmp(devfs_devices[i].name, name) == 0) {
            return &devfs_devices[i];
        }
    }
    return NULL;
}

static int devfs_open(struct vfs_file *file, int flags, uint64_t pwid) {
    if (file == NULL) return -1;
    
    const char *dev_name = file->path;
    if (*dev_name == '/') dev_name++;
    
    struct devfs_device *dev = find_device(dev_name);
    if (dev == NULL) {
        return -1;
    }
    
    file->inode_num = dev->type;
    file->offset = 0;
    file->type = VFS_TYPE_DEV;
    file->private_data = dev;
    
    return 0;
}

static int devfs_close(struct vfs_file *file) {
    if (file == NULL) return -1;
    file->private_data = NULL;
    return 0;
}

static int devfs_read(struct vfs_file *file, void *buf, uint32_t count) {
    if (file == NULL || buf == NULL) return -1;
    
    struct devfs_device *dev = (struct devfs_device *)file->private_data;
    if (dev == NULL) return -1;
    
    switch (dev->type) {
        case DEV_TYPE_NULL:
            return 0;
            
        case DEV_TYPE_ZERO:
            mem_set(buf, 0, count);
            return count;
            
        case DEV_TYPE_CONSOLE:
        case DEV_TYPE_TTY:
            return 0;
            
        default:
            return -1;
    }
}

static int devfs_write(struct vfs_file *file, const void *buf, uint32_t count) {
    if (file == NULL || buf == NULL) return -1;
    
    struct devfs_device *dev = (struct devfs_device *)file->private_data;
    if (dev == NULL) return -1;
    
    switch (dev->type) {
        case DEV_TYPE_NULL:
            return count;
            
        case DEV_TYPE_ZERO:
            return count;
            
        case DEV_TYPE_CONSOLE:
        case DEV_TYPE_TTY:
            serial_puts(SERIAL_COM1, (const char *)buf);
            return count;
            
        default:
            return -1;
    }
}

static int devfs_seek(struct vfs_file *file, int64_t offset, int whence) {
    (void)offset;
    (void)whence;
    if (file == NULL) return -1;
    return 0;
}

static int devfs_readdir(struct vfs_file *file, struct vfs_dirent *entry) {
    if (file == NULL || entry == NULL) return -1;
    
    int entry_idx = file->offset;
    
    while (entry_idx < DEVFS_MAX_DEVICES && !devfs_devices[entry_idx].used) {
        entry_idx++;
    }
    
    if (entry_idx >= DEVFS_MAX_DEVICES) {
        return 0;
    }
    
    entry->inode = devfs_devices[entry_idx].type;
    entry->type = VFS_TYPE_DEV;
    str_cpy(entry->name, devfs_devices[entry_idx].name);
    
    file->offset = entry_idx + 1;
    
    return 1;
}

static int devfs_stat(struct vfs_file *file, struct vfs_stat *st) {
    if (file == NULL || st == NULL) return -1;
    
    const char *dev_name = file->path;
    if (*dev_name == '/') dev_name++;
    
    struct devfs_device *dev = find_device(dev_name);
    if (dev == NULL) return -1;
    
    st->inode_num = dev->type;
    st->mode = 0666;
    st->size = 0;
    st->type = VFS_TYPE_DEV;
    st->perm = 0666;
    
    return 0;
}

static int devfs_mount(const char *path) {
    for (int i = 0; i < DEVFS_MAX_DEVICES; i++) {
        devfs_devices[i].used = 0;
    }
    
    devfs_devices[0].used = 1;
    str_cpy(devfs_devices[0].name, "null");
    devfs_devices[0].type = DEV_TYPE_NULL;
    
    devfs_devices[1].used = 1;
    str_cpy(devfs_devices[1].name, "zero");
    devfs_devices[1].type = DEV_TYPE_ZERO;
    
    devfs_devices[2].used = 1;
    str_cpy(devfs_devices[2].name, "console");
    devfs_devices[2].type = DEV_TYPE_CONSOLE;
    
    devfs_devices[3].used = 1;
    str_cpy(devfs_devices[3].name, "tty");
    devfs_devices[3].type = DEV_TYPE_TTY;
    
    devfs_device_count = 4;
    
    serial_puts(SERIAL_COM1, "DevFS: mounted at '");
    serial_puts(SERIAL_COM1, path);
    serial_puts(SERIAL_COM1, "'\n");
    
    return 0;
}

static struct vfs_file_operations devfs_fops = {
    .open = devfs_open,
    .close = devfs_close,
    .read = devfs_read,
    .write = devfs_write,
    .seek = devfs_seek,
    .readdir = devfs_readdir,
};

static struct vfs_inode_operations devfs_iops = {
    .create = NULL,
    .mkdir = NULL,
    .rmdir = NULL,
    .unlink = NULL,
    .rename = NULL,
    .stat = devfs_stat,
    .chmod = NULL,
    .chown = NULL,
};

static struct vfs_sb_operations devfs_sops = {
    .sync = NULL,
    .mount = devfs_mount,
    .unmount = NULL,
};

void devfs_init(void) {
    devfs_fs.name[0] = '\0';
    devfs_fs.fops = &devfs_fops;
    devfs_fs.iops = &devfs_iops;
    devfs_fs.sops = &devfs_sops;
    devfs_fs.fs_data = NULL;
    
    vfs_register_fs("devfs", &devfs_fs);
}
