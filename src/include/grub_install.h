#ifndef _GRUB_INSTALL_H
#define _GRUB_INSTALL_H

#include "types.h"

#define GRUB_BOOT_SECTOR_SIZE     512
#define GRUB_CORE_SIZE            (32 * 1024)

#define PARTITION_TYPE_EMPTY      0x00
#define PARTITION_TYPE_FAT32_LBA  0x0C
#define PARTITION_TYPE_LINUX      0x83
#define PARTITION_TYPE_HVFS       0xA0

#define PARTITION_FLAG_BOOTABLE   0x80

struct mbr_partition_entry {
    uint8_t  boot_flag;
    uint8_t  start_head;
    uint8_t  start_sector_cyl_high;
    uint8_t  start_cyl_low;
    uint8_t  partition_type;
    uint8_t  end_head;
    uint8_t  end_sector_cyl_high;
    uint8_t  end_cyl_low;
    uint32_t start_lba;
    uint32_t total_sectors;
} __attribute__((packed));

struct mbr_header {
    uint8_t  bootstrap_code[446];
    struct mbr_partition_entry partitions[4];
    uint8_t  signature[2];
} __attribute__((packed));

struct grub_boot_info {
    uint32_t disk_id;
    uint32_t boot_partition;
    uint64_t kernel_lba;
    uint32_t kernel_size;
};

int grub_install_mbr(uint32_t disk_id);
int grub_create_partition_table(uint32_t disk_id, uint64_t disk_sectors);
int grub_write_boot_code(uint32_t disk_id);
int grub_write_config(uint32_t disk_id, const char *kernel_path);

int grub_get_boot_sector(uint32_t disk_id, void *buffer);
int grub_set_boot_sector(uint32_t disk_id, const void *buffer);

uint32_t grub_calculate_kernel_lba(uint32_t disk_id, uint32_t partition);

#endif
