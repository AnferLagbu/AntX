#include "vfs.h"
#include "serial.h"
#include "string.h"

static struct vfs_mount mount_table[VFS_MAX_MOUNTS];
static struct vfs_filesystem *fs_registry[VFS_MAX_MOUNTS];
static int fs_registry_count = 0;

struct vfs_file vfs_fd_table[VFS_MAX_FDS];

static char current_cwd[VFS_MAX_PATH] = "/";
static uint32_t current_cwd_inode = 1;
static uint32_t next_fd = 3;

static int str_starts_with(const char *str, const char *prefix) {
    while (*prefix) {
        if (*str != *prefix) return 0;
        str++;
        prefix++;
    }
    return 1;
}

static int path_depth(const char *path) {
    int depth = 0;
    while (*path) {
        if (*path == '/') depth++;
        path++;
    }
    return depth;
}

void vfs_init(void) {
    for (int i = 0; i < VFS_MAX_MOUNTS; i++) {
        mount_table[i].used = 0;
        mount_table[i].path[0] = '\0';
        mount_table[i].fs = NULL;
    }
    
    for (int i = 0; i < VFS_MAX_FDS; i++) {
        vfs_fd_table[i].used = 0;
        vfs_fd_table[i].fd = 0;
        vfs_fd_table[i].inode_num = 0;
        vfs_fd_table[i].offset = 0;
        vfs_fd_table[i].flags = 0;
        vfs_fd_table[i].pwid = 0;
        vfs_fd_table[i].fops = NULL;
        vfs_fd_table[i].fs_data = NULL;
        vfs_fd_table[i].private_data = NULL;
    }
    
    fs_registry_count = 0;
    current_cwd[0] = '/';
    current_cwd[1] = '\0';
    current_cwd_inode = 1;
    next_fd = 3;
    
    serial_puts(SERIAL_COM1, "VFS: initialized\n");
}

int vfs_register_fs(const char *name, struct vfs_filesystem *fs) {
    if (fs_registry_count >= VFS_MAX_MOUNTS) {
        return -1;
    }
    
    for (int i = 0; i < fs_registry_count; i++) {
        if (strcmp(fs_registry[i]->name, name) == 0) {
            return -1;
        }
    }
    
    strcpy(fs->name, name);
    fs_registry[fs_registry_count++] = fs;
    
    serial_puts(SERIAL_COM1, "VFS: registered filesystem '");
    serial_puts(SERIAL_COM1, name);
    serial_puts(SERIAL_COM1, "'\n");
    
    return 0;
}

int vfs_unregister_fs(const char *name) {
    for (int i = 0; i < fs_registry_count; i++) {
        if (strcmp(fs_registry[i]->name, name) == 0) {
            for (int j = i; j < fs_registry_count - 1; j++) {
                fs_registry[j] = fs_registry[j + 1];
            }
            fs_registry_count--;
            return 0;
        }
    }
    return -1;
}

struct vfs_filesystem* vfs_get_fs(const char *name) {
    for (int i = 0; i < fs_registry_count; i++) {
        if (strcmp(fs_registry[i]->name, name) == 0) {
            return fs_registry[i];
        }
    }
    return NULL;
}

struct vfs_mount* vfs_find_mount(const char *path) {
    struct vfs_mount *best_match = NULL;
    int best_depth = -1;
    
    for (int i = 0; i < VFS_MAX_MOUNTS; i++) {
        if (!mount_table[i].used) continue;
        
        if (str_starts_with(path, mount_table[i].path)) {
            int depth = path_depth(mount_table[i].path);
            if (depth > best_depth) {
                best_depth = depth;
                best_match = &mount_table[i];
            }
        }
    }
    
    return best_match;
}

const char* vfs_get_relative_path(const char *path, struct vfs_mount *mount) {
    if (mount == NULL) return path;
    
    int mount_len = strlen(mount->path);
    const char *rel_path = path + mount_len;
    
    while (*rel_path == '/') rel_path++;
    
    if (*rel_path == '\0') return "/";
    
    return rel_path;
}

