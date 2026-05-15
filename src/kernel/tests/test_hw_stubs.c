/* Hardware stubs for test kernel */
int ata_write_sector(unsigned char disk, unsigned int sector, const unsigned char *buf) { return 0; }
int ata_read_sector(unsigned char disk, unsigned int sector, unsigned char *buf) { return 0; }
int ata_disk_present(unsigned char disk) { return 0; }
