#ifndef _VFS_H
#define _VFS_H

#include "types.h"
#include "pwid.h"

#define VFS_MAX_PATH        128
#define VFS_MAX_NAME        64
#define VFS_MAX_FDS         16
#define VFS_MAX_MOUNTS      8

#define VFS_TYPE_FILE       0
#define VFS_TYPE_DIR        1
#define VFS_TYPE_DEV        2
#define VFS_TYPE_SYMLINK    3

#define VFS_PERM_R          0x04
#define VFS_PERM_W          0x02
#define VFS_PERM_X          0x01

#define VFS_O_RDONLY        0x0001
#define VFS_O_WRONLY        0x0002
#define VFS_O_RDWR          0x0004
#define VFS_O_CREAT         0x0100
#define VFS_O_TRUNC         0x0200
#define VFS_O_APPEND        0x0400

#define VFS_SEEK_SET        0
#define VFS_SEEK_CUR        1
#define VFS_SEEK_END        2

struct vfs_stat {
    uint32_t inode_num;
    uint16_t mode;
    uint32_t size;
    uint64_t atime;
    uint64_t mtime;
    uint64_t ctime;
    uint64_t owner_pwid;
    uint16_t perm;
    uint8_t  type;
    uint8_t  reserved;
};

struct vfs_dirent {
    uint32_t inode;
    uint8_t  type;
    char     name[VFS_MAX_NAME];
};

struct vfs_file;

struct vfs_inode_operations {
    int (*create)(struct vfs_file *parent, const char *name, int type, uint64_t pwid);
    int (*mkdir)(struct vfs_file *parent, const char *name, uint64_t pwid);
    int (*rmdir)(struct vfs_file *parent, const char *name, uint64_t pwid);
    int (*unlink)(struct vfs_file *parent, const char *name, uint64_t pwid);
    int (*rename)(struct vfs_file *old_parent, const char *old_name,
                  struct vfs_file *new_parent, const char *new_name, uint64_t pwid);
    int (*stat)(struct vfs_file *file, struct vfs_stat *st);
    int (*chmod)(struct vfs_file *file, uint16_t mode, uint64_t pwid);
    int (*chown)(struct vfs_file *file, uint64_t owner_pwid, uint64_t pwid);
};

struct vfs_file_operations {
    int (*open)(struct vfs_file *file, int flags, uint64_t pwid);
    int (*close)(struct vfs_file *file);
    int (*read)(struct vfs_file *file, void *buf, uint32_t count);
    int (*write)(struct vfs_file *file, const void *buf, uint32_t count);
    int (*seek)(struct vfs_file *file, int64_t offset, int whence);
    int (*readdir)(struct vfs_file *file, struct vfs_dirent *entry);
};

struct vfs_file {
    uint32_t fd;
    uint32_t inode_num;
    uint64_t offset;
    int flags;
    uint64_t pwid;
    uint8_t  used;
    uint8_t  type;
    char     path[VFS_MAX_PATH];
    void *fs_data;
    void *private_data;
    struct vfs_file_operations *fops;
};

struct vfs_sb_operations {
    int (*sync)(void);
    int (*mount)(const char *path);
    int (*unmount)(void);
};

struct vfs_filesystem {
    char name[32];
    struct vfs_file_operations *fops;
    struct vfs_inode_operations *iops;
    struct vfs_sb_operations *sops;
    void *fs_data;
};

struct vfs_mount {
    char path[VFS_MAX_PATH];
    struct vfs_filesystem *fs;
    uint8_t used;
};

void vfs_init(void);

int vfs_register_fs(const char *name, struct vfs_filesystem *fs);
int vfs_unregister_fs(const char *name);
struct vfs_filesystem* vfs_get_fs(const char *name);

int vfs_mount(const char *path, const char *fs_name);
int vfs_unmount(const char *path);

int vfs_open(const char *path, int flags, uint64_t pwid);
int vfs_close(int fd);
int vfs_read(int fd, void *buf, uint32_t count);
int vfs_write(int fd, const void *buf, uint32_t count);
int vfs_seek(int fd, int64_t offset, int whence);
int vfs_readdir(int fd, struct vfs_dirent *entry);

int vfs_mkdir(const char *path, uint64_t pwid);
int vfs_rmdir(const char *path, uint64_t pwid);
int vfs_unlink(const char *path, uint64_t pwid);
int vfs_rename(const char *old_path, const char *new_path, uint64_t pwid);
int vfs_stat(const char *path, struct vfs_stat *st, uint64_t pwid);
int vfs_chmod(const char *path, uint16_t mode, uint64_t pwid);
int vfs_chown(const char *path, uint64_t owner_pwid, uint64_t pwid);

int vfs_sync(void);

void vfs_set_cwd(const char *path);
const char* vfs_get_cwd(void);
uint32_t vfs_get_cwd_inode(void);

struct vfs_mount* vfs_find_mount(const char *path);
const char* vfs_get_relative_path(const char *path, struct vfs_mount *mount);

extern struct vfs_file vfs_fd_table[VFS_MAX_FDS];

#endif
