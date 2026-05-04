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
#include "serial.h"
#include "pci.h"
#include "dma.h"
#include "kmalloc.h"
#include "types.h"

#include "lwip/opt.h"
#include "lwip/pbuf.h"
#include "lwip/netif.h"
#include "lwip/etharp.h"
#include "lwip/def.h"
#include "lwip/err.h"

e1000_dev_t g_e1000 = {0};

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
        dev->tx_descs[i].status = E1000_TXD_STAT_DD;  /* 标记为可用 */
    }
    dev->tx_tail = 0;

    /* 配置 TX 描述符环 */
    uint64_t tx_phys = (uint64_t)(uintptr_t)dev->tx_descs;
    mmio_write32(base, E1000_TDBAL, (uint32_t)(tx_phys & 0xFFFFFFFF));
    mmio_write32(base, E1000_TDBAH, (uint32_t)(tx_phys >> 32));
    mmio_write32(base, E1000_TDLEN, sizeof(e1000_tx_desc_t) * E1000_TX_RING_SIZE);
    mmio_write32(base, E1000_TDH,   0);
    mmio_write32(base, E1000_TDT,   0);

    /* 分配 RX 描述符 (16字节对齐) */
    dev->rx_descs = (e1000_rx_desc_t *)kmalloc_align(
        sizeof(e1000_rx_desc_t) * E1000_RX_RING_SIZE, 16);
    if (!dev->rx_descs) return -1;

    for (i = 0; i < E1000_RX_RING_SIZE; i++) {
        dev->rx_buffers[i] = (uint8_t *)kmalloc_align(E1000_RX_BUFFER_SIZE, 16);
        if (!dev->rx_buffers[i]) return -1;

        dev->rx_descs[i].addr   = (uint64_t)(uintptr_t)dev->rx_buffers[i];
        dev->rx_descs[i].length = 0;
        dev->rx_descs[i].status = 0;
    }
    dev->rx_tail = 0;

    /* 配置 RX 描述符环 */
    uint64_t rx_phys = (uint64_t)(uintptr_t)dev->rx_descs;
    mmio_write32(base, E1000_RDBAL, (uint32_t)(rx_phys & 0xFFFFFFFF));
    mmio_write32(base, E1000_RDBAH, (uint32_t)(rx_phys >> 32));
    mmio_write32(base, E1000_RDLEN, sizeof(e1000_rx_desc_t) * E1000_RX_RING_SIZE);
    mmio_write32(base, E1000_RDH,   0);
    mmio_write32(base, E1000_RDT,   E1000_RX_RING_SIZE - 1);

    return 0;
}

/* ============================================================
 * PCI 探测
 * ============================================================ */
int e1000_probe(void)
{
    pci_device_t *dev;
    serial_puts(SERIAL_COM1, "[E1000] Probing PCI bus for Intel 82540EM...\n");

    /* 搜索网络类以太网子类的 PCI 设备 */
    dev = pci_find_class(0x02, 0x00, NULL);
    if (!dev) {
        serial_puts(SERIAL_COM1, "[E1000] No network PCI device found\n");
        return -1;
    }

    /* 验证是 Intel 82540EM (QEMU 默认) */
    if (dev->vendor_id != 0x8086) {
        serial_puts(SERIAL_COM1, "[E1000] Not an Intel NIC (vendor=");
        serial_put_hex(SERIAL_COM1, dev->vendor_id);
        serial_puts(SERIAL_COM1, ")\n");
        return -1;
    }

    serial_puts(SERIAL_COM1, "[E1000] Found Intel NIC: ");
    serial_put_hex(SERIAL_COM1, dev->vendor_id);
    serial_puts(SERIAL_COM1, ":");
    serial_put_hex(SERIAL_COM1, dev->device_id);
    serial_puts(SERIAL_COM1, " at bus=");
    serial_put_dec(SERIAL_COM1, dev->bus);
    serial_puts(SERIAL_COM1, " dev=");
    serial_put_dec(SERIAL_COM1, dev->device);
    serial_puts(SERIAL_COM1, " func=");
    serial_put_dec(SERIAL_COM1, dev->function);
    serial_putc(SERIAL_COM1, '\n');

    g_e1000.bus    = dev->bus;
    g_e1000.device = dev->device;
    g_e1000.func   = dev->function;

    /* BAR0 应为 MMIO */
    if (dev->bars[0].type != PCI_BAR_MEMORY_32 &&
        dev->bars[0].type != PCI_BAR_MEMORY_64) {
        serial_puts(SERIAL_COM1, "[E1000] BAR0 is not MMIO\n");
        return -1;
    }

    g_e1000.mmio_phys = dev->bars[0].base_addr;
    serial_puts(SERIAL_COM1, "[E1000] BAR0 phys=0x");
    serial_put_hex(SERIAL_COM1, (uint32_t)g_e1000.mmio_phys);
    serial_puts(SERIAL_COM1, " size=");
    serial_put_dec(SERIAL_COM1, (uint32_t)dev->bars[0].size);
    serial_putc(SERIAL_COM1, '\n');

    /* MMIO 映射 */
    g_e1000.mmio_base = (volatile uint8_t *)ioremap(
        g_e1000.mmio_phys, (size_t)dev->bars[0].size);
    if (!g_e1000.mmio_base) {
        serial_puts(SERIAL_COM1, "[E1000] ioremap failed\n");
        return -1;
    }

    serial_puts(SERIAL_COM1, "[E1000] MMIO mapped at 0x");
    serial_put_hex(SERIAL_COM1, (uint64_t)(uintptr_t)g_e1000.mmio_base);
    serial_putc(SERIAL_COM1, '\n');

    /* 读取 MAC */
    e1000_read_mac(g_e1000.mmio_base, g_e1000.mac);
    serial_puts(SERIAL_COM1, "[E1000] MAC: ");
    serial_put_hex(SERIAL_COM1, g_e1000.mac[0]);
    serial_putc(SERIAL_COM1, ':');
    serial_put_hex(SERIAL_COM1, g_e1000.mac[1]);
    serial_putc(SERIAL_COM1, ':');
    serial_put_hex(SERIAL_COM1, g_e1000.mac[2]);
    serial_putc(SERIAL_COM1, ':');
    serial_put_hex(SERIAL_COM1, g_e1000.mac[3]);
    serial_putc(SERIAL_COM1, ':');
    serial_put_hex(SERIAL_COM1, g_e1000.mac[4]);
    serial_putc(SERIAL_COM1, ':');
    serial_put_hex(SERIAL_COM1, g_e1000.mac[5]);
    serial_putc(SERIAL_COM1, '\n');

    /* IRQ 分配 */
    pci_enable_bus_master(dev);
    g_e1000.irq = dev->interrupt_line;
    serial_puts(SERIAL_COM1, "[E1000] IRQ=");
    serial_put_dec(SERIAL_COM1, g_e1000.irq);
    serial_putc(SERIAL_COM1, '\n');

    return 0;
}

