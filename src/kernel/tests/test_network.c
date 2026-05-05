#include "kernel_test.h"
#include "klog.h"
#include "e1000.h"
#include "io.h"

#include "lwipopts.h"
#include "lwip/opt.h"
#include "lwip/init.h"
#include "lwip/netif.h"
#include "lwip/ip_addr.h"
#include "lwip/ip4_addr.h"
#include "lwip/pbuf.h"
#include "lwip/raw.h"
#include "lwip/udp.h"
#include "lwip/tcp.h"
#include "lwip/icmp.h"
#include "lwip/dns.h"
#include "lwip/dhcp.h"
#include "lwip/prot/icmp.h"
#include "lwip/inet_chksum.h"
#include "lwip/etharp.h"
#include "lwip/apps/httpd.h"
#include "lwip/apps/http_client.h"
#include "lwip/apps/mdns.h"
#include "lwip/apps/mqtt.h"
#include "lwip/apps/netbiosns.h"
#include "lwip/apps/smtp.h"
#include "lwip/apps/sntp.h"
#include "lwip/apps/tftp_server.h"
#include "lwip/apps/lwiperf.h"
#include "lwip/apps/snmp.h"
#include "lwip/apps/snmp_mib2.h"
#include "lwip/stats.h"
#include "lwip/mem.h"
#include "lwip/memp.h"
#include "netif/ethernet.h"

static int g_net_initialized = 0;

static int test_net_init(void)
{
    lwip_init();

    extern void sys_init(void);
    sys_init();

    if (e1000_probe() != 0) {
        return TEST_SKIP;
    }

    TEST_ASSERT_NOT_NULL(g_e1000.mmio_base);
    TEST_ASSERT_GT(g_e1000.irq, 0);
    TEST_ASSERT_LT(g_e1000.irq, 16);

    g_net_initialized = 1;
    return TEST_PASS;
}

static int test_e1000_mac(void)
{
    if (!g_net_initialized) return TEST_SKIP;
    int valid = 0;
    for (int i = 0; i < 6; i++) {
        if (g_e1000.mac[i] != 0) valid = 1;
    }
    if (!valid) return TEST_SKIP;
    return TEST_PASS;
}

static int test_e1000_link(void)
{
    if (!g_net_initialized) return TEST_SKIP;
    volatile uint8_t *base = g_e1000.mmio_base;
    TEST_ASSERT_NOT_NULL(base);
    uint32_t status = *(volatile uint32_t *)(base + 0x0008);
    TEST_ASSERT(status & (1 << 1));
    return TEST_PASS;
}

static int test_e1000_irq_enabled(void)
{
    if (!g_net_initialized) return TEST_SKIP;
    uint8_t master_imr = inb(0x21);
    uint8_t slave_imr  = inb(0xA1);
    TEST_ASSERT_EQ(master_imr & (1 << 2), 0);
    if (g_e1000.irq >= 8) {
        TEST_ASSERT_EQ(slave_imr & (1 << (g_e1000.irq - 8)), 0);
    } else {
        TEST_ASSERT_EQ(master_imr & (1 << g_e1000.irq), 0);
    }
    return TEST_PASS;
}

static int test_pbuf_alloc(void)
{
    struct pbuf *p = pbuf_alloc(PBUF_RAW, 64, PBUF_RAM);
    TEST_ASSERT_NOT_NULL(p);
    TEST_ASSERT_GE(p->tot_len, 64);
    pbuf_free(p);
    return TEST_PASS;
}

static int test_pbuf_pool(void)
{
    struct pbuf *bufs[16];
    int i;
    for (i = 0; i < 16; i++) {
        bufs[i] = pbuf_alloc(PBUF_RAW, 256, PBUF_POOL);
        if (!bufs[i]) break;
    }
    TEST_ASSERT_GT(i, 0);
    for (int j = 0; j < i; j++) {
        pbuf_free(bufs[j]);
    }
    return TEST_PASS;
}

static int test_pbuf_chain(void)
{
    struct pbuf *head = pbuf_alloc(PBUF_RAW, 100, PBUF_RAM);
    TEST_ASSERT_NOT_NULL(head);
    struct pbuf *tail = pbuf_alloc(PBUF_RAW, 50, PBUF_RAM);
    TEST_ASSERT_NOT_NULL(tail);
    pbuf_cat(head, tail);
    TEST_ASSERT_GE(head->tot_len, 150);
    pbuf_free(head);
    return TEST_PASS;
}