int vfs_mount(const char *path, const char *fs_name) {
    struct vfs_filesystem *fs = vfs_get_fs(fs_name);
    if (fs == NULL) {
        serial_puts(SERIAL_COM1, "VFS: filesystem '");
        serial_puts(SERIAL_COM1, fs_name);
        serial_puts(SERIAL_COM1, "' not found\n");
        return -1;
    }
    
    for (int i = 0; i < VFS_MAX_MOUNTS; i++) {
        if (!mount_table[i].used) {
            strcpy(mount_table[i].path, path);
            mount_table[i].fs = fs;
            mount_table[i].used = 1;
            
            if (fs->sops && fs->sops->mount) {
                fs->sops->mount(path);
            }
            
            serial_puts(SERIAL_COM1, "VFS: mounted '");
            serial_puts(SERIAL_COM1, fs_name);
            serial_puts(SERIAL_COM1, "' at '");
            serial_puts(SERIAL_COM1, path);
            serial_puts(SERIAL_COM1, "'\n");
            
            return 0;
        }
    }
    
    return -1;
}

int vfs_unmount(const char *path) {
    for (int i = 0; i < VFS_MAX_MOUNTS; i++) {
        if (mount_table[i].used && strcmp(mount_table[i].path, path) == 0) {
            if (mount_table[i].fs && mount_table[i].fs->sops && 
                mount_table[i].fs->sops->unmount) {
                mount_table[i].fs->sops->unmount();
            }
            
            mount_table[i].used = 0;
            mount_table[i].path[0] = '\0';
            mount_table[i].fs = NULL;
            
            serial_puts(SERIAL_COM1, "VFS: unmounted '");
            serial_puts(SERIAL_COM1, path);
            serial_puts(SERIAL_COM1, "'\n");
            
            return 0;
        }
    }
    return -1;
}

static struct vfs_file* alloc_fd(void) {
    for (int i = 0; i < VFS_MAX_FDS; i++) {
        if (!vfs_fd_table[i].used) {
            vfs_fd_table[i].used = 1;
            vfs_fd_table[i].fd = next_fd++;
            return &vfs_fd_table[i];
        }
    }
    return NULL;
}

static void free_fd(struct vfs_file *file) {
    if (file) {
        file->used = 0;
        file->fd = 0;
        file->inode_num = 0;
        file->offset = 0;
        file->flags = 0;
        file->pwid = 0;
        file->fops = NULL;
        file->fs_data = NULL;
        file->private_data = NULL;
    }
}

struct vfs_file* vfs_open(const char *path, int flags, uint64_t pwid) {
    struct vfs_mount *mount = vfs_find_mount(path);
    if (mount == NULL || mount->fs == NULL) {
        serial_puts(SERIAL_COM1, "VFS: no mount point for '");
        serial_puts(SERIAL_COM1, path);
        serial_puts(SERIAL_COM1, "'\n");
        return NULL;
    }
    
    struct vfs_file *file = alloc_fd();
    if (file == NULL) {
        return NULL;
    }
    
    const char *rel_path = vfs_get_relative_path(path, mount);
    
    strcpy(file->path, rel_path);
    file->flags = flags;
    file->pwid = pwid;
    file->fs_data = mount->fs->fs_data;
    file->fops = mount->fs->fops;
    
    if (file->fops && file->fops->open) {
        if (file->fops->open(file, flags, pwid) != 0) {
            free_fd(file);
            return NULL;
        }
    }
    
    return file;
}

int vfs_close(struct vfs_file *file) {
    if (file == NULL || !file->used) {
        return -1;
    }
    
    int result = 0;
    if (file->fops && file->fops->close) {
        result = file->fops->close(file);
    }
    
    free_fd(file);
    return result;
}

int vfs_read(struct vfs_file *file, void *buf, uint32_t count) {
    if (file == NULL || !file->used || buf == NULL) {
        return -1;
    }
    
    if (file->fops && file->fops->read) {
        return file->fops->read(file, buf, count);
    }
    
    return -1;
}