/* ============================================================
 * 网卡初始化 (配置寄存器 + 启动)
 * ============================================================ */
err_t e1000_init(struct netif *netif)
{
    volatile uint8_t *base = g_e1000.mmio_base;
    if (!base) return ERR_IF;

    g_e1000.netif = netif;

    /* 1. 复位设备 */
    mmio_write32(base, E1000_CTRL, E1000_CTRL_RST);
    for (volatile int i = 0; i < 100000; i++) { __asm__ volatile("pause"); }
    /* CTRL.RST 自动清除 */

    /* 2. 禁用中断 */
    mmio_write32(base, E1000_IMC, 0xFFFFFFFF);

    /* 3. 设置链路: 1000M 全双工, 开启链路 */
    uint32_t ctrl = mmio_read32(base, E1000_CTRL);
    ctrl |= E1000_CTRL_SLU;
    ctrl |= E1000_CTRL_ASDE;
    ctrl |= E1000_CTRL_FRCSPD | E1000_CTRL_SPEED_1000;
    ctrl |= E1000_CTRL_FRCDPX | E1000_CTRL_FD;
    mmio_write32(base, E1000_CTRL, ctrl);

    /* 4. 等待链路就绪 */
    serial_puts(SERIAL_COM1, "[E1000] Waiting for link...\n");
    for (volatile int i = 0; i < 500000; i++) {
        if (mmio_read32(base, E1000_STATUS) & E1000_STATUS_LU) break;
        __asm__ volatile("pause");
    }
    if (mmio_read32(base, E1000_STATUS) & E1000_STATUS_LU) {
        serial_puts(SERIAL_COM1, "[E1000] Link up! speed=");
        uint32_t spd = mmio_read32(base, E1000_STATUS) & (3 << 6);
        if (spd == E1000_STATUS_SPEED_1000) serial_puts(SERIAL_COM1, "1000Mbps");
        else if (spd == E1000_STATUS_SPEED_100) serial_puts(SERIAL_COM1, "100Mbps");
        else serial_puts(SERIAL_COM1, "10Mbps");
        if (mmio_read32(base, E1000_STATUS) & E1000_STATUS_FD)
            serial_puts(SERIAL_COM1, " Full-Duplex");
        serial_putc(SERIAL_COM1, '\n');
    } else {
        serial_puts(SERIAL_COM1, "[E1000] Warning: link not detected\n");
    }

    /* 5. 设置描述符环 */
    if (e1000_setup_rings(&g_e1000) != 0) {
        serial_puts(SERIAL_COM1, "[E1000] Failed to setup descriptor rings\n");
        return ERR_MEM;
    }

    /* 6. 配置接收控制 */
    uint32_t rctl = E1000_RCTL_EN
                  | E1000_RCTL_SBP
                  | E1000_RCTL_UPE
                  | E1000_RCTL_MPE
                  | E1000_RCTL_BAM
                  | E1000_RCTL_SECRC
                  | E1000_RCTL_BSIZE_2048;
    mmio_write32(base, E1000_RCTL, rctl);

    /* 7. 配置发送控制 */
    uint32_t tctl = E1000_TCTL_EN
                  | E1000_TCTL_PSP
                  | E1000_TCTL_CT(0x10)
                  | E1000_TCTL_COLD(0x40);
    mmio_write32(base, E1000_TCTL, tctl);

    /* 8. 帧间间隔 */
    mmio_write32(base, E1000_TIPG, 0x0060200A);

    /* 9. 设置 netif */
    netif->hwaddr_len = 6;
    for (int i = 0; i < 6; i++) {
        netif->hwaddr[i] = g_e1000.mac[i];
    }
    netif->mtu = 1500;
    netif->flags = NETIF_FLAG_BROADCAST | NETIF_FLAG_ETHARP | NETIF_FLAG_LINK_UP;
    netif->output = etharp_output;
    netif->linkoutput = e1000_send;

    /* 10. 启用 RX 中断 */
    mmio_write32(base, E1000_IMS, E1000_ICR_RXT0 | E1000_ICR_RXDMT0 | E1000_ICR_LSC);

    serial_puts(SERIAL_COM1, "[E1000] Initialization complete\n");
    return ERR_OK;
}

