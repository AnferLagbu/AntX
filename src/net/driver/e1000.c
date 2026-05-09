/* ============================================================
 * e1000.c — Intel 82540EM 网卡驱动
 *
 * 基于 Intel 82540EM Gigabit Ethernet Controller Datasheet
 * 目标: QEMU -device e1000
 *
 * 参考: lwIP contrib/ports/unix/port/netif/e1000.c
 * ============================================================ */

#include "e1000.h"
#include "e1000_regs.h"
#include "klog.h"
#include "dma.h"
#include "kmalloc.h"
#include "types.h"
#include "idt.h"

#include "lwip/opt.h"
#include "lwip/pbuf.h"
#include "lwip/netif.h"
#include "lwip/etharp.h"
#include "lwip/def.h"
#include "lwip/err.h"

e1000_dev_t g_e1000 = {0};

#define KERNEL_VMA_BASE  0xFFFF800000000000ULL

static inline uint64_t virt_to_phys(void *virt)
{
    uint64_t v = (uint64_t)(uintptr_t)virt;
    if (v >= KERNEL_VMA_BASE) {
        return v - KERNEL_VMA_BASE;
    }
    return v;
}

/* ---- 对齐分配辅助 ---- */
static void *kmalloc_align(size_t size, size_t align)
{
    void *raw = kmalloc(size + align);
    if (!raw) return NULL;
    uintptr_t addr = (uintptr_t)raw;
    uintptr_t offset = (align - (addr % align)) % align;
    return (void *)(addr + offset);
}

/* ---- 内联 MMIO 辅助 ---- */
static inline uint32_t mmio_read32(volatile uint8_t *base, uint32_t reg)
{
    return *(volatile uint32_t *)(base + reg);
}

static inline void mmio_write32(volatile uint8_t *base, uint32_t reg, uint32_t val)
{
    *(volatile uint32_t *)(base + reg) = val;
}

/* ---- EEPROM 读 (获取 MAC) ---- */
static uint16_t e1000_eeprom_read(volatile uint8_t *base, uint8_t addr)
{
    uint32_t val;
    mmio_write32(base, E1000_EERD, ((uint32_t)addr << 2) | E1000_EERD_START);
    while (1) {
        val = mmio_read32(base, E1000_EERD);
        if (val & E1000_EERD_DONE) break;
    }
    return (uint16_t)(val >> 16);
}

/* ---- 读取 MAC 地址 ---- */
static void e1000_read_mac(volatile uint8_t *base, uint8_t mac[6])
{
    uint16_t lo, hi;
    lo = e1000_eeprom_read(base, 0);
    hi = e1000_eeprom_read(base, 1);
    mac[0] = (uint8_t)(lo & 0xFF);
    mac[1] = (uint8_t)(lo >> 8);
    mac[2] = (uint8_t)(hi & 0xFF);
    mac[3] = (uint8_t)(hi >> 8);
    lo = e1000_eeprom_read(base, 2);
    mac[4] = (uint8_t)(lo & 0xFF);
    mac[5] = (uint8_t)(lo >> 8);
}

/* ============================================================
 * 初始化 TX/RX 描述符环
 * ============================================================ */
