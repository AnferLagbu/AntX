#include "vfs.h"
#include "serial.h"
#include "pwid.h"

#define RAMFS_MAX_INODES    64
#define RAMFS_MAX_BLOCKS    256
#define RAMFS_BLOCK_SIZE    512
#define RAMFS_MAX_NAME      64

struct ramfs_inode {
    uint32_t inode_num;
    uint8_t  type;
    uint16_t perm;
    uint32_t size;
    uint64_t owner_pwid;
    uint64_t atime;
    uint64_t mtime;
    uint64_t ctime;
    uint32_t direct_blocks[8];
    uint32_t link_count;
    uint8_t  used;
    uint8_t  dirty;
};

struct ramfs_dirent {
    uint32_t inode;
    uint8_t  type;
    char     name[RAMFS_MAX_NAME];
};

struct ramfs_data {
    struct ramfs_inode inodes[RAMFS_MAX_INODES];
    uint8_t data_area[RAMFS_MAX_BLOCKS * RAMFS_BLOCK_SIZE];
    uint8_t inode_bitmap[RAMFS_MAX_INODES / 8];
    uint8_t block_bitmap[RAMFS_MAX_BLOCKS / 8];
    uint32_t root_inode;
    uint32_t free_inodes;
    uint32_t free_blocks;
};

static struct ramfs_data ramfs_data;
static struct vfs_filesystem ramfs_fs;

static uint64_t get_time(void) {
    uint64_t tsc;
    __asm__ volatile ("rdtsc" : "=A"(tsc));
    return tsc;
}

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

static void mem_cpy(void *dest, const void *src, uint32_t count) {
    uint8_t *d = (uint8_t *)dest;
    const uint8_t *s = (const uint8_t *)src;
    for (uint32_t i = 0; i < count; i++) {
        d[i] = s[i];
    }
}

static uint8_t* get_block(uint32_t block_num) {
    if (block_num >= RAMFS_MAX_BLOCKS) return NULL;
    return &ramfs_data.data_area[block_num * RAMFS_BLOCK_SIZE];
}

static int block_is_free(uint32_t block_num) {
    if (block_num >= RAMFS_MAX_BLOCKS) return 0;
    uint32_t byte_idx = block_num / 8;
    uint32_t bit_idx = block_num % 8;
    return !(ramfs_data.block_bitmap[byte_idx] & (1 << bit_idx));
}

static void block_set_used(uint32_t block_num) {
    if (block_num >= RAMFS_MAX_BLOCKS) return;
    uint32_t byte_idx = block_num / 8;
    uint32_t bit_idx = block_num % 8;
    ramfs_data.block_bitmap[byte_idx] |= (1 << bit_idx);
    ramfs_data.free_blocks--;
}

static void block_set_free(uint32_t block_num) {
    if (block_num >= RAMFS_MAX_BLOCKS) return;
    uint32_t byte_idx = block_num / 8;
    uint32_t bit_idx = block_num % 8;
    ramfs_data.block_bitmap[byte_idx] &= ~(1 << bit_idx);
    ramfs_data.free_blocks++;
}

static uint32_t block_alloc(void) {
    for (uint32_t i = 0; i < RAMFS_MAX_BLOCKS; i++) {
        if (block_is_free(i)) {
            block_set_used(i);
            mem_set(get_block(i), 0, RAMFS_BLOCK_SIZE);
            return i;
        }
    }
    return 0;
}

static int inode_is_free(uint32_t inode_num) {
    if (inode_num >= RAMFS_MAX_INODES) return 0;
    uint32_t byte_idx = inode_num / 8;
    uint32_t bit_idx = inode_num % 8;
    return !(ramfs_data.inode_bitmap[byte_idx] & (1 << bit_idx));
}

static void inode_set_used(uint32_t inode_num) {
    if (inode_num >= RAMFS_MAX_INODES) return;
    uint32_t byte_idx = inode_num / 8;
    uint32_t bit_idx = inode_num % 8;
    ramfs_data.inode_bitmap[byte_idx] |= (1 << bit_idx);
    ramfs_data.free_inodes--;
}

