#include "lwipopts.h"
#include "lwip/opt.h"
#include "lwip/init.h"
#include "lwip/netif.h"
#include "lwip/ip_addr.h"
#include "lwip/ip4_addr.h"
#include "lwip/raw.h"
#include "lwip/icmp.h"
#include "lwip/inet_chksum.h"
#include "lwip/prot/icmp.h"
#include "lwip/dns.h"
#include "lwip/pbuf.h"
#include "lwip/def.h"
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
#include "lwip/apps/snmp_scalar.h"
#include "lwip/apps/snmp_table.h"
#include "lwip/apps/snmp_core.h"

#include "e1000.h"
#include "klog.h"

#define PING_DATA_SIZE   32
#define PING_ID          0xA701

static uint16_t ping_seq_num = 0;
static volatile uint8_t ping_reply_received = 0;
static uint32_t ping_sent_count = 0;
static uint32_t ping_recv_count = 0;

static uint16_t ping_checksum(const void *data, uint16_t len)
{
    const uint16_t *buf = (const uint16_t *)data;
    uint32_t sum = 0;
    while (len > 1) {
        sum += *buf++;
        len -= 2;
    }
    if (len == 1) {
        sum += *(const uint8_t *)buf;
    }
    sum = (sum >> 16) + (sum & 0xFFFF);
    sum += (sum >> 16);
    return (uint16_t)(~sum);
}

static u8_t ping_recv(void *arg, struct raw_pcb *pcb, struct pbuf *p,
                       const ip_addr_t *addr)
{
    (void)arg;
    (void)pcb;

    if (p->tot_len >= 20 + 8) {
        struct icmp_echo_hdr *iecho = (struct icmp_echo_hdr *)((uint8_t *)p->payload + 20);
        if (iecho->type == ICMP_ER && iecho->id == PING_ID) {
            ping_recv_count++;
            if (!ping_reply_received) {
                ping_reply_received = 1;
                const ip4_addr_t *src = ip_2_ip4(addr);
                klog_net("Ping reply from %d.%d.%d.%d seq=%d",
                         ip4_addr1(src), ip4_addr2(src),
                         ip4_addr3(src), ip4_addr4(src),
                         lwip_ntohs(iecho->seqno));
            }
            pbuf_free(p);
            return 1;
        }
    }

    return 0;
}

static void ping_send(struct raw_pcb *pcb, const ip_addr_t *target)
{
    struct pbuf *p = pbuf_alloc(PBUF_IP, 8 + PING_DATA_SIZE, PBUF_RAM);
    if (!p) {
        klog_net_warn("Ping: pbuf_alloc failed");
        return;
    }

    struct icmp_echo_hdr *iecho = (struct icmp_echo_hdr *)p->payload;
    iecho->type  = ICMP_ECHO;
    iecho->code  = 0;
    iecho->id    = PING_ID;
    iecho->seqno = lwip_htons(++ping_seq_num);
    iecho->chksum = 0;
    iecho->chksum = ping_checksum(iecho, 8 + PING_DATA_SIZE);

    raw_sendto(pcb, p, target);
    pbuf_free(p);

    ping_sent_count++;
    ping_reply_received = 0;
    const ip4_addr_t *dst = ip_2_ip4(target);
    klog_net("Ping %d.%d.%d.%d seq=%d",
             ip4_addr1(dst), ip4_addr2(dst),
             ip4_addr3(dst), ip4_addr4(dst),
             ping_seq_num);
}

static struct raw_pcb *g_ping_pcb = NULL;

static void ping_gateway(struct netif *netif)
{
    ip_addr_t gw;
    ip_addr_copy(gw, netif->gw);

    if (ip4_addr_get_u32(ip_2_ip4(&netif->gw)) == 0) {
        IP4_ADDR(ip_2_ip4(&gw), 10, 0, 2, 2);
        IP_SET_TYPE_VAL(gw, IPADDR_TYPE_V4);
    }

    g_ping_pcb = raw_new(IP_PROTO_ICMP);
    if (!g_ping_pcb) {
        klog_net_warn("Ping: raw_new failed");
        return;
    }

    raw_recv(g_ping_pcb, ping_recv, NULL);
    raw_bind(g_ping_pcb, IP_ADDR_ANY);

    klog_net("Ping: sending to gateway...");
    ping_send(g_ping_pcb, &gw);
    ping_send(g_ping_pcb, &gw);
    ping_send(g_ping_pcb, &gw);

    klog_net("Ping: sent=%lu recv=%lu", ping_sent_count, ping_recv_count);
}

