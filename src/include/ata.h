#ifndef _ATA_H
#define _ATA_H

#include "types.h"

#define ATA_PRIMARY_IO        0x1F0
#define ATA_PRIMARY_CTRL      0x3F6
#define ATA_SECONDARY_IO      0x170
#define ATA_SECONDARY_CTRL    0x376

#define ATA_DATA              0x00
#define ATA_ERROR_REG         0x01
#define ATA_FEATURES          0x01
#define ATA_SECTOR_COUNT      0x02
#define ATA_SECTOR_NUM        0x03
#define ATA_CYLINDER_LOW      0x04
#define ATA_CYLINDER_HIGH     0x05
#define ATA_DRIVE_HEAD        0x06
#define ATA_STATUS            0x07
#define ATA_COMMAND           0x07

#define ATA_ALT_STATUS        0x00
#define ATA_DEV_CTRL          0x00

#define ATA_CMD_READ_SECTORS    0x20
#define ATA_CMD_WRITE_SECTORS   0x30
#define ATA_CMD_IDENTIFY        0xEC
#define ATA_CMD_FLUSH_CACHE     0xE7

#define ATA_STATUS_BSY          0x80
#define ATA_STATUS_DRDY         0x40
#define ATA_STATUS_DF           0x20
#define ATA_STATUS_DSC          0x10
#define ATA_STATUS_DRQ          0x08
#define ATA_STATUS_CORR         0x04
#define ATA_STATUS_IDX          0x02
#define ATA_STATUS_ERR          0x01

#define ATA_ERR_AMNF            0x01
#define ATA_ERR_TK0NF           0x02
#define ATA_ERR_ABRT            0x04
#define ATA_ERR_MCR             0x08
#define ATA_ERR_IDNF            0x10
#define ATA_ERR_MC              0x20
#define ATA_ERR_UNC             0x40
#define ATA_ERR_BBK             0x80

#define ATA_DRIVE_MASTER        0x00
#define ATA_DRIVE_SLAVE         0x01

#define ATA_TIMEOUT             1000000

#define ATA_SUCCESS             0
#define ATA_ERR                 -1
#define ATA_TIMEOUT_ERR         -2
#define ATA_NO_DISK             -3

void ata_init(void);

int ata_disk_present(uint8_t drive);

int ata_read_sector(uint8_t drive, uint32_t lba, void *buffer);
int ata_write_sector(uint8_t drive, uint32_t lba, const void *buffer);

int ata_read_sectors(uint8_t drive, uint32_t lba, uint32_t count, void *buffer);
int ata_write_sectors(uint8_t drive, uint32_t lba, uint32_t count, const void *buffer);

int ata_identify(uint8_t drive, uint16_t *identify_data);

#endif