static struct ramfs_inode* inode_alloc(void) {
    for (int i = 1; i < RAMFS_MAX_INODES; i++) {
        if (!ramfs_data.inodes[i].used) {
            ramfs_data.inodes[i].used = 1;
            ramfs_data.inodes[i].inode_num = i;
            ramfs_data.inodes[i].link_count = 1;
            ramfs_data.inodes[i].dirty = 0;
            inode_set_used(i);
            return &ramfs_data.inodes[i];
        }
    }
    return NULL;
}

static struct ramfs_inode* get_inode(uint32_t inode_num) {
    if (inode_num == 0 || inode_num >= RAMFS_MAX_INODES) return NULL;
    if (!ramfs_data.inodes[inode_num].used) return NULL;
    return &ramfs_data.inodes[inode_num];
}

static int check_permission(struct ramfs_inode *inode, uint64_t pwid, int access_type) {
    if (inode == NULL) return 0;
    
    uint8_t level = pwid_get_level(pwid);
    
    if (level == PWID_LEVEL_ROOT) {
        return 1;
    }
    
    if (pwid == inode->owner_pwid) {
        uint16_t owner_perm = (inode->perm >> 6) & 0x07;
        return (owner_perm & access_type) == access_type;
    }
    
    uint16_t other_perm = inode->perm & 0x07;
    return (other_perm & access_type) == access_type;
}

static struct ramfs_inode* resolve_path(const char *path) {
    struct ramfs_inode *current = get_inode(ramfs_data.root_inode);
    const char *p = path;
    
    if (*p == '/') p++;
    
    if (*p == '\0') return current;
    
    while (*p && current) {
        while (*p == '/') p++;
        if (*p == '\0') break;
        
        if (current->type != VFS_TYPE_DIR) {
            return NULL;
        }
        
        char name[RAMFS_MAX_NAME];
        int name_len = 0;
        while (*p && *p != '/' && name_len < RAMFS_MAX_NAME - 1) {
            name[name_len++] = *p++;
        }
        name[name_len] = '\0';
        
        int found = 0;
        struct ramfs_dirent *entries = (struct ramfs_dirent *)get_block(current->direct_blocks[0]);
        int num_entries = current->size / sizeof(struct ramfs_dirent);
        
        for (int i = 0; i < num_entries; i++) {
            if (entries[i].inode != 0 && str_cmp(entries[i].name, name) == 0) {
                current = get_inode(entries[i].inode);
                found = 1;
                break;
            }
        }
        
        if (!found) return NULL;
    }
    
    return current;
}

static int ramfs_open(struct vfs_file *file, int flags, uint64_t pwid) {
    if (file == NULL) return -1;
    
    struct ramfs_inode *inode = resolve_path(file->path);
    
    if (inode == NULL) {
        if (flags & VFS_O_CREAT) {
            const char *filename = file->path;
            const char *last_slash = file->path;
            for (const char *p = file->path; *p; p++) {
                if (*p == '/') last_slash = p + 1;
            }
            filename = last_slash;
            
            char dir_path[VFS_MAX_PATH];
            int dir_len = last_slash - file->path;
            if (dir_len == 0) {
                dir_path[0] = '/';
                dir_path[1] = '\0';
            } else {
                mem_cpy(dir_path, file->path, dir_len);
                dir_path[dir_len] = '\0';
            }
            
            struct ramfs_inode *parent = resolve_path(dir_path);
            if (parent == NULL) return -1;
            
            if (!check_permission(parent, pwid, VFS_PERM_W)) {
                return -1;
            }
            
            inode = inode_alloc();
            if (inode == NULL) return -1;
            
            inode->type = VFS_TYPE_FILE;
            inode->perm = 0644;
            inode->size = 0;
            inode->owner_pwid = pwid;
            inode->atime = get_time();
            inode->mtime = get_time();
            inode->ctime = get_time();
            inode->direct_blocks[0] = block_alloc();
            
            struct ramfs_dirent *entries = (struct ramfs_dirent *)get_block(parent->direct_blocks[0]);
            int num_entries = parent->size / sizeof(struct ramfs_dirent);
            
            entries[num_entries].inode = inode->inode_num;
            entries[num_entries].type = VFS_TYPE_FILE;
            str_cpy(entries[num_entries].name, filename);
            
            parent->size += sizeof(struct ramfs_dirent);
            parent->mtime = get_time();
        } else {
            return -1;
        }
    }
    
    if (!check_permission(inode, pwid, VFS_PERM_R)) {
        return -1;
    }
    
    file->inode_num = inode->inode_num;
    file->offset = (flags & VFS_O_APPEND) ? inode->size : 0;
    file->type = inode->type;
    file->fs_data = &ramfs_data;
    file->private_data = inode;
    
    return 0;
}

