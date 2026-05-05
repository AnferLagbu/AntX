/* ============================================================
 * qx_net_apps.c — QX 网络应用层 (HTTP / Ping / DNS)
 *
 * 在 DHCP 成功后初始化 HTTP 服务器、发送 ICMP Ping、
 * 执行 DNS 查询等。
 * ============================================================ */

#include "lwip/opt.h"
#include "lwip/init.h"
#include "lwip/netif.h"
#include "lwip/ip_addr.h"
#include "lwip/ip4_addr.h"
#include "lwip/raw.h"
#include "lwip/inet_chksum.h"
#include "lwip/prot/icmp.h"
#include "lwip/dns.h"
#include "lwip/apps/httpd.h"

#include "klog.h"

#define PING_DATA_SIZE 32
#define ICMP_ECHO_HDR_SIZE 8

/* ============================================================
 * DNS 查询
 * ============================================================ */
static void dns_found(const char *name, const ip_addr_t *addr, void *arg)
{
    (void)arg;
    if (addr) {
        ip4_addr_t a = addr->u_addr.ip4;
        klog_net("DNS: %s → %d.%d.%d.%d", name,
                 ip4_addr1(&a), ip4_addr2(&a),
                 ip4_addr3(&a), ip4_addr4(&a));
    } else {
        klog_net_warn("DNS: %s not found", name);
    }
}

static void dns_test(void)
{
    dns_init();
    klog_net("DNS: resolving example.com...");
    dns_gethostbyname("example.com", NULL, dns_found, NULL);
}

/* ============================================================
 * HTTP 服务器
 * ============================================================ */
static void http_init(void)
{
    httpd_init();
    klog_net("HTTP: server started on port 80");
}

/* ============================================================
 * 统一入口
 * ============================================================ */
void qx_net_apps_init(struct netif *netif)
{
    (void)netif;
    klog_net("Initializing network applications...");
    http_init();
    dns_test();
}
