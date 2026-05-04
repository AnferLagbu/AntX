/* ============================================================
 * qx_netif.c — QX lwIP 网络接口适配层
 *
 * 将 E1000 网卡驱动挂载到 lwIP netif 管理器
 * ============================================================ */

#include "lwipopts.h"
#include "lwip/opt.h"
#include "lwip/netif.h"
#include "lwip/ip_addr.h"
#include "lwip/ip4_addr.h"
#include "lwip/dhcp.h"
#include "lwip/init.h"
#include "lwip/etharp.h"
#include "netif/ethernet.h"

#include "e1000.h"
#include "serial.h"

static struct netif g_qx_netif;

/* DHCP 状态回调 */
static void qx_netif_status(struct netif *netif)
{
    uint32_t ip = netif->ip_addr.u_addr.ip4.addr;
    uint8_t *b = (uint8_t *)&ip;

    serial_puts(SERIAL_COM1, "[NETIF] Status: ");
    if (netif_is_up(netif)) serial_puts(SERIAL_COM1, "UP");
    else serial_puts(SERIAL_COM1, "DOWN");

    serial_puts(SERIAL_COM1, " IP=");
    serial_put_dec(SERIAL_COM1, b[0]);
    serial_putc(SERIAL_COM1, '.');
    serial_put_dec(SERIAL_COM1, b[1]);
    serial_putc(SERIAL_COM1, '.');
    serial_put_dec(SERIAL_COM1, b[2]);
    serial_putc(SERIAL_COM1, '.');
    serial_put_dec(SERIAL_COM1, b[3]);
    serial_putc(SERIAL_COM1, '\n');
}

int qx_netif_register_e1000(void)
{
    serial_puts(SERIAL_COM1, "[NETIF] Registering E1000 as lwIP netif...\n");

    if (!g_e1000.mmio_base) {
        serial_puts(SERIAL_COM1, "[NETIF] E1000 not probed\n");
        return -1;
    }

    /* Raw API: ethernet_input 直接分发接收帧 */
    struct netif *netif = netif_add(&g_qx_netif,
                                     NULL, NULL, NULL, NULL,
                                     e1000_init,
                                     ethernet_input);

    if (!netif) {
        serial_puts(SERIAL_COM1, "[NETIF] netif_add failed\n");
        return -1;
    }

    netif_set_default(netif);
    netif_set_up(netif);
    netif_set_status_callback(netif, qx_netif_status);

    serial_puts(SERIAL_COM1, "[NETIF] E1000 registered as default netif\n");

    /* 启动 DHCP */
    serial_puts(SERIAL_COM1, "[NETIF] Starting DHCP...\n");
    dhcp_start(netif);

    return 0;
}