static void dns_found(const char *name, const ip_addr_t *addr, void *arg)
{
    (void)arg;
    if (addr) {
        const ip4_addr_t *a = ip_2_ip4(addr);
        klog_net("DNS: %s -> %d.%d.%d.%d", name,
                 ip4_addr1(a), ip4_addr2(a),
                 ip4_addr3(a), ip4_addr4(a));
    } else {
        klog_net_warn("DNS: %s not found", name);
    }
}

static void dns_test(void)
{
    klog_net("DNS: resolving example.com...");
    dns_gethostbyname("example.com", NULL, dns_found, NULL);
}

static void http_server_init(void)
{
    httpd_init();
    klog_net("HTTP Server: started on port 80");
}

#if LWIP_HTTP_CLIENT
static void http_client_result(void *arg, httpc_result_t httpc_result,
                                u32_t rx_content_len, u32_t srv_res, err_t err)
{
    (void)arg;
    (void)srv_res;
    klog_net("HTTP Client: result=%d rx_len=%lu err=%d",
             (int)httpc_result, (unsigned long)rx_content_len, (int)err);
}

static err_t http_client_headers(httpc_state_t *connection, void *arg,
                                  struct pbuf *hdr, u16_t hdr_len,
                                  u32_t content_len)
{
    (void)connection;
    (void)arg;
    (void)hdr;
    (void)content_len;
    klog_net("HTTP Client: hdr_len=%d content_len=%lu", (int)hdr_len, (unsigned long)content_len);
    return ERR_OK;
}

static void http_client_test(struct netif *netif)
{
    (void)netif;
    ip_addr_t server;
    IP4_ADDR(ip_2_ip4(&server), 10, 0, 2, 2);
    IP_SET_TYPE_VAL(server, IPADDR_TYPE_V4);

    httpc_connection_t conn_settings = {0};
    conn_settings.result_fn = http_client_result;
    conn_settings.headers_done_fn = http_client_headers;

    err_t err = httpc_get_file(&server, 80, "/",
                                &conn_settings, NULL, NULL, NULL);
    klog_net("HTTP Client: GET / from 10.0.2.2 -> err=%d", (int)err);
}
#endif

#if LWIP_MDNS_RESPONDER
static void mdns_report(struct netif *netif, u8_t result, s8_t slot)
{
    (void)netif;
    (void)slot;
    klog_net("mDNS: report result=%d slot=%d", (int)result, (int)slot);
}

static void mdns_init_module(struct netif *netif)
{
    mdns_resp_register_name_result_cb(mdns_report);
    mdns_resp_init();
    mdns_resp_add_netif(netif, "antx");
    mdns_resp_add_service(netif, "antx", "_http", DNSSD_PROTO_TCP, 80, NULL, NULL);
    klog_net("mDNS: responder started (host=antx, _http._tcp:80)");
}
#endif

#if LWIP_MQTT
static void mqtt_connection_cb(mqtt_client_t *client, void *arg, mqtt_connection_status_t status)
{
    (void)client;
    (void)arg;
    klog_net("MQTT: connection status=%d", (int)status);
}

static void mqtt_init_module(struct netif *netif)
{
    (void)netif;
    mqtt_client_t *client = mqtt_client_new();
    if (client) {
        klog_net("MQTT: client allocated (ready for broker connection)");
    } else {
        klog_net_warn("MQTT: client alloc failed");
    }
}
#endif

#if LWIP_NETBIOSNS
static void netbios_init_module(struct netif *netif)
{
    netbiosns_init();
    netbiosns_set_name("ANTX");
    klog_net("NetBIOS: name=ANTX");
}
#endif

#if LWIP_SMTP
static void qx_smtp_result(void *arg, u8_t smtp_result, u16_t srv_err, err_t err)
{
    (void)arg;
    (void)srv_err;
    klog_net("SMTP: result=%d err=%d", (int)smtp_result, (int)err);
}

static void smtp_init_module(struct netif *netif)
{
    (void)netif;
    smtp_set_server_addr("10.0.2.2");
    smtp_set_server_port(25);
    klog_net("SMTP: configured (server=10.0.2.2:25)");
}
#endif