int vfs_write(struct vfs_file *file, const void *buf, uint32_t count) {
    if (file == NULL || !file->used || buf == NULL) {
        return -1;
    }
    
    if (file->fops && file->fops->write) {
        return file->fops->write(file, buf, count);
    }
    
    return -1;
}

int vfs_seek(struct vfs_file *file, int64_t offset, int whence) {
    if (file == NULL || !file->used) {
        return -1;
    }
    
    if (file->fops && file->fops->seek) {
        return file->fops->seek(file, offset, whence);
    }
    
    return -1;
}

int vfs_readdir(struct vfs_file *file, struct vfs_dirent *entry) {
    if (file == NULL || !file->used || entry == NULL) {
        return -1;
    }
    
    if (file->fops && file->fops->readdir) {
        return file->fops->readdir(file, entry);
    }
    
    return -1;
}

int vfs_mkdir(const char *path, uint64_t pwid) {
    struct vfs_mount *mount = vfs_find_mount(path);
    if (mount == NULL || mount->fs == NULL) {
        return -1;
    }
    
    if (mount->fs->iops && mount->fs->iops->mkdir) {
        const char *rel_path = vfs_get_relative_path(path, mount);
        
        const char *last_slash = rel_path;
        for (const char *p = rel_path; *p; p++) {
            if (*p == '/') last_slash = p + 1;
        }
        
        char parent_path[VFS_MAX_PATH];
        int parent_len = last_slash - rel_path;
        if (parent_len == 0) {
            parent_path[0] = '/';
            parent_path[1] = '\0';
        } else {
            memcpy(parent_path, rel_path, parent_len);
            parent_path[parent_len] = '\0';
        }
        
        struct vfs_file parent_file;
        parent_file.path[0] = '\0';
        strcpy(parent_file.path, parent_path);
        parent_file.fs_data = mount->fs->fs_data;
        parent_file.fops = mount->fs->fops;
        parent_file.pwid = pwid;
        
        if (parent_file.fops && parent_file.fops->open) {
            parent_file.fops->open(&parent_file, VFS_O_RDONLY, pwid);
        }
        
        int result = mount->fs->iops->mkdir(&parent_file, last_slash, pwid);
        
        if (parent_file.fops && parent_file.fops->close) {
            parent_file.fops->close(&parent_file);
        }
        
        return result;
    }
    
    return -1;
}

int vfs_rmdir(const char *path, uint64_t pwid) {
    struct vfs_mount *mount = vfs_find_mount(path);
    if (mount == NULL || mount->fs == NULL) {
        return -1;
    }
    
    if (mount->fs->iops && mount->fs->iops->rmdir) {
        const char *rel_path = vfs_get_relative_path(path, mount);
        
        const char *last_slash = rel_path;
        for (const char *p = rel_path; *p; p++) {
            if (*p == '/') last_slash = p + 1;
        }
        
        char parent_path[VFS_MAX_PATH];
        int parent_len = last_slash - rel_path;
        if (parent_len == 0) {
            parent_path[0] = '/';
            parent_path[1] = '\0';
        } else {
            memcpy(parent_path, rel_path, parent_len);
            parent_path[parent_len] = '\0';
        }
        
        struct vfs_file parent_file;
        strcpy(parent_file.path, parent_path);
        parent_file.fs_data = mount->fs->fs_data;
        parent_file.fops = mount->fs->fops;
        parent_file.pwid = pwid;
        
        if (parent_file.fops && parent_file.fops->open) {
            parent_file.fops->open(&parent_file, VFS_O_RDONLY, pwid);
        }
        
        int result = mount->fs->iops->rmdir(&parent_file, last_slash, pwid);
        
        if (parent_file.fops && parent_file.fops->close) {
            parent_file.fops->close(&parent_file);
        }
        
        return result;
    }
    
    return -1;
}