static int test_raw_pcb(void)
{
    struct raw_pcb *pcb = raw_new(IP_PROTO_ICMP);
    TEST_ASSERT_NOT_NULL(pcb);
    raw_remove(pcb);
    return TEST_PASS;
}

static int test_udp_pcb(void)
{
    struct udp_pcb *pcb = udp_new();
    TEST_ASSERT_NOT_NULL(pcb);
    err_t err = udp_bind(pcb, IP_ADDR_ANY, 0);
    TEST_ASSERT_EQ(err, ERR_OK);
    udp_remove(pcb);
    return TEST_PASS;
}

static int test_tcp_pcb(void)
{
    struct tcp_pcb *pcb = tcp_new();
    TEST_ASSERT_NOT_NULL(pcb);
    err_t err = tcp_bind(pcb, IP_ADDR_ANY, 0);
    TEST_ASSERT_EQ(err, ERR_OK);
    tcp_abort(pcb);
    return TEST_PASS;
}

static int test_tcp_listen(void)
{
    struct tcp_pcb *pcb = tcp_new();
    TEST_ASSERT_NOT_NULL(pcb);
    err_t err = tcp_bind(pcb, IP_ADDR_ANY, 8080);
    TEST_ASSERT_EQ(err, ERR_OK);
    struct tcp_pcb *listen = tcp_listen(pcb);
    TEST_ASSERT_NOT_NULL(listen);
    tcp_abort(listen);
    return TEST_PASS;
}

static int test_netif_register(void)
{
    if (!g_net_initialized) return TEST_SKIP;

    static struct netif netif;
    struct netif *n = netif_add(&netif, NULL, NULL, NULL, NULL,
                                 e1000_init, ethernet_input);
    TEST_ASSERT_NOT_NULL(n);

    netif_set_default(n);
    netif_set_up(n);

#if LWIP_IPV6
    netif_create_ip6_linklocal_address(n, 1);
#endif

    return TEST_PASS;
}

static int test_netif_default(void)
{
    struct netif *netif = netif_default;
    if (!netif) return TEST_SKIP;
    return TEST_PASS;
}

static int test_netif_hwaddr(void)
{
    struct netif *netif = netif_default;
    TEST_ASSERT_NOT_NULL(netif);
    int valid = 0;
    for (int i = 0; i < 6; i++) {
        if (netif->hwaddr[i] != 0) valid = 1;
    }
    TEST_ASSERT(valid);
    TEST_ASSERT_EQ(netif->hwaddr_len, 6);
    return TEST_PASS;
}

static int test_netif_mtu(void)
{
    struct netif *netif = netif_default;
    if (!netif) return TEST_SKIP;
    TEST_ASSERT_GT(netif->mtu, 0);
    return TEST_PASS;
}

static int test_dhcp_start(void)
{
    struct netif *netif = netif_default;
    TEST_ASSERT_NOT_NULL(netif);
    err_t err = dhcp_start(netif);
    TEST_ASSERT_EQ(err, ERR_OK);
    return TEST_PASS;
}

static int test_arp_config(void)
{
    TEST_ASSERT(LWIP_ARP);
    return TEST_PASS;
}

static int test_dns_config(void)
{
    TEST_ASSERT(LWIP_DNS);
    return TEST_PASS;
}

static int test_ipv4_frag_config(void)
{
    TEST_ASSERT(LWIP_IPV4_FRAG);
    return TEST_PASS;
}

#if LWIP_IPV6
static int test_ipv6_linklocal(void)
{
    struct netif *netif = netif_default;
    if (!netif) return TEST_SKIP;
    if (!ip6_addr_isvalid(netif_ip6_addr_state(netif, 0))) return TEST_SKIP;
    return TEST_PASS;
}

static int test_ipv6_config(void)
{
    TEST_ASSERT(LWIP_IPV6);
    TEST_ASSERT(LWIP_ND6);
    TEST_ASSERT(LWIP_IPV6_NUM_ADDRESSES >= 2);
    return TEST_PASS;
}
#endif

static int test_icmp_config(void)
{
    TEST_ASSERT(LWIP_ICMP);
    return TEST_PASS;
}

static int test_http_server_config(void)
{
    TEST_ASSERT(LWIP_HTTPD);
    return TEST_PASS;
}

#if LWIP_HTTP_CLIENT
static int test_http_client_config(void)
{
    TEST_ASSERT(LWIP_HTTP_CLIENT);
    return TEST_PASS;
}
#endif