static int ramfs_close(struct vfs_file *file) {
    if (file == NULL) return -1;
    file->private_data = NULL;
    return 0;
}

static int ramfs_read(struct vfs_file *file, void *buf, uint32_t count) {
    if (file == NULL || buf == NULL) return -1;
    
    struct ramfs_inode *inode = (struct ramfs_inode *)file->private_data;
    if (inode == NULL) return -1;
    
    if (!check_permission(inode, file->pwid, VFS_PERM_R)) {
        return -1;
    }
    
    uint32_t bytes_read = 0;
    uint8_t *buffer = (uint8_t *)buf;
    
    while (bytes_read < count && file->offset < inode->size) {
        uint32_t block_idx = file->offset / RAMFS_BLOCK_SIZE;
        uint32_t block_offset = file->offset % RAMFS_BLOCK_SIZE;
        uint32_t bytes_to_read = RAMFS_BLOCK_SIZE - block_offset;
        
        if (bytes_to_read > count - bytes_read) {
            bytes_to_read = count - bytes_read;
        }
        if (bytes_to_read > inode->size - file->offset) {
            bytes_to_read = inode->size - file->offset;
        }
        
        if (block_idx < 8 && inode->direct_blocks[block_idx] != 0) {
            mem_cpy(buffer + bytes_read,
                    get_block(inode->direct_blocks[block_idx]) + block_offset,
                    bytes_to_read);
        }
        
        bytes_read += bytes_to_read;
        file->offset += bytes_to_read;
    }
    
    inode->atime = get_time();
    return bytes_read;
}

static int ramfs_write(struct vfs_file *file, const void *buf, uint32_t count) {
    if (file == NULL || buf == NULL) return -1;
    
    struct ramfs_inode *inode = (struct ramfs_inode *)file->private_data;
    if (inode == NULL) return -1;
    
    if (!check_permission(inode, file->pwid, VFS_PERM_W)) {
        return -1;
    }
    
    uint32_t bytes_written = 0;
    const uint8_t *buffer = (const uint8_t *)buf;
    
    while (bytes_written < count) {
        uint32_t block_idx = file->offset / RAMFS_BLOCK_SIZE;
        uint32_t block_offset = file->offset % RAMFS_BLOCK_SIZE;
        uint32_t bytes_to_write = RAMFS_BLOCK_SIZE - block_offset;
        
        if (bytes_to_write > count - bytes_written) {
            bytes_to_write = count - bytes_written;
        }
        
        if (block_idx >= 8) break;
        
        if (inode->direct_blocks[block_idx] == 0) {
            inode->direct_blocks[block_idx] = block_alloc();
            if (inode->direct_blocks[block_idx] == 0) break;
        }
        
        mem_cpy(get_block(inode->direct_blocks[block_idx]) + block_offset,
                buffer + bytes_written, bytes_to_write);
        
        bytes_written += bytes_to_write;
        file->offset += bytes_to_write;
        
        if (file->offset > inode->size) {
            inode->size = file->offset;
        }
    }
    
    inode->mtime = get_time();
    return bytes_written;
}

static int ramfs_seek(struct vfs_file *file, int64_t offset, int whence) {
    if (file == NULL) return -1;
    
    struct ramfs_inode *inode = (struct ramfs_inode *)file->private_data;
    if (inode == NULL) return -1;
    
    int64_t new_offset;
    
    switch (whence) {
        case VFS_SEEK_SET:
            new_offset = offset;
            break;
        case VFS_SEEK_CUR:
            new_offset = file->offset + offset;
            break;
        case VFS_SEEK_END:
            new_offset = inode->size + offset;
            break;
        default:
            return -1;
    }
    
    if (new_offset < 0) new_offset = 0;
    if (new_offset > inode->size) new_offset = inode->size;
    
    file->offset = new_offset;
    return new_offset;
}

