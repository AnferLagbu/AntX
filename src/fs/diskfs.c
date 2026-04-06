#include "vfs.h"
#include "hvfs.h"
#include "ata.h"
#include "serial.h"
#include "pwid.h"
#include "string.h"

#define DISKFS_MAX_FDS          16
#define DISKFS_MAX_PATH         128

struct diskfs_fd {
    uint32_t fd;
    uint32_t inode_num;
    uint64_t offset;
    int flags;
    uint64_t pwid;
    uint8_t used;
};

static struct diskfs_fd diskfs_fds[DISKFS_MAX_FDS];
static uint32_t next_fd = 3;
static int diskfs_mounted = 0;

static uint64_t get_time(void) {
    uint64_t tsc;
    __asm__ volatile ("rdtsc" : "=A"(tsc));
    return tsc;
}

static struct diskfs_fd* alloc_diskfs_fd(void) {
    for (int i = 0; i < DISKFS_MAX_FDS; i++) {
        if (!diskfs_fds[i].used) {
            diskfs_fds[i].used = 1;
            diskfs_fds[i].fd = next_fd++;
            return &diskfs_fds[i];
        }
    }
    return NULL;
}

static void free_diskfs_fd(struct diskfs_fd *fdf) {
    if (fdf) {
        fdf->used = 0;
        fdf->fd = 0;
        fdf->inode_num = 0;
        fdf->offset = 0;
        fdf->flags = 0;
        fdf->pwid = 0;
    }
}

static struct diskfs_fd* find_diskfs_fd(uint32_t fd) {
    for (int i = 0; i < DISKFS_MAX_FDS; i++) {
        if (diskfs_fds[i].used && diskfs_fds[i].fd == fd) {
            return &diskfs_fds[i];
        }
    }
    return NULL;
}

static int diskfs_open(struct vfs_file *file, int flags, uint64_t pwid) {
    if (file == NULL) return -1;
    
    struct inode *inode = hvfs_find_inode(file->path);
    
    if (inode == NULL) {
        if (flags & VFS_O_CREAT) {
            int fd = hvfs_open(file->path, HVFS_O_CREAT | HVFS_O_WRONLY, pwid);
            if (fd < 0) return -1;
            
            inode = hvfs_find_inode(file->path);
            if (inode == NULL) return -1;
            
            hvfs_close(fd);
        } else {
            return -1;
        }
    }
    
    if (!hvfs_check_permission(inode, pwid, HVFS_PERM_R)) {
        return -1;
    }
    
    struct diskfs_fd *fdf = alloc_diskfs_fd();
    if (fdf == NULL) return -1;
    
    fdf->inode_num = inode->inode_num;
    fdf->offset = (flags & VFS_O_APPEND) ? inode->size : 0;
    fdf->flags = flags;
    fdf->pwid = pwid;
    
    file->inode_num = inode->inode_num;
    file->offset = fdf->offset;
    file->type = (inode->mode >> 12) & 0xF;
    file->private_data = fdf;
    
    return 0;
}

static int diskfs_close(struct vfs_file *file) {
    if (file == NULL) return -1;
    
    struct diskfs_fd *fdf = (struct diskfs_fd *)file->private_data;
    if (fdf) {
        free_diskfs_fd(fdf);
        file->private_data = NULL;
    }
    
    return 0;
}

static int diskfs_read(struct vfs_file *file, void *buf, uint32_t count) {
    if (file == NULL || buf == NULL) return -1;
    
    struct diskfs_fd *fdf = (struct diskfs_fd *)file->private_data;
    if (fdf == NULL) return -1;
    
    struct inode *inode = hvfs_get_inode(fdf->inode_num);
    if (inode == NULL) return -1;
    
    if (!hvfs_check_permission(inode, fdf->pwid, HVFS_PERM_R)) {
        return -1;
    }
    
    uint32_t bytes_read = 0;
    uint8_t *buffer = (uint8_t *)buf;
    
    while (bytes_read < count && fdf->offset < inode->size) {
        uint32_t block_idx = fdf->offset / HVFS_BLOCK_SIZE;
        uint32_t block_offset = fdf->offset % HVFS_BLOCK_SIZE;
        uint32_t bytes_to_read = HVFS_BLOCK_SIZE - block_offset;
        
        if (bytes_to_read > count - bytes_read) {
            bytes_to_read = count - bytes_read;
        }
        if (bytes_to_read > inode->size - fdf->offset) {
            bytes_to_read = inode->size - fdf->offset;
        }
        
        if (block_idx < 12 && inode->direct_blocks[block_idx] != 0) {
            memcpy(buffer + bytes_read,
                    hvfs_get_inode(0) + inode->direct_blocks[block_idx] * HVFS_BLOCK_SIZE + block_offset,
                    bytes_to_read);
        }
        
        bytes_read += bytes_to_read;
        fdf->offset += bytes_to_read;
    }
    
    file->offset = fdf->offset;
    inode->atime = get_time();
    
    return bytes_read;
}

