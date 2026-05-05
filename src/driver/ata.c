#include "ata.h"
#include "io.h"
#include "klog.h"

static int ata_primary_present = 0;
static int ata_secondary_present = 0;
static int ata_master_present[4] = {0, 0, 0, 0};

static uint16_t ata_get_io_base(uint8_t drive) {
    return (drive < 2) ? ATA_PRIMARY_IO : ATA_SECONDARY_IO;
}

static uint16_t ata_get_ctrl_base(uint8_t drive) {
    return (drive < 2) ? ATA_PRIMARY_CTRL : ATA_SECONDARY_CTRL;
}

static void ata_delay(uint16_t ctrl) {
    for (int i = 0; i < 4; i++) {
        inb(ctrl);
    }
}

static int ata_wait_bsy(uint16_t io, uint16_t ctrl) {
    uint32_t timeout = ATA_TIMEOUT;
    
    while (timeout--) {
        uint8_t status = inb(io + ATA_STATUS);
        if (!(status & ATA_STATUS_BSY)) {
            return ATA_SUCCESS;
        }
        ata_delay(ctrl);
    }
    return ATA_TIMEOUT_ERR;
}

static int ata_wait_drq(uint16_t io, uint16_t ctrl) {
    uint32_t timeout = ATA_TIMEOUT;
    
    while (timeout--) {
        uint8_t status = inb(io + ATA_STATUS);
        if (status & ATA_STATUS_ERR) {
            return ATA_ERR;
        }
        if ((status & (ATA_STATUS_DRQ | ATA_STATUS_BSY)) == ATA_STATUS_DRQ) {
            return ATA_SUCCESS;
        }
        ata_delay(ctrl);
    }
    return ATA_TIMEOUT_ERR;
}

static int ata_select_drive(uint16_t io, uint16_t ctrl, uint8_t slave) {
    outb(io + ATA_DRIVE_HEAD, 0xA0 | (slave << 4));
    ata_delay(ctrl);
    
    uint32_t timeout = ATA_TIMEOUT;
    while (timeout--) {
        uint8_t status = inb(io + ATA_STATUS);
        if (!(status & ATA_STATUS_BSY)) {
            return ATA_SUCCESS;
        }
        ata_delay(ctrl);
    }
    return ATA_TIMEOUT_ERR;
}

static int ata_detect_drive(uint16_t io, uint16_t ctrl, uint8_t slave) {
    if (ata_select_drive(io, ctrl, slave) != ATA_SUCCESS) {
        return 0;
    }
    
    outb(io + ATA_SECTOR_COUNT, 0);
    outb(io + ATA_SECTOR_NUM, 0);
    outb(io + ATA_CYLINDER_LOW, 0);
    outb(io + ATA_CYLINDER_HIGH, 0);
    
    outb(io + ATA_COMMAND, ATA_CMD_IDENTIFY);
    ata_delay(ctrl);
    
    uint8_t status = inb(io + ATA_STATUS);
    if (status == 0) {
        return 0;
    }
    
    if (ata_wait_bsy(io, ctrl) != ATA_SUCCESS) {
        return 0;
    }
    
    status = inb(io + ATA_STATUS);
    if (status & ATA_STATUS_ERR) {
        return 0;
    }
    
    if (ata_wait_drq(io, ctrl) != ATA_SUCCESS) {
        return 0;
    }
    
    for (int i = 0; i < 256; i++) {
        inw(io + ATA_DATA);
    }
    
    return 1;
}