static int e1000_setup_rings(e1000_dev_t *dev)
{
    int i;
    volatile uint8_t *base = dev->mmio_base;

    /* 分配 TX 描述符 (16字节对齐) */
    extern void *kmalloc_align(size_t size, size_t align);
    dev->tx_descs = (e1000_tx_desc_t *)kmalloc_align(
        sizeof(e1000_tx_desc_t) * E1000_TX_RING_SIZE, 16);
    if (!dev->tx_descs) return -1;

    for (i = 0; i < E1000_TX_RING_SIZE; i++) {
        dev->tx_descs[i].addr   = 0;
        dev->tx_descs[i].length = 0;
        dev->tx_descs[i].cmd    = 0;
        dev->tx_descs[i].status = E1000_TXD_STAT_DD;
    }
    dev->tx_tail = 0;

    uint64_t tx_phys = virt_to_phys(dev->tx_descs);
    mmio_write32(base, E1000_TDBAL, (uint32_t)(tx_phys & 0xFFFFFFFF));
    mmio_write32(base, E1000_TDBAH, (uint32_t)(tx_phys >> 32));
    mmio_write32(base, E1000_TDLEN, sizeof(e1000_tx_desc_t) * E1000_TX_RING_SIZE);
    mmio_write32(base, E1000_TDH,   0);
    mmio_write32(base, E1000_TDT,   0);

    dev->rx_descs = (e1000_rx_desc_t *)kmalloc_align(
        sizeof(e1000_rx_desc_t) * E1000_RX_RING_SIZE, 16);
    if (!dev->rx_descs) return -1;

    for (i = 0; i < E1000_RX_RING_SIZE; i++) {
        dev->rx_buffers[i] = (uint8_t *)kmalloc_align(E1000_RX_BUFFER_SIZE, 16);
        if (!dev->rx_buffers[i]) return -1;

        dev->rx_descs[i].addr   = virt_to_phys(dev->rx_buffers[i]);
        dev->rx_descs[i].length = 0;
        dev->rx_descs[i].status = 0;
    }
    dev->rx_tail = 0;

    uint64_t rx_phys = virt_to_phys(dev->rx_descs);
    mmio_write32(base, E1000_RDBAL, (uint32_t)(rx_phys & 0xFFFFFFFF));
    mmio_write32(base, E1000_RDBAH, (uint32_t)(rx_phys >> 32));
    mmio_write32(base, E1000_RDLEN, sizeof(e1000_rx_desc_t) * E1000_RX_RING_SIZE);
    mmio_write32(base, E1000_RDH,   0);
    mmio_write32(base, E1000_RDT,   E1000_RX_RING_SIZE - 1);

    return 0;
}

/* ============================================================
 * PCI 探测 — 使用共享 Rust PCI 原语 (pci_read_config_* / pci_write_config_*)
 * ============================================================ */
