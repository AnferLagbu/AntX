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
#include "klog.h"

static struct netif g_qx_netif;

static void qx_netif_status(struct netif *netif)
{
    uint32_t ip = netif->ip_addr.u_addr.ip4.addr;
    uint8_t *b = (uint8_t *)&ip;
    const char *state = netif_is_up(netif) ? "UP" : "DOWN";

    klog_net("netif status: %s IP=%d.%d.%d.%d", state, b[0], b[1], b[2], b[3]);
}

int qx_netif_register_e1000(void)
{
    klog_net("Registering E1000 as lwIP netif");

    if (!g_e1000.mmio_base) {
        klog_net_err("E1000 not probed");
        return -1;
    }

    struct netif *netif = netif_add(&g_qx_netif,
                                     NULL, NULL, NULL, NULL,
                                     e1000_init,
                                     ethernet_input);

    if (!netif) {
        klog_net_err("netif_add failed");
        return -1;
    }

    netif_set_default(netif);
    netif_set_up(netif);
    netif_set_status_callback(netif, qx_netif_status);

    klog_net("E1000 registered as default netif, starting DHCP");
    dhcp_start(netif);

    return 0;
}
