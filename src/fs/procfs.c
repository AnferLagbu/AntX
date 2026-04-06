#include "vfs.h"
#include "serial.h"
#include "pwid.h"
#include "proc.h"

#define PROCFS_MAX_ENTRIES     16
#define PROCFS_MAX_NAME        32
#define PROCFS_MAX_CONTENT     256

#define PROC_ENTRY_SELF        0
#define PROC_ENTRY_VERSION     1
#define PROC_ENTRY_UPTIME      2
#define PROC_ENTRY_MEMINFO     3
#define PROC_ENTRY_CPUINFO     4

struct procfs_entry {
    char name[PROCFS_MAX_NAME];
    uint8_t type;
    uint8_t used;
    char content[PROCFS_MAX_CONTENT];
    int content_len;
};

static struct procfs_entry procfs_entries[PROCFS_MAX_ENTRIES];
static int procfs_entry_count = 0;
static struct vfs_filesystem procfs_fs;

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

static void uint_to_str(uint64_t val, char *buf) {
    char tmp[32];
    int i = 0;
    
    if (val == 0) {
        buf[0] = '0';
        buf[1] = '\0';
        return;
    }
    
    while (val > 0) {
        tmp[i++] = '0' + (val % 10);
        val /= 10;
    }
    
    int j = 0;
    while (i > 0) {
        buf[j++] = tmp[--i];
    }
    buf[j] = '\0';
}

static struct procfs_entry* find_entry(const char *name) {
    for (int i = 0; i < PROCFS_MAX_ENTRIES; i++) {
        if (procfs_entries[i].used && str_cmp(procfs_entries[i].name, name) == 0) {
            return &procfs_entries[i];
        }
    }
    return NULL;
}

static void update_entry_content(struct procfs_entry *entry) {
    if (entry == NULL) return;
    
    switch (entry->type) {
        case PROC_ENTRY_SELF:
            entry->content_len = 0;
            str_cpy(entry->content, "self -> current process\n");
            entry->content_len = str_len(entry->content);
            break;
            
        case PROC_ENTRY_VERSION:
            entry->content_len = 0;
            str_cpy(entry->content, "AntX OS version 0.1.0\n");
            entry->content_len = str_len(entry->content);
            break;
            
        case PROC_ENTRY_UPTIME:
            {
                uint64_t tsc;
                __asm__ volatile ("rdtsc" : "=A"(tsc));
                char uptime[32];
                uint_to_str(tsc / 1000000, uptime);
                str_cpy(entry->content, "uptime: ");
                mem_cpy(entry->content + str_len(entry->content), uptime, str_len(uptime));
                str_cpy(entry->content + str_len(entry->content), " ms\n");
                entry->content_len = str_len(entry->content);
            }
            break;
            
        case PROC_ENTRY_MEMINFO:
            str_cpy(entry->content, "MemTotal: 128 MB\nMemFree: 64 MB\n");
            entry->content_len = str_len(entry->content);
            break;
            
        case PROC_ENTRY_CPUINFO:
            str_cpy(entry->content, "processor: 0\nvendor: AntX CPU\n");
            entry->content_len = str_len(entry->content);
            break;
            
        default:
            break;
    }
}

static int procfs_open(struct vfs_file *file, int flags, uint64_t pwid) {
    if (file == NULL) return -1;
    
    const char *entry_name = file->path;
    if (*entry_name == '/') entry_name++;
    
    if (*entry_name == '\0') {
        file->inode_num = 0;
        file->offset = 0;
        file->type = VFS_TYPE_DIR;
        file->private_data = NULL;
        return 0;
    }
    
    struct procfs_entry *entry = find_entry(entry_name);
    if (entry == NULL) {
        return -1;
    }
    
    update_entry_content(entry);
    
    file->inode_num = entry->type;
    file->offset = 0;
    file->type = VFS_TYPE_FILE;
    file->private_data = entry;
    
    return 0;
}

static int procfs_close(struct vfs_file *file) {
    if (file == NULL) return -1;
    file->private_data = NULL;
    return 0;
}

static int procfs_read(struct vfs_file *file, void *buf, uint32_t count) {
    if (file == NULL || buf == NULL) return -1;
    
    if (file->type == VFS_TYPE_DIR) {
        return -1;
    }
    
    struct procfs_entry *entry = (struct procfs_entry *)file->private_data;
    if (entry == NULL) return -1;
    
    uint32_t bytes_read = 0;
    
    if (file->offset < entry->content_len) {
        bytes_read = entry->content_len - file->offset;
        if (bytes_read > count) {
            bytes_read = count;
        }
        mem_cpy(buf, entry->content + file->offset, bytes_read);
        file->offset += bytes_read;
    }
    
    return bytes_read;
}