int e1000_probe(void)
{
    extern uint16_t pci_read_config_word(uint8_t bus, uint8_t dev, uint8_t func, uint8_t offset);
    extern uint32_t pci_read_config_dword(uint8_t bus, uint8_t dev, uint8_t func, uint8_t offset);
    extern void     pci_write_config_dword(uint8_t bus, uint8_t dev, uint8_t func, uint8_t offset, uint32_t val);

    uint8_t bus, dev_idx, func;
    klog_drv("E1000: PCI device scan using Rust PCI primitives...");

    for (bus = 0; bus < 255; bus++) {
        uint16_t bus_vendor = pci_read_config_word(bus, 0, 0, 0x00);
        if (bus_vendor == 0xFFFF || bus_vendor == 0x0000) {
            if (bus > 0) continue;
        }

        for (dev_idx = 0; dev_idx < 32; dev_idx++) {
            for (func = 0; func < 8; func++) {
                uint16_t vendor_id = pci_read_config_word(bus, dev_idx, func, 0x00);

                if (vendor_id == 0xFFFF || vendor_id == 0x0000) {
                    if (func == 0) break;
                    continue;
                }

                uint16_t device_id = pci_read_config_word(bus, dev_idx, func, 0x02);
                uint32_t class_code = pci_read_config_dword(bus, dev_idx, func, 0x08);
                uint8_t  base_class = (uint8_t)(class_code >> 24);

                if (vendor_id == 0x8086 && base_class == 0x02) {
                    char buf[128];
                    snprintf(buf, sizeof(buf),
                             "E1000: Found NIC vendor=8086 dev=0x%x bus=%d slot=%d",
                             device_id, bus, dev_idx);
                    klog_drv("%s", buf);

                    g_e1000.bus    = bus;
                    g_e1000.device = dev_idx;
                    g_e1000.func   = func;

                    uint32_t bar0lo = pci_read_config_dword(bus, dev_idx, func, 0x10);
                    pci_write_config_dword(bus, dev_idx, func, 0x10, 0xFFFFFFFF);
                    uint32_t bar_size_mask = pci_read_config_dword(bus, dev_idx, func, 0x10);
                    pci_write_config_dword(bus, dev_idx, func, 0x10, bar0lo);

                    uint64_t bar0_phys;
                    uint64_t bar0_size;
                    int bar_is_io = (bar0lo & 0x01);

                    if (bar_is_io) {
                        bar0_phys = bar0lo & ~0x03;
                        bar0_size = ~(bar_size_mask & ~0x03) + 1;
                    } else {
                        bar0_phys = bar0lo & ~0x0F;
                        bar0_size = ~(bar_size_mask & ~0x0F) + 1;
                    }

                    snprintf(buf, sizeof(buf),
                             "E1000: BAR0 phys=0x%x size=%u (%s)",
                             (uint32_t)bar0_phys, (uint32_t)bar0_size,
                             bar_is_io ? "IO" : "MMIO");
                    klog_drv("%s", buf);

                    if (bar_is_io) {
                        klog_drv_err("E1000: I/O BAR not supported");
                        return -1;
                    }

                    g_e1000.mmio_phys = bar0_phys;

                    /* 动态页表映射: CR3 → PML4 → pdpt_low[3] 映射 3-4GB */
                    {
                        volatile uint64_t *pml4;
                        volatile uint64_t *pdpt_low;
                        volatile uint64_t *pd_new;

                        __asm__ volatile("mov %%cr3, %0" : "=r"(pml4));
                        pdpt_low = (volatile uint64_t *)((uint64_t)(uintptr_t)pml4 + 0x1000);

                        pd_new = (volatile uint64_t *)kmalloc_align(4096, 4096);
                        if (!pd_new) {
                            klog_drv_err("E1000: Failed to alloc page table");
                            return -1;
                        }

                        for (int pi = 0; pi < 512; pi++) pd_new[pi] = 0;

                        uint64_t mmio_base_2m = bar0_phys & ~0x1FFFFFULL;
                        int pd_idx = (int)((bar0_phys >> 21) & 0x1FF);
                        pd_new[pd_idx] = mmio_base_2m | 0x93;
                        if (pd_idx < 511) {
                            pd_new[pd_idx + 1] = (mmio_base_2m + 0x200000ULL) | 0x93;
                        }

                        uint64_t pd_phys = virt_to_phys((void *)pd_new);
                        pdpt_low[3] = pd_phys | 0x03;

                        {
                            uint64_t cr3_val;
                            __asm__ volatile("mov %%cr3, %0; mov %0, %%cr3"
                                             : "=r"(cr3_val) :: "memory");
                        }

                        g_e1000.mmio_base = (volatile uint8_t *)(uintptr_t)bar0_phys;
                    }

                    {
                        uint32_t cmd_reg = pci_read_config_dword(bus, dev_idx, func, 0x04);
                        cmd_reg |= 0x06;
                        pci_write_config_dword(bus, dev_idx, func, 0x04, cmd_reg);
                    }

                    g_e1000.irq = (uint8_t)pci_read_config_dword(bus, dev_idx, func, 0x3C);

                    e1000_read_mac(g_e1000.mmio_base, g_e1000.mac);
                    snprintf(buf, sizeof(buf),
                             "E1000: MAC %02x:%02x:%02x:%02x:%02x:%02x IRQ %d",
                             g_e1000.mac[0], g_e1000.mac[1], g_e1000.mac[2],
                             g_e1000.mac[3], g_e1000.mac[4], g_e1000.mac[5],
                             g_e1000.irq);
                    klog_drv("%s", buf);

                    return 0;
                }

                if (func == 0 && !(vendor_id & 0x8000)) {
                    break;
                }
            }
        }
    }

    klog_drv_err("E1000: No Intel NIC found on PCI bus");
    return -1;
}

/* ============================================================
 * 网卡初始化 (配置寄存器 + 启动)
 * ============================================================ */