static int ramfs_readdir(struct vfs_file *file, struct vfs_dirent *entry) {
    if (file == NULL || entry == NULL) return -1;
    
    struct ramfs_inode *inode = (struct ramfs_inode *)file->private_data;
    if (inode == NULL || inode->type != VFS_TYPE_DIR) return -1;
    
    struct ramfs_dirent *entries = (struct ramfs_dirent *)get_block(inode->direct_blocks[0]);
    int num_entries = inode->size / sizeof(struct ramfs_dirent);
    
    int entry_idx = file->offset / sizeof(struct ramfs_dirent);
    
    while (entry_idx < num_entries && entries[entry_idx].inode == 0) {
        entry_idx++;
        file->offset += sizeof(struct ramfs_dirent);
    }
    
    if (entry_idx >= num_entries) {
        return 0;
    }
    
    entry->inode = entries[entry_idx].inode;
    entry->type = entries[entry_idx].type;
    str_cpy(entry->name, entries[entry_idx].name);
    
    file->offset += sizeof(struct ramfs_dirent);
    
    return 1;
}

static int ramfs_mkdir(struct vfs_file *parent, const char *name, uint64_t pwid) {
    if (parent == NULL || name == NULL) return -1;
    
    struct ramfs_inode *parent_inode = resolve_path(parent->path);
    if (parent_inode == NULL) return -1;
    
    if (!check_permission(parent_inode, pwid, VFS_PERM_W)) {
        return -1;
    }
    
    struct ramfs_inode *new_dir = inode_alloc();
    if (new_dir == NULL) return -1;
    
    new_dir->type = VFS_TYPE_DIR;
    new_dir->perm = 0755;
    new_dir->size = 2 * sizeof(struct ramfs_dirent);
    new_dir->owner_pwid = pwid;
    new_dir->atime = get_time();
    new_dir->mtime = get_time();
    new_dir->ctime = get_time();
    new_dir->direct_blocks[0] = block_alloc();
    new_dir->link_count = 2;
    
    struct ramfs_dirent *new_entries = (struct ramfs_dirent *)get_block(new_dir->direct_blocks[0]);
    new_entries[0].inode = new_dir->inode_num;
    new_entries[0].type = VFS_TYPE_DIR;
    str_cpy(new_entries[0].name, ".");
    
    new_entries[1].inode = parent_inode->inode_num;
    new_entries[1].type = VFS_TYPE_DIR;
    str_cpy(new_entries[1].name, "..");
    
    struct ramfs_dirent *parent_entries = (struct ramfs_dirent *)get_block(parent_inode->direct_blocks[0]);
    int num_entries = parent_inode->size / sizeof(struct ramfs_dirent);
    
    parent_entries[num_entries].inode = new_dir->inode_num;
    parent_entries[num_entries].type = VFS_TYPE_DIR;
    str_cpy(parent_entries[num_entries].name, name);
    
    parent_inode->size += sizeof(struct ramfs_dirent);
    parent_inode->link_count++;
    parent_inode->mtime = get_time();
    
    serial_puts(SERIAL_COM1, "RamFS: created directory '");
    serial_puts(SERIAL_COM1, name);
    serial_puts(SERIAL_COM1, "'\n");
    
    return 0;
}

static int ramfs_rmdir(struct vfs_file *parent, const char *name, uint64_t pwid) {
    if (parent == NULL || name == NULL) return -1;
    
    struct ramfs_inode *parent_inode = resolve_path(parent->path);
    if (parent_inode == NULL) return -1;
    
    struct ramfs_dirent *entries = (struct ramfs_dirent *)get_block(parent_inode->direct_blocks[0]);
    int num_entries = parent_inode->size / sizeof(struct ramfs_dirent);
    
    for (int i = 0; i < num_entries; i++) {
        if (str_cmp(entries[i].name, name) == 0 && entries[i].inode != 0) {
            struct ramfs_inode *dir = get_inode(entries[i].inode);
            if (dir == NULL || dir->type != VFS_TYPE_DIR) return -1;
            
            if (dir->size > 2 * sizeof(struct ramfs_dirent)) {
                serial_puts(SERIAL_COM1, "RamFS: directory not empty\n");
                return -1;
            }
            
            if (!check_permission(dir, pwid, VFS_PERM_W)) {
                return -1;
            }
            
            entries[i].inode = 0;
            parent_inode->link_count--;
            parent_inode->mtime = get_time();
            
            if (dir->direct_blocks[0] != 0) {
                block_set_free(dir->direct_blocks[0]);
            }
            dir->used = 0;
            
            serial_puts(SERIAL_COM1, "RamFS: removed directory '");
            serial_puts(SERIAL_COM1, name);
            serial_puts(SERIAL_COM1, "'\n");
            
            return 0;
        }
    }
    
    return -1;
}

