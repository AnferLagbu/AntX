#ifndef _VFS_FFI_H
#define _VFS_FFI_H

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

struct vfs_stat_ffi {
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

void vfs_init_internal(void);
int32_t vfs_mount_internal(const char *path, const char *fs_name);
int32_t vfs_unmount_internal(const char *path);
int32_t vfs_open_internal(const char *path, uint32_t flags, uint64_t pwid);
int32_t vfs_close_internal(uint32_t fd);
int32_t vfs_read_internal(uint32_t fd, void *buf, uint32_t count);
int32_t vfs_write_internal(uint32_t fd, const void *buf, uint32_t count);
int32_t vfs_mkdir_internal(const char *path, uint64_t pwid);
int32_t vfs_stat_internal(const char *path, struct vfs_stat_ffi *st, uint64_t pwid);
void vfs_set_cwd_internal(const char *path);
int32_t vfs_get_cwd_internal(char *buf, uint32_t size);

#ifdef __cplusplus
}
#endif

#endif