/* ============================================================
 * 发送数据包
 * ============================================================ */
err_t e1000_send(struct netif *netif, struct pbuf *p)
{
    (void)netif;

    volatile uint8_t *base = g_e1000.mmio_base;
    uint16_t tail = g_e1000.tx_tail;
    e1000_tx_desc_t *desc = &g_e1000.tx_descs[tail];

    if (!desc) return ERR_OK;

    /* 等待上一个描述符完成 */
    int timeout = 100000;
    while (!(desc->status & E1000_TXD_STAT_DD) && timeout > 0) {
        __asm__ volatile("pause");
        timeout--;
    }
    if (timeout == 0) {
        serial_puts(SERIAL_COM1, "[E1000] TX timeout\n");
        return ERR_TIMEOUT;
    }

    /* 将 pbuf 链拷入连续缓冲区 */
    static uint8_t tx_buf[2048];
    size_t total_len = 0;
    struct pbuf *q;
    for (q = p; q != NULL; q = q->next) {
        for (size_t i = 0; i < q->len && total_len < sizeof(tx_buf); i++) {
            tx_buf[total_len++] = ((uint8_t *)q->payload)[i];
        }
    }

    desc->addr   = (uint64_t)(uintptr_t)tx_buf;
    desc->length = (uint16_t)total_len;
    desc->cmd    = E1000_TXD_CMD_EOP | E1000_TXD_CMD_IFCS | E1000_TXD_CMD_RS;
    desc->status = 0;

    /* 推进尾指针 */
    g_e1000.tx_tail = (tail + 1) % E1000_TX_RING_SIZE;
    mmio_write32(base, E1000_TDT, g_e1000.tx_tail);

    return ERR_OK;
}

/* ============================================================
 * 中断处理
 * ============================================================ */
void e1000_isr(void)
{
    volatile uint8_t *base = g_e1000.mmio_base;
    if (!base) return;

    uint32_t icr = mmio_read32(base, E1000_ICR);
    if (icr == 0) return;  /* 不是我们的中断 */

    /* 链路状态改变 */
    if (icr & E1000_ICR_LSC) {
        uint32_t status = mmio_read32(base, E1000_STATUS);
        if (status & E1000_STATUS_LU) {
            if (g_e1000.netif && !(g_e1000.netif->flags & NETIF_FLAG_LINK_UP)) {
                serial_puts(SERIAL_COM1, "[E1000] Link up!\n");
                g_e1000.netif->flags |= NETIF_FLAG_LINK_UP;
            }
        } else {
            if (g_e1000.netif && (g_e1000.netif->flags & NETIF_FLAG_LINK_UP)) {
                serial_puts(SERIAL_COM1, "[E1000] Link down!\n");
                g_e1000.netif->flags &= ~NETIF_FLAG_LINK_UP;
            }
        }
    }

    /* 接收数据包 */
    if (icr & (E1000_ICR_RXT0 | E1000_ICR_RXDMT0)) {
        uint32_t rdh = mmio_read32(base, E1000_RDH);
        while (g_e1000.rx_tail != rdh) {
            e1000_rx_desc_t *desc = &g_e1000.rx_descs[g_e1000.rx_tail];

            if (desc->status & E1000_RXD_STAT_DD) {
                uint16_t len = desc->length;

                if (!(desc->errors & (E1000_RXD_ERR_CE | E1000_RXD_ERR_SE |
                                       E1000_RXD_ERR_SEQ | E1000_RXD_ERR_RXE))) {
                    /* 将数据提交给 lwIP */
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
            }

            /* 推进 RX 尾指针 */
            uint16_t prev = g_e1000.rx_tail;
            g_e1000.rx_tail = (g_e1000.rx_tail + 1) % E1000_RX_RING_SIZE;
            mmio_write32(base, E1000_RDT, prev);

            rdh = mmio_read32(base, E1000_RDH);
        }
    }
}
