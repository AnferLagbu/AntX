#ifndef _HVFS_RUST_H
#define _HVFS_RUST_H

#include "types.h"

#ifdef __cplusplus
extern "C" {
#endif

void rust_hvfs_init(void);
int32_t rust_hvfs_format(void);
int32_t rust_hvfs_check_disk(void);
void rust_hvfs_set_disk_present(bool present);

int32_t rust_hvfs_open(const char *path, uint32_t flags, uint64_t pwid);
int32_t rust_hvfs_close(uint32_t fd);
int32_t rust_hvfs_read(uint32_t fd, uint8_t *buf, uint32_t count);
int32_t rust_hvfs_write(uint32_t fd, const uint8_t *buf, uint32_t count);
int32_t rust_hvfs_mkdir(const char *path, uint64_t pwid);
int32_t rust_hvfs_sync(void);

void rust_hvfs_get_stats(uint32_t *total_blocks, uint32_t *free_blocks,
                          uint32_t *total_inodes, uint32_t *free_inodes);

void rust_hvfs_set_current_dir(uint32_t inode_num);
uint32_t rust_hvfs_get_current_dir(void);
void rust_hvfs_set_current_pwid(uint64_t pwid);
uint64_t rust_hvfs_get_current_pwid(void);

#ifdef __cplusplus
}
#endif

#endif