int vfs_unlink(const char *path, uint64_t pwid) {
    struct vfs_mount *mount = vfs_find_mount(path);
    if (mount == NULL || mount->fs == NULL) {
        return -1;
    }
    
    if (mount->fs->iops && mount->fs->iops->unlink) {
        const char *rel_path = vfs_get_relative_path(path, mount);
        
        const char *last_slash = rel_path;
        for (const char *p = rel_path; *p; p++) {
            if (*p == '/') last_slash = p + 1;
        }
        
        char parent_path[VFS_MAX_PATH];
        int parent_len = last_slash - rel_path;
        if (parent_len == 0) {
            parent_path[0] = '/';
            parent_path[1] = '\0';
        } else {
            memcpy(parent_path, rel_path, parent_len);
            parent_path[parent_len] = '\0';
        }
        
        struct vfs_file parent_file;
        strcpy(parent_file.path, parent_path);
        parent_file.fs_data = mount->fs->fs_data;
        parent_file.fops = mount->fs->fops;
        parent_file.pwid = pwid;
        
        if (parent_file.fops && parent_file.fops->open) {
            parent_file.fops->open(&parent_file, VFS_O_RDONLY, pwid);
        }
        
        int result = mount->fs->iops->unlink(&parent_file, last_slash, pwid);
        
        if (parent_file.fops && parent_file.fops->close) {
            parent_file.fops->close(&parent_file);
        }
        
        return result;
    }
    
    return -1;
}

int vfs_rename(const char *old_path, const char *new_path, uint64_t pwid) {
    struct vfs_mount *old_mount = vfs_find_mount(old_path);
    struct vfs_mount *new_mount = vfs_find_mount(new_path);
    
    if (old_mount == NULL || new_mount == NULL) {
        return -1;
    }
    
    if (old_mount != new_mount) {
        serial_puts(SERIAL_COM1, "VFS: cross-filesystem rename not supported\n");
        return -1;
    }
    
    if (old_mount->fs->iops && old_mount->fs->iops->rename) {
        const char *old_rel = vfs_get_relative_path(old_path, old_mount);
        const char *new_rel = vfs_get_relative_path(new_path, new_mount);
        
        const char *old_last_slash = old_rel;
        for (const char *p = old_rel; *p; p++) {
            if (*p == '/') old_last_slash = p + 1;
        }
        
        const char *new_last_slash = new_rel;
        for (const char *p = new_rel; *p; p++) {
            if (*p == '/') new_last_slash = p + 1;
        }
        
        char old_parent_path[VFS_MAX_PATH];
        int old_parent_len = old_last_slash - old_rel;
        if (old_parent_len == 0) {
            old_parent_path[0] = '/';
            old_parent_path[1] = '\0';
        } else {
            memcpy(old_parent_path, old_rel, old_parent_len);
            old_parent_path[old_parent_len] = '\0';
        }
        
        char new_parent_path[VFS_MAX_PATH];
        int new_parent_len = new_last_slash - new_rel;
        if (new_parent_len == 0) {
            new_parent_path[0] = '/';
            new_parent_path[1] = '\0';
        } else {
            memcpy(new_parent_path, new_rel, new_parent_len);
            new_parent_path[new_parent_len] = '\0';
        }
        
        struct vfs_file old_parent_file;
        strcpy(old_parent_file.path, old_parent_path);
        old_parent_file.fs_data = old_mount->fs->fs_data;
        old_parent_file.fops = old_mount->fs->fops;
        old_parent_file.pwid = pwid;
        
        struct vfs_file new_parent_file;
        strcpy(new_parent_file.path, new_parent_path);
        new_parent_file.fs_data = new_mount->fs->fs_data;
        new_parent_file.fops = new_mount->fs->fops;
        new_parent_file.pwid = pwid;
        
        if (old_parent_file.fops && old_parent_file.fops->open) {
            old_parent_file.fops->open(&old_parent_file, VFS_O_RDONLY, pwid);
        }
        if (new_parent_file.fops && new_parent_file.fops->open) {
            new_parent_file.fops->open(&new_parent_file, VFS_O_RDONLY, pwid);
        }
        
        int result = old_mount->fs->iops->rename(&old_parent_file, old_last_slash,
                                                  &new_parent_file, new_last_slash, pwid);
        
        if (old_parent_file.fops && old_parent_file.fops->close) {
            old_parent_file.fops->close(&old_parent_file);
        }
        if (new_parent_file.fops && new_parent_file.fops->close) {
            new_parent_file.fops->close(&new_parent_file);
        }
        
        return result;
    }
    
    return -1;
}