static int procfs_write(struct vfs_file *file, const void *buf, uint32_t count) {
    (void)file;
    (void)buf;
    (void)count;
    return -1;
}

static int procfs_seek(struct vfs_file *file, int64_t offset, int whence) {
    if (file == NULL) return -1;
    
    struct procfs_entry *entry = (struct procfs_entry *)file->private_data;
    if (entry == NULL) return -1;
    
    int64_t new_offset;
    
    switch (whence) {
        case VFS_SEEK_SET:
            new_offset = offset;
            break;
        case VFS_SEEK_CUR:
            new_offset = file->offset + offset;
            break;
        case VFS_SEEK_END:
            new_offset = entry->content_len + offset;
            break;
        default:
            return -1;
    }
    
    if (new_offset < 0) new_offset = 0;
    if (new_offset > entry->content_len) new_offset = entry->content_len;
    
    file->offset = new_offset;
    return new_offset;
}

static int procfs_readdir(struct vfs_file *file, struct vfs_dirent *entry) {
    if (file == NULL || entry == NULL) return -1;
    
    int entry_idx = file->offset;
    
    while (entry_idx < PROCFS_MAX_ENTRIES && !procfs_entries[entry_idx].used) {
        entry_idx++;
    }
    
    if (entry_idx >= PROCFS_MAX_ENTRIES) {
        return 0;
    }
    
    entry->inode = procfs_entries[entry_idx].type;
    entry->type = VFS_TYPE_FILE;
    str_cpy(entry->name, procfs_entries[entry_idx].name);
    
    file->offset = entry_idx + 1;
    
    return 1;
}

static int procfs_stat(struct vfs_file *file, struct vfs_stat *st) {
    if (file == NULL || st == NULL) return -1;
    
    const char *entry_name = file->path;
    if (*entry_name == '/') entry_name++;
    
    if (*entry_name == '\0') {
        st->inode_num = 0;
        st->mode = 0555;
        st->size = 0;
        st->type = VFS_TYPE_DIR;
        st->perm = 0555;
        return 0;
    }
    
    struct procfs_entry *entry = find_entry(entry_name);
    if (entry == NULL) return -1;
    
    st->inode_num = entry->type;
    st->mode = 0444;
    st->size = entry->content_len;
    st->type = VFS_TYPE_FILE;
    st->perm = 0444;
    
    return 0;
}

static int procfs_mount(const char *path) {
    for (int i = 0; i < PROCFS_MAX_ENTRIES; i++) {
        procfs_entries[i].used = 0;
    }
    
    procfs_entries[0].used = 1;
    str_cpy(procfs_entries[0].name, "self");
    procfs_entries[0].type = PROC_ENTRY_SELF;
    
    procfs_entries[1].used = 1;
    str_cpy(procfs_entries[1].name, "version");
    procfs_entries[1].type = PROC_ENTRY_VERSION;
    
    procfs_entries[2].used = 1;
    str_cpy(procfs_entries[2].name, "uptime");
    procfs_entries[2].type = PROC_ENTRY_UPTIME;
    
    procfs_entries[3].used = 1;
    str_cpy(procfs_entries[3].name, "meminfo");
    procfs_entries[3].type = PROC_ENTRY_MEMINFO;
    
    procfs_entries[4].used = 1;
    str_cpy(procfs_entries[4].name, "cpuinfo");
    procfs_entries[4].type = PROC_ENTRY_CPUINFO;
    
    procfs_entry_count = 5;
    
    serial_puts(SERIAL_COM1, "ProcFS: mounted at '");
    serial_puts(SERIAL_COM1, path);
    serial_puts(SERIAL_COM1, "'\n");
    
    return 0;
}

static struct vfs_file_operations procfs_fops = {
    .open = procfs_open,
    .close = procfs_close,
    .read = procfs_read,
    .write = procfs_write,
    .seek = procfs_seek,
    .readdir = procfs_readdir,
};

static struct vfs_inode_operations procfs_iops = {
    .create = NULL,
    .mkdir = NULL,
    .rmdir = NULL,
    .unlink = NULL,
    .rename = NULL,
    .stat = procfs_stat,
    .chmod = NULL,
    .chown = NULL,
};

static struct vfs_sb_operations procfs_sops = {
    .sync = NULL,
    .mount = procfs_mount,
    .unmount = NULL,
};

void procfs_init(void) {
    procfs_fs.name[0] = '\0';
    procfs_fs.fops = &procfs_fops;
    procfs_fs.iops = &procfs_iops;
    procfs_fs.sops = &procfs_sops;
    procfs_fs.fs_data = NULL;
    
    vfs_register_fs("procfs", &procfs_fs);
}
