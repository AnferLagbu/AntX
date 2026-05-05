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

int qx_netif_register_e1000(void)
{
    ip4_addr_t ip, mask, gw;
    klog_net("Registering E1000 as lwIP netif");

    if (!g_e1000.mmio_base) {
        klog_net_err("E1000 not probed");
        return -1;
    }

    IP4_ADDR(&ip,   10, 0, 2, 15);
    IP4_ADDR(&mask, 255, 255, 255, 0);
    IP4_ADDR(&gw,   10, 0, 2, 2);

    struct netif *netif = netif_add(&g_qx_netif,
                                     &ip, &mask, &gw, NULL,
                                     e1000_init,
                                     ethernet_input);

    if (!netif) {
        klog_net_err("netif_add failed");
        return -1;
    }

    netif_set_default(netif);
    netif_set_up(netif);

    klog_net("E1000 static IP 10.0.2.15/24 gw=10.0.2.2");

    extern void qx_net_apps_init(struct netif *netif);
    qx_net_apps_init(netif);

    return 0;
}