void ata_init(void) {
    ata_primary_present = 0;
    ata_secondary_present = 0;
    
    for (int i = 0; i < 4; i++) {
        ata_master_present[i] = 0;
    }
    
    outb(ATA_PRIMARY_CTRL, 0x04);
    ata_delay(ATA_PRIMARY_CTRL);
    outb(ATA_PRIMARY_CTRL, 0x00);
    ata_delay(ATA_PRIMARY_CTRL);
    
    outb(ATA_PRIMARY_IO + ATA_SECTOR_COUNT, 0x55);
    outb(ATA_PRIMARY_IO + ATA_SECTOR_NUM, 0xAA);
    
    uint8_t count = inb(ATA_PRIMARY_IO + ATA_SECTOR_COUNT);
    uint8_t num = inb(ATA_PRIMARY_IO + ATA_SECTOR_NUM);
    
    if (count == 0x55 && num == 0xAA) {
        ata_primary_present = 1;
        klog_drv("ATA: Primary controller detected");

        if (ata_detect_drive(ATA_PRIMARY_IO, ATA_PRIMARY_CTRL, 0)) {
            ata_master_present[0] = 1;
            klog_drv("ATA: Primary master detected");
        }
        if (ata_detect_drive(ATA_PRIMARY_IO, ATA_PRIMARY_CTRL, 1)) {
            ata_master_present[1] = 1;
            klog_drv("ATA: Primary slave detected");
        }
    }
    
    outb(ATA_SECONDARY_CTRL, 0x04);
    ata_delay(ATA_SECONDARY_CTRL);
    outb(ATA_SECONDARY_CTRL, 0x00);
    ata_delay(ATA_SECONDARY_CTRL);
    
    outb(ATA_SECONDARY_IO + ATA_SECTOR_COUNT, 0x55);
    outb(ATA_SECONDARY_IO + ATA_SECTOR_NUM, 0xAA);
    
    count = inb(ATA_SECONDARY_IO + ATA_SECTOR_COUNT);
    num = inb(ATA_SECONDARY_IO + ATA_SECTOR_NUM);
    
    if (count == 0x55 && num == 0xAA) {
        ata_secondary_present = 1;
        klog_drv("ATA: Secondary controller detected");

        if (ata_detect_drive(ATA_SECONDARY_IO, ATA_SECONDARY_CTRL, 0)) {
            ata_master_present[2] = 1;
            klog_drv("ATA: Secondary master detected");
        }
        if (ata_detect_drive(ATA_SECONDARY_IO, ATA_SECONDARY_CTRL, 1)) {
            ata_master_present[3] = 1;
            klog_drv("ATA: Secondary slave detected");
        }
    }
}

int ata_disk_present(uint8_t drive) {
    if (drive >= 4) return 0;
    return ata_master_present[drive];
}

int ata_read_sector(uint8_t drive, uint32_t lba, void *buffer) {
    if (buffer == NULL) {
        return ATA_ERR;
    }
    if (!ata_disk_present(drive)) {
        return ATA_NO_DISK;
    }
    
    uint16_t io = ata_get_io_base(drive);
    uint16_t ctrl = ata_get_ctrl_base(drive);
    uint8_t slave = (drive & 0x01);
    
    if (ata_select_drive(io, ctrl, slave) != ATA_SUCCESS) {
        return ATA_ERR;
    }
    
    outb(io + ATA_SECTOR_COUNT, 1);
    outb(io + ATA_SECTOR_NUM, lba & 0xFF);
    outb(io + ATA_CYLINDER_LOW, (lba >> 8) & 0xFF);
    outb(io + ATA_CYLINDER_HIGH, (lba >> 16) & 0xFF);
    outb(io + ATA_DRIVE_HEAD, 0xE0 | (slave << 4) | ((lba >> 24) & 0x0F));
    ata_delay(ctrl);
    
    outb(io + ATA_COMMAND, ATA_CMD_READ_SECTORS);
    ata_delay(ctrl);
    
    if (ata_wait_bsy(io, ctrl) != ATA_SUCCESS) {
        return ATA_TIMEOUT_ERR;
    }
    
    if (ata_wait_drq(io, ctrl) != ATA_SUCCESS) {
        return ATA_ERR;
    }
    
    for (int i = 0; i < 256; i++) {
        ((uint16_t*)buffer)[i] = inw(io + ATA_DATA);
    }
    
    return ATA_SUCCESS;
}