err_t e1000_init(struct netif *netif)
{
    volatile uint8_t *base = g_e1000.mmio_base;
    if (!base) return ERR_IF;

    g_e1000.netif = netif;

    mmio_write32(base, E1000_CTRL, E1000_CTRL_RST);
    for (volatile int i = 0; i < 100000; i++) { __asm__ volatile("pause"); }

    mmio_write32(base, E1000_IMC, 0xFFFFFFFF);

    uint32_t ctrl = mmio_read32(base, E1000_CTRL);
    ctrl |= E1000_CTRL_SLU;
    ctrl |= E1000_CTRL_ASDE;
    ctrl |= E1000_CTRL_FRCSPD | E1000_CTRL_SPEED_1000;
    ctrl |= E1000_CTRL_FRCDPX | E1000_CTRL_FD;
    mmio_write32(base, E1000_CTRL, ctrl);

    klog_drv("E1000: Waiting for link...");
    for (volatile int i = 0; i < 500000; i++) {
        if (mmio_read32(base, E1000_STATUS) & E1000_STATUS_LU) break;
        __asm__ volatile("pause");
    }
    if (mmio_read32(base, E1000_STATUS) & E1000_STATUS_LU) {
        uint32_t spd = mmio_read32(base, E1000_STATUS) & (3 << 6);
        const char *speed_str;
        if (spd == E1000_STATUS_SPEED_1000) speed_str = "1000Mbps";
        else if (spd == E1000_STATUS_SPEED_100) speed_str = "100Mbps";
        else speed_str = "10Mbps";
        int fd = (mmio_read32(base, E1000_STATUS) & E1000_STATUS_FD) ? 1 : 0;
        klog_drv("E1000: Link up! %s %s", speed_str, fd ? "Full-Duplex" : "Half-Duplex");
    } else {
        klog_drv_warn("E1000: Link not detected");
    }

    if (e1000_setup_rings(&g_e1000) != 0) {
        klog_drv_err("E1000: Failed to setup descriptor rings");
        return ERR_MEM;
    }

    uint32_t rctl = E1000_RCTL_EN
                  | E1000_RCTL_SBP
                  | E1000_RCTL_UPE
                  | E1000_RCTL_MPE
                  | E1000_RCTL_BAM
                  | E1000_RCTL_SECRC
                  | E1000_RCTL_BSIZE_2048;
    mmio_write32(base, E1000_RCTL, rctl);

    uint32_t tctl = E1000_TCTL_EN
                  | E1000_TCTL_PSP
                  | E1000_TCTL_CT(0x10)
                  | E1000_TCTL_COLD(0x40);
    mmio_write32(base, E1000_TCTL, tctl);

    mmio_write32(base, E1000_TIPG, 0x0060200A);

    netif->hwaddr_len = 6;
    for (int i = 0; i < 6; i++) {
        netif->hwaddr[i] = g_e1000.mac[i];
    }
    netif->mtu = 1500;
    netif->flags = NETIF_FLAG_BROADCAST | NETIF_FLAG_ETHARP | NETIF_FLAG_LINK_UP;
    netif->output = etharp_output;
    netif->linkoutput = e1000_send;

    mmio_write32(base, E1000_IMS, E1000_ICR_RXT0 | E1000_ICR_RXDMT0 | E1000_ICR_LSC);

    {
        extern void e1000_irq_entry(struct interrupt_frame *);
        idt_register_irq(g_e1000.irq, e1000_irq_entry, "e1000", 0);
        idt_enable_irq(g_e1000.irq);
    }

    klog_drv("E1000: IRQ %d enabled via IDT (IOAPIC/PIC auto-routed), init complete", g_e1000.irq);
    return ERR_OK;
}

void e1000_irq_entry(struct interrupt_frame *frame)
{
    (void)frame;
    e1000_isr();
}

err_t e1000_send(struct netif *netif, struct pbuf *p)
{
    (void)netif;

    volatile uint8_t *base = g_e1000.mmio_base;
    if (!base) return ERR_IF;

    uint16_t tail = g_e1000.tx_tail;
    e1000_tx_desc_t *desc = &g_e1000.tx_descs[tail];

    if (!desc) return ERR_OK;

    int timeout = 100000;
    while (!(desc->status & E1000_TXD_STAT_DD) && timeout > 0) {
        __asm__ volatile("pause");
        timeout--;
    }
    if (timeout == 0) {
        klog_drv_err("E1000: TX timeout");
        return ERR_TIMEOUT;
    }

    static uint8_t tx_buf[2048];
    size_t total_len = 0;
    struct pbuf *q;
    for (q = p; q != NULL; q = q->next) {
        for (size_t i = 0; i < q->len && total_len < sizeof(tx_buf); i++) {
            tx_buf[total_len++] = ((uint8_t *)q->payload)[i];
        }
    }

    desc->addr   = virt_to_phys(tx_buf);
    desc->length = (uint16_t)total_len;
    desc->cmd    = E1000_TXD_CMD_EOP | E1000_TXD_CMD_IFCS | E1000_TXD_CMD_RS;
    desc->status = 0;

    g_e1000.tx_tail = (tail + 1) % E1000_TX_RING_SIZE;
    mmio_write32(base, E1000_TDT, g_e1000.tx_tail);

    g_e1000.tx_count++;

    if (g_e1000.tx_count <= 5) {
        klog_drv("E1000: TX #%lu len=%d", g_e1000.tx_count, (int)total_len);
    }

    return ERR_OK;
}

