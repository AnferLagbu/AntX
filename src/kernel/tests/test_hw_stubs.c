/* Hardware stubs for test kernel */
int ata_write_sector(unsigned char disk, unsigned int sector, const unsigned char *buf) { (void)disk; (void)sector; (void)buf; return 0; }
int ata_read_sector(unsigned char disk, unsigned int sector, unsigned char *buf) { (void)disk; (void)sector; (void)buf; return 0; }
int ata_disk_present(unsigned char disk) { (void)disk; return 0; }

/* ── Network glue stubs ── */
struct netif;
struct pbuf;

void qx_netif_init_virtio(struct netif *netif, const unsigned char *mac) { (void)netif; (void)mac; }
void qx_pbuf_copyout(struct pbuf *p, void *buf, unsigned short *out_len) { (void)p; (void)buf; if (out_len) *out_len = 0; }
int ethernet_input_from_virtio(void *data, unsigned short len) { (void)data; (void)len; return 0; }

/* ── Disk read stub (used by HvFS test) ── */
int hdd_is_present(unsigned char disk) { (void)disk; return 0; }
int hdd_read_sector(unsigned char disk, unsigned int sector, unsigned char *buf) { (void)disk; (void)sector; (void)buf; return 0; }
int hdd_write_sector(unsigned char disk, unsigned int sector, const unsigned char *buf) { (void)disk; (void)sector; (void)buf; return 0; }
int ata_get_drive_count(void) { return 0; }
unsigned int ata_get_drive_size(unsigned char disk) { (void)disk; return 0; }

/* ── I/O port stubs ── */
unsigned char inb(unsigned short port) { (void)port; return 0; }
void outb(unsigned short port, unsigned char val) { (void)port; (void)val; }
unsigned short inw(unsigned short port) { (void)port; return 0; }
void outw(unsigned short port, unsigned short val) { (void)port; (void)val; }
unsigned int inl(unsigned short port) { (void)port; return 0; }
void outl(unsigned short port, unsigned int val) { (void)port; (void)val; }

/* ── PCI stubs ── */
unsigned short pci_config_read_word(unsigned char bus, unsigned char slot, unsigned char func, unsigned char offset) {
    (void)bus; (void)slot; (void)func; (void)offset; return 0xFFFF;
}
unsigned int pci_config_read_dword(unsigned char bus, unsigned char slot, unsigned char func, unsigned char offset) {
    (void)bus; (void)slot; (void)func; (void)offset; return 0xFFFFFFFF;
}

/* ── Timer stubs ── */
unsigned long long timer_get_ticks(void) { return 0; }
void timer_sleep_ms(unsigned int ms) { (void)ms; }

/* ── MTRs ── */
void mtr_write(void) { }

/* ── Kernel end symbol (defined in linker script, but some toolchains need a weak def) ── */
char _kernel_end[1];