static int ramfs_unlink(struct vfs_file *parent, const char *name, uint64_t pwid) {
    if (parent == NULL || name == NULL) return -1;
    
    struct ramfs_inode *parent_inode = resolve_path(parent->path);
    if (parent_inode == NULL) return -1;
    
    struct ramfs_dirent *entries = (struct ramfs_dirent *)get_block(parent_inode->direct_blocks[0]);
    int num_entries = parent_inode->size / sizeof(struct ramfs_dirent);
    
    for (int i = 0; i < num_entries; i++) {
        if (str_cmp(entries[i].name, name) == 0 && entries[i].inode != 0) {
            struct ramfs_inode *file = get_inode(entries[i].inode);
            if (file == NULL || file->type == VFS_TYPE_DIR) return -1;
            
            if (!check_permission(file, pwid, VFS_PERM_W)) {
                return -1;
            }
            
            entries[i].inode = 0;
            
            file->link_count--;
            if (file->link_count <= 0) {
                for (int j = 0; j < 8; j++) {
                    if (file->direct_blocks[j] != 0) {
                        block_set_free(file->direct_blocks[j]);
                    }
                }
                file->used = 0;
            }
            
            serial_puts(SERIAL_COM1, "RamFS: removed file '");
            serial_puts(SERIAL_COM1, name);
            serial_puts(SERIAL_COM1, "'\n");
            
            return 0;
        }
    }
    
    return -1;
}

static int ramfs_rename(struct vfs_file *old_parent, const char *old_name,
                        struct vfs_file *new_parent, const char *new_name, uint64_t pwid) {
    if (old_parent == NULL || new_parent == NULL) return -1;
    
    struct ramfs_inode *old_parent_inode = resolve_path(old_parent->path);
    struct ramfs_inode *new_parent_inode = resolve_path(new_parent->path);
    
    if (old_parent_inode == NULL || new_parent_inode == NULL) return -1;
    
    struct ramfs_dirent *old_entries = (struct ramfs_dirent *)get_block(old_parent_inode->direct_blocks[0]);
    int old_num = old_parent_inode->size / sizeof(struct ramfs_dirent);
    
    uint32_t target_inode = 0;
    uint8_t target_type = 0;
    
    for (int i = 0; i < old_num; i++) {
        if (str_cmp(old_entries[i].name, old_name) == 0 && old_entries[i].inode != 0) {
            target_inode = old_entries[i].inode;
            target_type = old_entries[i].type;
            old_entries[i].inode = 0;
            break;
        }
    }
    
    if (target_inode == 0) return -1;
    
    struct ramfs_dirent *new_entries = (struct ramfs_dirent *)get_block(new_parent_inode->direct_blocks[0]);
    int new_num = new_parent_inode->size / sizeof(struct ramfs_dirent);
    
    int insert_pos = -1;
    for (int i = 0; i < new_num; i++) {
        if (new_entries[i].inode == 0) {
            insert_pos = i;
            break;
        }
    }
    
    if (insert_pos == -1) {
        insert_pos = new_num;
        new_parent_inode->size += sizeof(struct ramfs_dirent);
    }
    
    new_entries[insert_pos].inode = target_inode;
    new_entries[insert_pos].type = target_type;
    str_cpy(new_entries[insert_pos].name, new_name);
    
    old_parent_inode->mtime = get_time();
    new_parent_inode->mtime = get_time();
    
    serial_puts(SERIAL_COM1, "RamFS: renamed '");
    serial_puts(SERIAL_COM1, old_name);
    serial_puts(SERIAL_COM1, "' to '");
    serial_puts(SERIAL_COM1, new_name);
    serial_puts(SERIAL_COM1, "'\n");
    
    return 0;
}