static void e1000_rx_process(void)
{
    volatile uint8_t *base = g_e1000.mmio_base;
    uint32_t rdh = mmio_read32(base, E1000_RDH);

    while (g_e1000.rx_tail != rdh) {
        e1000_rx_desc_t *desc = &g_e1000.rx_descs[g_e1000.rx_tail];

        if (desc->status & E1000_RXD_STAT_DD) {
            uint16_t len = desc->length;

            if (!(desc->errors & (E1000_RXD_ERR_CE | E1000_RXD_ERR_SE |
                                   E1000_RXD_ERR_SEQ | E1000_RXD_ERR_RXE))) {
                struct pbuf *p = pbuf_alloc(PBUF_RAW, len, PBUF_POOL);
                if (p) {
                    pbuf_take(p, g_e1000.rx_buffers[g_e1000.rx_tail], len);
                    if (g_e1000.netif) {
                        if (g_e1000.netif->input(p, g_e1000.netif) != ERR_OK) {
                            pbuf_free(p);
                        }
                    } else {
                        pbuf_free(p);
                    }
                }
            }

            desc->status = 0;
            g_e1000.rx_count++;
        }

        uint16_t prev = g_e1000.rx_tail;
        g_e1000.rx_tail = (g_e1000.rx_tail + 1) % E1000_RX_RING_SIZE;
        mmio_write32(base, E1000_RDT, prev);

        rdh = mmio_read32(base, E1000_RDH);
    }
}

void e1000_isr(void)
{
    volatile uint8_t *base = g_e1000.mmio_base;
    if (!base) return;

    uint32_t icr = mmio_read32(base, E1000_ICR);
    if (icr == 0) return;

    g_e1000.isr_count++;

    if (g_e1000.isr_count <= 3) {
        klog_drv("E1000 ISR: ICR=0x%x (#%lu)", icr, g_e1000.isr_count);
    }

    if (icr & E1000_ICR_LSC) {
        g_e1000.link_change_count++;
        uint32_t status = mmio_read32(base, E1000_STATUS);
        if (status & E1000_STATUS_LU) {
            if (g_e1000.netif && !(g_e1000.netif->flags & NETIF_FLAG_LINK_UP)) {
                klog_drv("E1000: Link up");
                g_e1000.netif->flags |= NETIF_FLAG_LINK_UP;
            }
        } else {
            if (g_e1000.netif && (g_e1000.netif->flags & NETIF_FLAG_LINK_UP)) {
                klog_drv("E1000: Link down");
                g_e1000.netif->flags &= ~NETIF_FLAG_LINK_UP;
            }
        }
    }

    if (icr & (E1000_ICR_RXT0 | E1000_ICR_RXDMT0)) {
        e1000_rx_process();
    }
}

void e1000_poll(void)
{
    volatile uint8_t *base = g_e1000.mmio_base;
    if (!base) return;

    uint32_t icr = mmio_read32(base, E1000_ICR);
    if (icr & E1000_ICR_LSC) {
        g_e1000.link_change_count++;
        uint32_t status = mmio_read32(base, E1000_STATUS);
        if (status & E1000_STATUS_LU) {
            if (g_e1000.netif && !(g_e1000.netif->flags & NETIF_FLAG_LINK_UP)) {
                klog_drv("E1000: Link up (poll)");
                g_e1000.netif->flags |= NETIF_FLAG_LINK_UP;
            }
        } else {
            if (g_e1000.netif && (g_e1000.netif->flags & NETIF_FLAG_LINK_UP)) {
                klog_drv("E1000: Link down (poll)");
                g_e1000.netif->flags &= ~NETIF_FLAG_LINK_UP;
            }
        }
    }

    e1000_rx_process();
}

void e1000_dump_stats(void)
{
    klog_drv("E1000 Stats: ISR=%lu RX=%lu TX=%lu LinkChg=%lu",
             g_e1000.isr_count, g_e1000.rx_count,
             g_e1000.tx_count, g_e1000.link_change_count);
}
