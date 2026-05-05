#ifndef _HVFS_FFI_H
#define _HVFS_FFI_H

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

void hvfs_init_internal(void);
int32_t hvfs_format_internal(void);
int32_t hvfs_check_disk_internal(void);
void hvfs_set_disk_present_internal(bool present);

int32_t hvfs_open_internal(const char *path, uint32_t flags, uint64_t pwid);
int32_t hvfs_close_internal(uint32_t fd);
int32_t hvfs_read_internal(uint32_t fd, uint8_t *buf, uint32_t count);
int32_t hvfs_write_internal(uint32_t fd, const uint8_t *buf, uint32_t count);
int32_t hvfs_mkdir_internal(const char *path, uint64_t pwid);
int32_t hvfs_unlink_internal(const char *path, uint64_t pwid);
int32_t hvfs_rmdir_internal(const char *path, uint64_t pwid);
int32_t hvfs_sync_internal(void);

void hvfs_get_stats_internal(uint32_t *total_blocks, uint32_t *free_blocks,
                          uint32_t *total_inodes, uint32_t *free_inodes);

void hvfs_set_current_dir_internal(uint32_t inode_num);
uint32_t hvfs_get_current_dir_internal(void);
void hvfs_set_current_pwid_internal(uint64_t pwid);
uint64_t hvfs_get_current_pwid_internal(void);

#ifdef __cplusplus
}
#endif

#endif