static int diskfs_write(struct vfs_file *file, const void *buf, uint32_t count) {
    if (file == NULL || buf == NULL) return -1;
    
    struct diskfs_fd *fdf = (struct diskfs_fd *)file->private_data;
    if (fdf == NULL) return -1;
    
    int hvfs_fd = hvfs_open(file->path, HVFS_O_WRONLY, fdf->pwid);
    if (hvfs_fd < 0) return -1;
    
    hvfs_seek(hvfs_fd, fdf->offset, 0);
    
    int result = hvfs_write(hvfs_fd, buf, count);
    
    struct inode *inode = hvfs_get_inode(fdf->inode_num);
    if (inode) {
        fdf->offset = fdf->offset + (result > 0 ? result : 0);
        file->offset = fdf->offset;
    }
    
    hvfs_close(hvfs_fd);
    
    return result;
}

static int diskfs_seek(struct vfs_file *file, int64_t offset, int whence) {
    if (file == NULL) return -1;
    
    struct diskfs_fd *fdf = (struct diskfs_fd *)file->private_data;
    if (fdf == NULL) return -1;
    
    struct inode *inode = hvfs_get_inode(fdf->inode_num);
    if (inode == NULL) return -1;
    
    int64_t new_offset;
    
    switch (whence) {
        case VFS_SEEK_SET:
            new_offset = offset;
            break;
        case VFS_SEEK_CUR:
            new_offset = fdf->offset + offset;
            break;
        case VFS_SEEK_END:
            new_offset = inode->size + offset;
            break;
        default:
            return -1;
    }
    
    if (new_offset < 0) new_offset = 0;
    if (new_offset > inode->size) new_offset = inode->size;
    
    fdf->offset = new_offset;
    file->offset = new_offset;
    
    return new_offset;
}

static int diskfs_readdir(struct vfs_file *file, struct vfs_dirent *entry) {
    if (file == NULL || entry == NULL) return -1;
    
    struct diskfs_fd *fdf = (struct diskfs_fd *)file->private_data;
    if (fdf == NULL) return -1;
    
    struct inode *inode = hvfs_get_inode(fdf->inode_num);
    if (inode == NULL) return -1;
    
    if ((inode->mode & 0xF000) != (HVFS_TYPE_DIR << 12)) {
        return -1;
    }
    
    struct dir_entry hvfs_entry;
    int result = hvfs_readdir(fdf->fd, &hvfs_entry);
    
    if (result <= 0) return result;
    
    entry->inode = hvfs_entry.inode;
    entry->type = hvfs_entry.file_type;
    strcpy(entry->name, hvfs_entry.name);
    
    return 1;
}

static int diskfs_mkdir(struct vfs_file *parent, const char *name, uint64_t pwid) {
    if (parent == NULL || name == NULL) return -1;
    
    char full_path[DISKFS_MAX_PATH];
    strcpy(full_path, parent->path);
    int len = strlen(full_path);
    if (len > 0 && full_path[len - 1] != '/') {
        full_path[len] = '/';
        len++;
    }
    strcpy(full_path + len, name);
    
    return hvfs_mkdir(full_path, pwid);
}

static int diskfs_rmdir(struct vfs_file *parent, const char *name, uint64_t pwid) {
    if (parent == NULL || name == NULL) return -1;
    
    char full_path[DISKFS_MAX_PATH];
    strcpy(full_path, parent->path);
    int len = strlen(full_path);
    if (len > 0 && full_path[len - 1] != '/') {
        full_path[len] = '/';
        len++;
    }
    strcpy(full_path + len, name);
    
    return hvfs_rmdir(full_path, pwid);
}

static int diskfs_unlink(struct vfs_file *parent, const char *name, uint64_t pwid) {
    if (parent == NULL || name == NULL) return -1;
    
    char full_path[DISKFS_MAX_PATH];
    strcpy(full_path, parent->path);
    int len = strlen(full_path);
    if (len > 0 && full_path[len - 1] != '/') {
        full_path[len] = '/';
        len++;
    }
    strcpy(full_path + len, name);
    
    return hvfs_unlink(full_path, pwid);
}