static int ramfs_stat(struct vfs_file *file, struct vfs_stat *st) {
    if (file == NULL || st == NULL) return -1;
    
    struct ramfs_inode *inode = resolve_path(file->path);
    if (inode == NULL) return -1;
    
    st->inode_num = inode->inode_num;
    st->mode = inode->perm;
    st->size = inode->size;
    st->atime = inode->atime;
    st->mtime = inode->mtime;
    st->ctime = inode->ctime;
    st->owner_pwid = inode->owner_pwid;
    st->perm = inode->perm;
    st->type = inode->type;
    
    return 0;
}

static int ramfs_chmod(struct vfs_file *file, uint16_t mode, uint64_t pwid) {
    if (file == NULL) return -1;
    
    struct ramfs_inode *inode = resolve_path(file->path);
    if (inode == NULL) return -1;
    
    uint8_t level = pwid_get_level(pwid);
    if (level != PWID_LEVEL_ROOT && inode->owner_pwid != pwid) {
        return -1;
    }
    
    inode->perm = mode;
    inode->mtime = get_time();
    
    return 0;
}

static int ramfs_chown(struct vfs_file *file, uint64_t owner_pwid, uint64_t pwid) {
    if (file == NULL) return -1;
    
    struct ramfs_inode *inode = resolve_path(file->path);
    if (inode == NULL) return -1;
    
    uint8_t level = pwid_get_level(pwid);
    if (level != PWID_LEVEL_ROOT) {
        return -1;
    }
    
    inode->owner_pwid = owner_pwid;
    inode->mtime = get_time();
    
    return 0;
}

static int ramfs_mount(const char *path) {
    mem_set(&ramfs_data, 0, sizeof(ramfs_data));
    
    ramfs_data.free_inodes = RAMFS_MAX_INODES - 1;
    ramfs_data.free_blocks = RAMFS_MAX_BLOCKS;
    ramfs_data.root_inode = 1;
    
    struct ramfs_inode *root = &ramfs_data.inodes[1];
    root->inode_num = 1;
    root->type = VFS_TYPE_DIR;
    root->perm = 0755;
    root->size = 2 * sizeof(struct ramfs_dirent);
    root->owner_pwid = 0;
    root->atime = get_time();
    root->mtime = get_time();
    root->ctime = get_time();
    root->direct_blocks[0] = block_alloc();
    root->link_count = 2;
    root->used = 1;
    inode_set_used(1);
    
    struct ramfs_dirent *root_entries = (struct ramfs_dirent *)get_block(root->direct_blocks[0]);
    root_entries[0].inode = 1;
    root_entries[0].type = VFS_TYPE_DIR;
    str_cpy(root_entries[0].name, ".");
    
    root_entries[1].inode = 1;
    root_entries[1].type = VFS_TYPE_DIR;
    str_cpy(root_entries[1].name, "..");
    
    serial_puts(SERIAL_COM1, "RamFS: mounted at '");
    serial_puts(SERIAL_COM1, path);
    serial_puts(SERIAL_COM1, "'\n");
    
    return 0;
}

static struct vfs_file_operations ramfs_fops = {
    .open = ramfs_open,
    .close = ramfs_close,
    .read = ramfs_read,
    .write = ramfs_write,
    .seek = ramfs_seek,
    .readdir = ramfs_readdir,
};

static struct vfs_inode_operations ramfs_iops = {
    .create = NULL,
    .mkdir = ramfs_mkdir,
    .rmdir = ramfs_rmdir,
    .unlink = ramfs_unlink,
    .rename = ramfs_rename,
    .stat = ramfs_stat,
    .chmod = ramfs_chmod,
    .chown = ramfs_chown,
};

static struct vfs_sb_operations ramfs_sops = {
    .sync = NULL,
    .mount = ramfs_mount,
    .unmount = NULL,
};

void ramfs_init(void) {
    ramfs_fs.name[0] = '\0';
    ramfs_fs.fops = &ramfs_fops;
    ramfs_fs.iops = &ramfs_iops;
    ramfs_fs.sops = &ramfs_sops;
    ramfs_fs.fs_data = &ramfs_data;
    
    vfs_register_fs("ramfs", &ramfs_fs);
}