int vfs_stat(const char *path, struct vfs_stat *st, uint64_t pwid) {
    struct vfs_mount *mount = vfs_find_mount(path);
    if (mount == NULL || mount->fs == NULL || st == NULL) {
        return -1;
    }
    
    struct vfs_file file;
    const char *rel_path = vfs_get_relative_path(path, mount);
    strcpy(file.path, rel_path);
    file.fs_data = mount->fs->fs_data;
    file.fops = mount->fs->fops;
    file.pwid = pwid;
    
    if (file.fops && file.fops->open) {
        if (file.fops->open(&file, VFS_O_RDONLY, pwid) != 0) {
            return -1;
        }
    }
    
    int result = -1;
    if (mount->fs->iops && mount->fs->iops->stat) {
        result = mount->fs->iops->stat(&file, st);
    }
    
    if (file.fops && file.fops->close) {
        file.fops->close(&file);
    }
    
    return result;
}

int vfs_chmod(const char *path, uint16_t mode, uint64_t pwid) {
    struct vfs_mount *mount = vfs_find_mount(path);
    if (mount == NULL || mount->fs == NULL) {
        return -1;
    }
    
    struct vfs_file file;
    const char *rel_path = vfs_get_relative_path(path, mount);
    strcpy(file.path, rel_path);
    file.fs_data = mount->fs->fs_data;
    file.fops = mount->fs->fops;
    file.pwid = pwid;
    
    if (file.fops && file.fops->open) {
        if (file.fops->open(&file, VFS_O_RDONLY, pwid) != 0) {
            return -1;
        }
    }
    
    int result = -1;
    if (mount->fs->iops && mount->fs->iops->chmod) {
        result = mount->fs->iops->chmod(&file, mode, pwid);
    }
    
    if (file.fops && file.fops->close) {
        file.fops->close(&file);
    }
    
    return result;
}

int vfs_chown(const char *path, uint64_t owner_pwid, uint64_t pwid) {
    struct vfs_mount *mount = vfs_find_mount(path);
    if (mount == NULL || mount->fs == NULL) {
        return -1;
    }
    
    struct vfs_file file;
    const char *rel_path = vfs_get_relative_path(path, mount);
    strcpy(file.path, rel_path);
    file.fs_data = mount->fs->fs_data;
    file.fops = mount->fs->fops;
    file.pwid = pwid;
    
    if (file.fops && file.fops->open) {
        if (file.fops->open(&file, VFS_O_RDONLY, pwid) != 0) {
            return -1;
        }
    }
    
    int result = -1;
    if (mount->fs->iops && mount->fs->iops->chown) {
        result = mount->fs->iops->chown(&file, owner_pwid, pwid);
    }
    
    if (file.fops && file.fops->close) {
        file.fops->close(&file);
    }
    
    return result;
}

int vfs_sync(void) {
    int result = 0;
    
    for (int i = 0; i < VFS_MAX_MOUNTS; i++) {
        if (mount_table[i].used && mount_table[i].fs) {
            if (mount_table[i].fs->sops && mount_table[i].fs->sops->sync) {
                if (mount_table[i].fs->sops->sync() != 0) {
                    result = -1;
                }
            }
        }
    }
    
    return result;
}

void vfs_set_cwd(const char *path) {
    strcpy(current_cwd, path);
}

const char* vfs_get_cwd(void) {
    return current_cwd;
}

uint32_t vfs_get_cwd_inode(void) {
    return current_cwd_inode;
}