static int diskfs_rename(struct vfs_file *old_parent, const char *old_name,
                         struct vfs_file *new_parent, const char *new_name, uint64_t pwid) {
    if (old_parent == NULL || new_parent == NULL) return -1;
    
    char old_path[DISKFS_MAX_PATH];
    strcpy(old_path, old_parent->path);
    int old_len = strlen(old_path);
    if (old_len > 0 && old_path[old_len - 1] != '/') {
        old_path[old_len] = '/';
        old_len++;
    }
    strcpy(old_path + old_len, old_name);
    
    char new_path[DISKFS_MAX_PATH];
    strcpy(new_path, new_parent->path);
    int new_len = strlen(new_path);
    if (new_len > 0 && new_path[new_len - 1] != '/') {
        new_path[new_len] = '/';
        new_len++;
    }
    strcpy(new_path + new_len, new_name);
    
    return hvfs_rename(old_path, new_path, pwid);
}

static int diskfs_stat(struct vfs_file *file, struct vfs_stat *st) {
    if (file == NULL || st == NULL) return -1;
    
    struct inode *inode = hvfs_find_inode(file->path);
    if (inode == NULL) return -1;
    
    st->inode_num = inode->inode_num;
    st->mode = inode->pwid_perm;
    st->size = inode->size;
    st->atime = inode->atime;
    st->mtime = inode->mtime;
    st->ctime = inode->ctime;
    st->owner_pwid = inode->owner_pwid;
    st->perm = inode->pwid_perm;
    st->type = (inode->mode >> 12) & 0xF;
    
    return 0;
}

static int diskfs_chmod(struct vfs_file *file, uint16_t mode, uint64_t pwid) {
    if (file == NULL) return -1;
    return hvfs_chmod(file->path, mode, pwid);
}

static int diskfs_chown(struct vfs_file *file, uint64_t owner_pwid, uint64_t pwid) {
    if (file == NULL) return -1;
    return hvfs_chown(file->path, owner_pwid, pwid);
}

static int diskfs_sync(void) {
    return hvfs_sync();
}

static int diskfs_mount(const char *path) {
    if (diskfs_mounted) {
        serial_puts(SERIAL_COM1, "DiskFS: already mounted\n");
        return 0;
    }
    
    int status = hvfs_check_disk();
    
    switch (status) {
        case HVFS_DISK_OK:
            serial_puts(SERIAL_COM1, "DiskFS: Found valid disk filesystem\n");
            if (hvfs_mount() != 0) {
                return -1;
            }
            break;
            
        case HVFS_DISK_NO_DISK:
            serial_puts(SERIAL_COM1, "DiskFS: No disk detected\n");
            return -1;
            
        case HVFS_DISK_UNFORMATTED:
            serial_puts(SERIAL_COM1, "DiskFS: Disk unformatted, formatting...\n");
            if (hvfs_format_disk() != 0) {
                return -1;
            }
            hvfs_format();
            hvfs_sync();
            break;
            
        default:
            serial_puts(SERIAL_COM1, "DiskFS: Unknown disk status\n");
            return -1;
    }
    
    diskfs_mounted = 1;
    
    serial_puts(SERIAL_COM1, "DiskFS: mounted at '");
    serial_puts(SERIAL_COM1, path);
    serial_puts(SERIAL_COM1, "'\n");
    
    return 0;
}

static int diskfs_unmount(void) {
    if (!diskfs_mounted) return 0;
    
    hvfs_sync();
    diskfs_mounted = 0;
    
    serial_puts(SERIAL_COM1, "DiskFS: unmounted\n");
    return 0;
}

static struct vfs_file_operations diskfs_fops = {
    .open = diskfs_open,
    .close = diskfs_close,
    .read = diskfs_read,
    .write = diskfs_write,
    .seek = diskfs_seek,
    .readdir = diskfs_readdir,
};

static struct vfs_inode_operations diskfs_iops = {
    .create = NULL,
    .mkdir = diskfs_mkdir,
    .rmdir = diskfs_rmdir,
    .unlink = diskfs_unlink,
    .rename = diskfs_rename,
    .stat = diskfs_stat,
    .chmod = diskfs_chmod,
    .chown = diskfs_chown,
};

static struct vfs_sb_operations diskfs_sops = {
    .sync = diskfs_sync,
    .mount = diskfs_mount,
    .unmount = diskfs_unmount,
};

static struct vfs_filesystem diskfs_fs;

void diskfs_init(void) {
    for (int i = 0; i < DISKFS_MAX_FDS; i++) {
        diskfs_fds[i].used = 0;
    }
    
    diskfs_fs.name[0] = '\0';
    diskfs_fs.fops = &diskfs_fops;
    diskfs_fs.iops = &diskfs_iops;
    diskfs_fs.sops = &diskfs_sops;
    diskfs_fs.fs_data = NULL;
    
    vfs_register_fs("diskfs", &diskfs_fs);
}