#if LWIP_SNMP
static void snmp_init_module(struct netif *netif)
{
    (void)netif;
    static const u16_t sysdescr_len = 18;
    static const u16_t syscontact_len = 9;
    static const u16_t sysname_len = 4;
    static const u16_t sysloc_len = 4;
    snmp_mib2_set_sysdescr((const u8_t *)"AntX QueenX Kernel", &sysdescr_len);
    snmp_mib2_set_syscontact_readonly((const u8_t *)"root@antx", &syscontact_len);
    snmp_mib2_set_sysname_readonly((const u8_t *)"antx", &sysname_len);
    snmp_mib2_set_syslocation_readonly((const u8_t *)"QEMU", &sysloc_len);
    snmp_init();
    klog_net("SNMP: agent started (MIB2 + raw mode)");
}
#endif

#if LWIP_SNTP
static void sntp_result(void *arg, int status)
{
    (void)arg;
    klog_net("SNTP: sync result=%d", status);
}

static void sntp_init_module(struct netif *netif)
{
    (void)netif;
    sntp_setoperatingmode(SNTP_OPMODE_POLL);
    sntp_setservername(0, "pool.ntp.org");
    sntp_setservername(1, "time.google.com");
    sntp_init();
    klog_net("SNTP: started (pool.ntp.org, time.google.com)");
}
#endif

#if LWIP_TFTP
static void *tftp_open(const char *fname, const char *mode, u8_t write)
{
    (void)fname;
    (void)mode;
    (void)write;
    klog_net("TFTP: open %s mode=%s write=%d", fname, mode, (int)write);
    return NULL;
}

static void tftp_close(void *handle)
{
    (void)handle;
    klog_net("TFTP: close");
}

static int tftp_read(void *handle, void *buf, int bytes)
{
    (void)handle;
    (void)buf;
    (void)bytes;
    return -1;
}

static int tftp_write(void *handle, struct pbuf *p)
{
    (void)handle;
    (void)p;
    return -1;
}

static void tftp_error(void *handle, int err, const char *msg, int size)
{
    (void)handle;
    (void)err;
    (void)msg;
    (void)size;
}

static const struct tftp_context tftp_ctx = {
    tftp_open,
    tftp_close,
    tftp_read,
    tftp_write,
    tftp_error
};

static void tftp_init_module(struct netif *netif)
{
    (void)netif;
    tftp_init_server(&tftp_ctx);
    klog_net("TFTP: server started on port 69");
}
#endif

#if LWIP_LWIPERF
static void lwiperf_result(void *arg, enum lwiperf_report_type report_type,
                            const ip_addr_t *local_addr, u16_t local_port,
                            const ip_addr_t *remote_addr, u16_t remote_port,
                            u32_t bytes_transferred, u32_t ms_duration,
                            u32_t bandwidth_kbitpsec)
{
    (void)arg;
    (void)local_addr;
    (void)local_port;
    (void)remote_addr;
    (void)remote_port;
    klog_net("lwiperf: type=%d bytes=%lu ms=%lu kbps=%lu",
             (int)report_type,
             (unsigned long)bytes_transferred,
             (unsigned long)ms_duration,
             (unsigned long)bandwidth_kbitpsec);
}

static void lwiperf_init_module(struct netif *netif)
{
    (void)netif;
    lwiperf_start_tcp_server_default(lwiperf_result, NULL);
    klog_net("lwiperf: TCP server started on port 5001");
}
#endif

void qx_net_apps_init(struct netif *netif)
{
    klog_net("--- Initializing Network Applications ---");

    e1000_dump_stats();

    ping_gateway(netif);

    http_server_init();

#if LWIP_HTTP_CLIENT
    http_client_test(netif);
#endif

    dns_test();

#if LWIP_MDNS_RESPONDER
    mdns_init_module(netif);
#endif

#if LWIP_MQTT
    mqtt_init_module(netif);
#endif

#if LWIP_NETBIOSNS
    netbios_init_module(netif);
#endif

#if LWIP_SMTP
    smtp_init_module(netif);
#endif

#if LWIP_SNMP
    snmp_init_module(netif);
#endif

#if LWIP_SNTP
    sntp_init_module(netif);
#endif

#if LWIP_TFTP
    tftp_init_module(netif);
#endif

#if LWIP_LWIPERF
    lwiperf_init_module(netif);
#endif

    klog_net("--- All Network Applications Initialized ---");
}