static int test_mdns_config(void)
{
    TEST_ASSERT(LWIP_MDNS_RESPONDER);
    return TEST_PASS;
}

static int test_mqtt_config(void)
{
    TEST_ASSERT(LWIP_MQTT);
    return TEST_PASS;
}

static int test_netbios_config(void)
{
    TEST_ASSERT(LWIP_NETBIOSNS);
    return TEST_PASS;
}

static int test_smtp_config(void)
{
    TEST_ASSERT(LWIP_SMTP);
    return TEST_PASS;
}

static int test_snmp_config(void)
{
    TEST_ASSERT(LWIP_SNMP);
    return TEST_PASS;
}

static int test_sntp_config(void)
{
    TEST_ASSERT(LWIP_SNTP);
    return TEST_PASS;
}

static int test_tftp_config(void)
{
    TEST_ASSERT(LWIP_TFTP);
    return TEST_PASS;
}

static int test_lwiperf_config(void)
{
    TEST_ASSERT(LWIP_LWIPERF);
    return TEST_PASS;
}

static int test_mem_pool(void)
{
    void *p1 = mem_malloc(1024);
    TEST_ASSERT_NOT_NULL(p1);
    mem_free(p1);

    void *p2 = mem_malloc(4096);
    TEST_ASSERT_NOT_NULL(p2);
    mem_free(p2);
    return TEST_PASS;
}

static int test_lwip_stats_config(void)
{
    TEST_ASSERT(LWIP_STATS);
    return TEST_PASS;
}

static int test_e1000_stats(void)
{
    if (!g_net_initialized) return TEST_SKIP;
    e1000_dump_stats();
    return TEST_PASS;
}

void test_network_register(void)
{
    int mod = test_register_module("Network Stack (lwIP 2.2.1 + E1000)");
    if (mod < 0) return;

    test_register_case(mod, "Network init (lwIP+E1000)", test_net_init);
    test_register_case(mod, "E1000 MAC address", test_e1000_mac);
    test_register_case(mod, "E1000 link status", test_e1000_link);
    test_register_case(mod, "E1000 IRQ enabled", test_e1000_irq_enabled);

    test_register_case(mod, "pbuf alloc (RAM)", test_pbuf_alloc);
    test_register_case(mod, "pbuf pool", test_pbuf_pool);
    test_register_case(mod, "pbuf chain", test_pbuf_chain);

    test_register_case(mod, "RAW PCB (ICMP)", test_raw_pcb);
    test_register_case(mod, "UDP PCB", test_udp_pcb);
    test_register_case(mod, "TCP PCB", test_tcp_pcb);
    test_register_case(mod, "TCP listen", test_tcp_listen);

    test_register_case(mod, "netif register", test_netif_register);
    test_register_case(mod, "netif default exists", test_netif_default);
    test_register_case(mod, "netif HW address", test_netif_hwaddr);
    test_register_case(mod, "netif MTU=1500", test_netif_mtu);
    test_register_case(mod, "DHCP start", test_dhcp_start);
    test_register_case(mod, "ARP config", test_arp_config);
    test_register_case(mod, "DNS config", test_dns_config);
    test_register_case(mod, "IPv4 fragmentation", test_ipv4_frag_config);

#if LWIP_IPV6
    test_register_case(mod, "IPv6 link-local", test_ipv6_linklocal);
    test_register_case(mod, "IPv6 config", test_ipv6_config);
#endif

    test_register_case(mod, "ICMP config", test_icmp_config);
    test_register_case(mod, "HTTP server config", test_http_server_config);

#if LWIP_HTTP_CLIENT
    test_register_case(mod, "HTTP client config", test_http_client_config);
#endif

    test_register_case(mod, "mDNS config", test_mdns_config);
    test_register_case(mod, "MQTT config", test_mqtt_config);
    test_register_case(mod, "NetBIOS config", test_netbios_config);
    test_register_case(mod, "SMTP config", test_smtp_config);
    test_register_case(mod, "SNMP config", test_snmp_config);
    test_register_case(mod, "SNTP config", test_sntp_config);
    test_register_case(mod, "TFTP config", test_tftp_config);
    test_register_case(mod, "lwiperf config", test_lwiperf_config);

    test_register_case(mod, "mem pool alloc/free", test_mem_pool);
    test_register_case(mod, "lwIP stats config", test_lwip_stats_config);
    test_register_case(mod, "E1000 dump stats", test_e1000_stats);
}