int ata_write_sector(uint8_t drive, uint32_t lba, const void *buffer) {
    if (buffer == NULL) {
        return ATA_ERR;
    }
    if (!ata_disk_present(drive)) {
        return ATA_NO_DISK;
    }
    
    uint16_t io = ata_get_io_base(drive);
    uint16_t ctrl = ata_get_ctrl_base(drive);
    uint8_t slave = (drive & 0x01);
    
    if (ata_select_drive(io, ctrl, slave) != ATA_SUCCESS) {
        return ATA_ERR;
    }
    
    outb(io + ATA_SECTOR_COUNT, 1);
    outb(io + ATA_SECTOR_NUM, lba & 0xFF);
    outb(io + ATA_CYLINDER_LOW, (lba >> 8) & 0xFF);
    outb(io + ATA_CYLINDER_HIGH, (lba >> 16) & 0xFF);
    outb(io + ATA_DRIVE_HEAD, 0xE0 | (slave << 4) | ((lba >> 24) & 0x0F));
    ata_delay(ctrl);
    
    outb(io + ATA_COMMAND, ATA_CMD_WRITE_SECTORS);
    ata_delay(ctrl);
    
    if (ata_wait_bsy(io, ctrl) != ATA_SUCCESS) {
        return ATA_TIMEOUT_ERR;
    }
    
    if (ata_wait_drq(io, ctrl) != ATA_SUCCESS) {
        return ATA_ERR;
    }
    
    for (int i = 0; i < 256; i++) {
        outw(io + ATA_DATA, ((const uint16_t*)buffer)[i]);
    }
    
    outb(io + ATA_COMMAND, ATA_CMD_FLUSH_CACHE);
    ata_delay(ctrl);
    
    if (ata_wait_bsy(io, ctrl) != ATA_SUCCESS) {
        return ATA_TIMEOUT_ERR;
    }
    
    return ATA_SUCCESS;
}

int ata_read_sectors(uint8_t drive, uint32_t lba, uint32_t count, void *buffer) {
    if (buffer == NULL || count == 0) {
        return ATA_ERR;
    }
    uint8_t *buf = (uint8_t *)buffer;
    
    for (uint32_t i = 0; i < count; i++) {
        int result = ata_read_sector(drive, lba + i, buf + (i * 512));
        if (result != ATA_SUCCESS) {
            return result;
        }
    }
    
    return ATA_SUCCESS;
}

int ata_write_sectors(uint8_t drive, uint32_t lba, uint32_t count, const void *buffer) {
    if (buffer == NULL || count == 0) {
        return ATA_ERR;
    }
    const uint8_t *buf = (const uint8_t *)buffer;
    
    for (uint32_t i = 0; i < count; i++) {
        int result = ata_write_sector(drive, lba + i, buf + (i * 512));
        if (result != ATA_SUCCESS) {
            return result;
        }
    }
    
    return ATA_SUCCESS;
}

int ata_identify(uint8_t drive, uint16_t *identify_data) {
    if (identify_data == NULL) {
        return ATA_ERR;
    }
    if (!ata_disk_present(drive)) {
        return ATA_NO_DISK;
    }
    
    uint16_t io = ata_get_io_base(drive);
    uint16_t ctrl = ata_get_ctrl_base(drive);
    uint8_t slave = (drive & 0x01);
    
    if (ata_select_drive(io, ctrl, slave) != ATA_SUCCESS) {
        return ATA_ERR;
    }
    
    outb(io + ATA_SECTOR_COUNT, 0);
    outb(io + ATA_SECTOR_NUM, 0);
    outb(io + ATA_CYLINDER_LOW, 0);
    outb(io + ATA_CYLINDER_HIGH, 0);
    
    outb(io + ATA_COMMAND, ATA_CMD_IDENTIFY);
    ata_delay(ctrl);
    
    if (ata_wait_bsy(io, ctrl) != ATA_SUCCESS) {
        return ATA_TIMEOUT_ERR;
    }
    
    uint8_t status = inb(io + ATA_STATUS);
    if (status == 0) {
        return ATA_NO_DISK;
    }
    
    if (ata_wait_drq(io, ctrl) != ATA_SUCCESS) {
        return ATA_ERR;
    }
    
    for (int i = 0; i < 256; i++) {
        identify_data[i] = inw(io + ATA_DATA);
    }
    
    return ATA_SUCCESS;
}
