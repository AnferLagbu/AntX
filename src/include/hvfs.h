#ifndef _HVFS_H
#define _HVFS_H

#include "types.h"
#include "pwid.h"

#define HVFS_MAGIC          0x48564653
#define HVFS_VERSION        2
#define HVFS_BLOCK_SIZE     4096
#define HVFS_MAX_FDS        64
#define HVFS_MAX_PATH       256
#define HVFS_MAX_NAME       128

#define HVFS_DEFAULT_INODES     4096
#define HVFS_DEFAULT_BLOCKS     65536
#define HVFS_MAX_INODES_LIMIT   1048576
#define HVFS_MAX_BLOCKS_LIMIT   16777216

#define HVFS_TYPE_FILE      0
#define HVFS_TYPE_DIR       1
#define HVFS_TYPE_SYMLINK   2

#define HVFS_PERM_R         0x04
#define HVFS_PERM_W         0x02
#define HVFS_PERM_X         0x01

#define HVFS_PERM_SUID      0x4000
#define HVFS_PERM_SGID      0x2000
#define HVFS_PERM_STICKY    0x1000

#define HVFS_O_RDONLY       0x0001
#define HVFS_O_WRONLY       0x0002
#define HVFS_O_RDWR         0x0004
#define HVFS_O_CREAT        0x0100
#define HVFS_O_TRUNC        0x0200
#define HVFS_O_APPEND       0x0400

#define HVFS_DISK_SECTOR_SIZE       512
#define HVFS_DISK_SECTORS_PER_BLOCK 8

#define HVFS_BOOT_SECTOR_START      0
#define HVFS_BOOT_SECTOR_COUNT      2

#define HVFS_SUPER_SECTOR_START     200
#define HVFS_SUPER_SECTOR_COUNT     8

#define HVFS_INODE_SECTOR_START     208
#define HVFS_INODE_SECTOR_COUNT     8192
#define HVFS_INODES_PER_SECTOR      1

#define HVFS_BLOCK_BITMAP_START     8400
#define HVFS_BLOCK_BITMAP_COUNT     2048

#define HVFS_INODE_BITMAP_START     10448
#define HVFS_INODE_BITMAP_COUNT     256

#define HVFS_LOG_SECTOR_START       10704
#define HVFS_LOG_SECTOR_COUNT       16

#define HVFS_DATA_SECTOR_START      10720

#define HVFS_DIR_ENTRY_SIZE         128

#define HVFS_DISK_OK                0
#define HVFS_DISK_NO_DISK           -1
#define HVFS_DISK_UNFORMATTED       -2
#define HVFS_DISK_VERSION_ERROR     -3
#define HVFS_DISK_CORRUPT           -4

#define HVFS_CACHE_SIZE             256

struct super_block {
    uint32_t magic;
    uint32_t version;
    uint32_t block_size;
    uint32_t total_blocks;
    uint32_t free_blocks;
    uint32_t inode_count;
    uint32_t free_inodes;
    uint32_t first_data_block;
    uint32_t root_inode;
    uint32_t max_path_depth;
    uint32_t max_entries;
    uint64_t created_time;
    uint64_t modified_time;
    uint64_t mount_time;
    uint32_t mount_count;
    uint32_t state;
    uint32_t dynamic_inodes;
    uint32_t dynamic_blocks;
};

struct inode {
    uint32_t inode_num;
    uint16_t mode;
    uint16_t reserved_uid;
    uint32_t size;
    uint64_t atime;
    uint64_t mtime;
    uint64_t ctime;
    uint64_t owner_pwid;
    uint64_t group_pwid;
    uint16_t pwid_perm;
    uint32_t direct_blocks[12];
    uint32_t indirect_block;
    uint32_t double_indirect;
    uint32_t triple_indirect;
    uint32_t link_count;
    uint32_t ref_count;
    uint8_t  used;
    uint8_t  dirty;
    uint8_t  in_cache;
};

struct dir_entry {
    uint32_t inode;
    uint16_t rec_len;
    uint8_t  name_len;
    uint8_t  file_type;
    char     name[HVFS_MAX_NAME];
};

struct file_descriptor {
    uint32_t fd;
    uint32_t inode_num;
    uint64_t offset;
    int flags;
    uint64_t pwid;
    uint8_t  used;
};

