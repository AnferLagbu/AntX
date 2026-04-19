#ifndef _VFS_RUST_H
#define _VFS_RUST_H

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

struct vfs_stat_rust {
    uint32_t inode_num;
    uint16_t mode;
    uint32_t size;
    uint64_t atime;
    uint64_t mtime;
    uint64_t ctime;
    uint64_t owner_pwid;
    uint16_t perm;
    uint8_t  file_type;
    uint8_t  reserved;
};

void rust_vfs_init(void);
int32_t rust_vfs_mount(const char *path, const char *fs_name);
int32_t rust_vfs_unmount(const char *path);
int32_t rust_vfs_open(const char *path, uint32_t flags, uint64_t pwid);
int32_t rust_vfs_close(uint32_t fd);
int32_t rust_vfs_read(uint32_t fd, void *buf, uint32_t count);
int32_t rust_vfs_write(uint32_t fd, const void *buf, uint32_t count);
int32_t rust_vfs_mkdir(const char *path, uint64_t pwid);
int32_t rust_vfs_stat(const char *path, struct vfs_stat_rust *st, uint64_t pwid);
void rust_vfs_set_cwd(const char *path);
int32_t rust_vfs_get_cwd(char *buf, uint32_t size);

#ifdef __cplusplus
}
#endif

#endif
