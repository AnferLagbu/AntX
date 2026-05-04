/* ============================================================
 * qx_netif.c — QX lwIP 网络接口适配层
 *
 * 将 E1000 网卡驱动挂载到 lwIP netif 管理器
 * ============================================================ */

#include "lwipopts.h"
#include "lwip/opt.h"
#include "lwip/netif.h"
#include "lwip/ip_addr.h"
#include "lwip/dhcp.h"
#include "lwip/tcpip.h"
#include "lwip/init.h"

#include "e1000.h"
#include "serial.h"

static struct netif g_qx_netif;

int qx_netif_register_e1000(void)
{
    serial_puts(SERIAL_COM1, "[NETIF] Registering E1000 as lwIP netif...\n");

    if (!g_e1000.mmio_base) {
        serial_puts(SERIAL_COM1, "[NETIF] E1000 not probed\n");
        return -1;
    }

    /* 添加网络接口 */
    struct netif *netif = netif_add(&g_qx_netif,
                                     NULL,  /* IP (DHCP 自动分配) */
                                     NULL,  /* netmask */
                                     NULL,  /* gateway */
                                     NULL,  /* state */
                                     e1000_init,
                                     tcpip_input);

    if (!netif) {
        serial_puts(SERIAL_COM1, "[NETIF] netif_add failed\n");
        return -1;
    }

    netif_set_default(netif);
    netif_set_up(netif);

    serial_puts(SERIAL_COM1, "[NETIF] E1000 registered as default netif\n");

    /* 启动 DHCP */
    serial_puts(SERIAL_COM1, "[NETIF] Starting DHCP...\n");
    dhcp_start(netif);

    return 0;
}