struct hvfs_context {
    uint64_t current_pwid;
    uint32_t current_dir;
    struct file_descriptor fds[HVFS_MAX_FDS];
};

struct hvfs_super_block_disk {
    uint32_t magic;
    uint32_t version;
    uint32_t block_size;
    uint32_t total_blocks;
    uint32_t free_blocks;
    uint32_t inode_count;
    uint32_t free_inodes;
    uint32_t first_data_block;
    uint32_t root_inode;
    uint32_t block_bitmap_block;
    uint32_t inode_bitmap_block;
    uint32_t inode_table_block;
    uint64_t created_time;
    uint64_t modified_time;
    uint64_t mount_time;
    uint32_t mount_count;
    uint32_t state;
    uint32_t dynamic_inodes;
    uint32_t dynamic_blocks;
    uint32_t checksum;
    uint8_t  reserved[452];
} __attribute__((packed));

struct hvfs_inode_disk {
    uint32_t inode_num;
    uint16_t mode;
    uint16_t reserved;
    uint32_t size;
    uint32_t blocks;
    uint64_t atime;
    uint64_t mtime;
    uint64_t ctime;
    uint64_t owner_pwid;
    uint64_t group_pwid;
    uint16_t pwid_perm;
    uint32_t link_count;
    uint32_t direct_blocks[12];
    uint32_t indirect_block;
    uint32_t double_indirect;
    uint32_t triple_indirect;
    uint8_t  flags;
    uint8_t  reserved2[19];
} __attribute__((packed));

struct hvfs_dir_entry_disk {
    uint32_t inode;
    uint16_t rec_len;
    uint8_t  name_len;
    uint8_t  file_type;
    char     name[64];
    uint8_t  reserved[52];
} __attribute__((packed));

struct block_cache_entry {
    uint32_t block_num;
    uint8_t *data;
    uint8_t dirty;
    uint8_t valid;
    uint32_t access_time;
};

void hvfs_init(void);
int hvfs_format(void);
int hvfs_create_default_directories(void);

int hvfs_disk_init(void);
int hvfs_mount(void);
int hvfs_unmount(void);

int hvfs_sync(void);
int hvfs_sync_inode(struct inode *inode);
int hvfs_sync_super(void);
int hvfs_sync_block_bitmap(void);
int hvfs_sync_inode_bitmap(void);

int hvfs_load_super(void);
int hvfs_load_inode_table(void);
int hvfs_load_block_bitmap(void);
int hvfs_load_inode_bitmap(void);

int hvfs_check_disk(void);
int hvfs_format_disk(void);

int hvfs_open(const char *path, int flags, uint64_t pwid);
int hvfs_close(int fd);
int hvfs_read(int fd, void *buf, uint32_t count);
int hvfs_write(int fd, const void *buf, uint32_t count);
int hvfs_seek(int fd, int64_t offset, int whence);

int hvfs_mkdir(const char *path, uint64_t pwid);
int hvfs_rmdir(const char *path, uint64_t pwid);
int hvfs_readdir(int fd, struct dir_entry *entry);

int hvfs_unlink(const char *path, uint64_t pwid);
int hvfs_rename(const char *old_path, const char *new_path, uint64_t pwid);

int hvfs_stat(const char *path, struct inode *stat_buf, uint64_t pwid);
int hvfs_chmod(const char *path, uint16_t mode, uint64_t pwid);
int hvfs_chown(const char *path, uint64_t owner_pwid, uint64_t pwid);

int hvfs_check_permission(struct inode *inode, uint64_t pwid, int access_type);

struct inode* hvfs_get_inode(uint32_t inode_num);
struct inode* hvfs_find_inode(const char *path);

void hvfs_set_context(uint64_t pwid);
uint64_t hvfs_get_current_pwid(void);
uint32_t hvfs_get_current_dir(void);
void hvfs_set_current_dir(uint32_t inode_num);

void hvfs_list_root(void);
void hvfs_dump_super(void);

int hvfs_is_disk_mode(void);

int hvfs_expand_inodes(uint32_t new_count);
int hvfs_expand_blocks(uint32_t new_count);

uint32_t hvfs_get_total_blocks(void);
uint32_t hvfs_get_free_blocks(void);
uint32_t hvfs_get_total_inodes(void);
uint32_t hvfs_get_free_inodes(void);

extern struct super_block hvfs_super;

#endif
