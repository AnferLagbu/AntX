#include "lwipopts.h"
#include "lwip/opt.h"
#include "lwip/netif.h"
#include "lwip/ip_addr.h"
#include "lwip/ip4_addr.h"
#include "lwip/dhcp.h"
#include "lwip/init.h"
#include "lwip/etharp.h"
#include "lwip/autoip.h"
#include "lwip/nd6.h"
#include "netif/ethernet.h"

#include "e1000.h"
#include "klog.h"

static struct netif g_qx_netif;
static int g_dhcp_done = 0;

static void qx_netif_status(struct netif *netif)
{
    if (ip4_addr_get_u32(ip_2_ip4(&netif->ip_addr)) != 0) {
        const ip4_addr_t *ip4   = ip_2_ip4(&netif->ip_addr);
        const ip4_addr_t *mask4 = ip_2_ip4(&netif->netmask);
        const ip4_addr_t *gw4   = ip_2_ip4(&netif->gw);

        klog_net("Interface up: %d.%d.%d.%d/%d.%d.%d.%d gw=%d.%d.%d.%d",
                 ip4_addr1(ip4), ip4_addr2(ip4),
                 ip4_addr3(ip4), ip4_addr4(ip4),
                 ip4_addr1(mask4), ip4_addr2(mask4),
                 ip4_addr3(mask4), ip4_addr4(mask4),
                 ip4_addr1(gw4), ip4_addr2(gw4),
                 ip4_addr3(gw4), ip4_addr4(gw4));

        if (!g_dhcp_done) {
            g_dhcp_done = 1;
            klog_net("DHCP bound, starting network apps");
            extern void qx_net_apps_init(struct netif *netif);
            qx_net_apps_init(netif);
        }
    } else {
        klog_net("Interface down (IP=0.0.0.0)");
    }
}

#if LWIP_IPV6
static void qx_netif_ipv6_status(struct netif *netif, u8_t addr_idx)
{
    if (ip6_addr_isvalid(netif_ip6_addr_state(netif, addr_idx))) {
        const ip6_addr_t *a = netif_ip6_addr(netif, addr_idx);
        klog_net("IPv6 addr[%d]: %04x:%04x:%04x:%04x:%04x:%04x:%04x:%04x",
                 addr_idx,
                 IP6_ADDR_BLOCK1(a), IP6_ADDR_BLOCK2(a),
                 IP6_ADDR_BLOCK3(a), IP6_ADDR_BLOCK4(a),
                 IP6_ADDR_BLOCK5(a), IP6_ADDR_BLOCK6(a),
                 IP6_ADDR_BLOCK7(a), IP6_ADDR_BLOCK8(a));
    }
}
#endif

int qx_netif_register_e1000(void)
{
    klog_net("Registering E1000 as lwIP netif (DHCP + IPv6)");

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
    netif_set_status_callback(netif, qx_netif_status);
    netif_set_up(netif);

#if LWIP_IPV6
    netif_create_ip6_linklocal_address(netif, 1);
    netif_set_ip6_autoconfig_enabled(netif, 0);
    klog_net("IPv6 link-local address configured");
#endif

    klog_net("Starting DHCP on E1000...");
    err_t dhcp_err = dhcp_start(netif);
    klog_net("dhcp_start() returned %d", (int)dhcp_err);

    klog_net("netif flags=0x%08x hwaddr=%02x:%02x:%02x:%02x:%02x:%02x mtu=%d",
             netif->flags,
             netif->hwaddr[0], netif->hwaddr[1], netif->hwaddr[2],
             netif->hwaddr[3], netif->hwaddr[4], netif->hwaddr[5],
             (int)netif->mtu);

#if LWIP_IPV6
    qx_netif_ipv6_status(netif, 0);
#endif

    return 0;
}
