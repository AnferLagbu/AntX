#ifndef QX_LWIPOPTS_H
#define QX_LWIPOPTS_H

/* ============================================================
 * lwIP 2.2.1 配置 — AntX (QueenX) 内核定制
 *
 * 策略: 全模块启用, 构建完整的现代网络栈
 * ============================================================ */

/* ---- 操作系统模式 ---- */
#define NO_SYS                      1
#define LWIP_TIMERS                 1
#define LWIP_TIMERS_CUSTOM          0
#define LWIP_HAVE_INT64             1

/* ---- 内存配置 ---- */
#define MEM_LIBC_MALLOC             0
#define MEMP_MEM_MALLOC             0
#define MEM_ALIGNMENT               4
#define MEM_SIZE                    (128 * 1024)
#define MEMP_NUM_PBUF               64
#define MEMP_NUM_UDP_PCB            16
#define MEMP_NUM_TCP_PCB            32
#define MEMP_NUM_TCP_PCB_LISTEN    8
#define MEMP_NUM_TCP_SEG           512
#define MEMP_NUM_SYS_TIMEOUT        16
#define MEMP_NUM_NETBUF             16
#define MEMP_NUM_TCPIP_MSG_API      32
#define MEMP_NUM_TCPIP_MSG_INPKT    32
#define MEMP_NUM_ARP_QUEUE          16
#define MEMP_NUM_IGMP_GROUP         8
#define MEMP_NUM_NETDB              8
#define MEMP_NUM_LOCALHOSTLIST      4
#define MEMP_NUM_RAW_PCB            16
#define MEMP_NUM_SNMP_NODE          32
#define MEMP_NUM_SNMP_ROOTNODE      16
#define MEMP_NUM_SNMP_VARBIND       16
#define MEMP_NUM_SNMP_VALUE         16
#define MEMP_NUM_SNMP_TRAP          4
#define PBUF_POOL_SIZE              128
#define PBUF_POOL_BUFSIZE           1536

/* ---- IPv4 ---- */
#define LWIP_IPV4                   1
#define LWIP_ARP                    1
#define LWIP_ARP_TABLES             4
#define LWIP_ICMP                   1
#define LWIP_IGMP                   1
#define LWIP_AUTOIP                 1
#define LWIP_DHCP                   1
#define LWIP_DHCP_AUTOIP_COOP       1
#define LWIP_DHCP_AUTOIP_COOP_TRIES 5
#define LWIP_ACD                    1
#define LWIP_DNS                    1
#define LWIP_DNS_MAX_SERVERS        4
#define LWIP_DNS_TABLE_SIZE         8
#define LWIP_IPV4_FRAG              1

/* ---- IPv6 ---- */
#define LWIP_IPV6                   1
#define LWIP_IPV6_DHCP6             1
#define LWIP_IPV6_MLD               1
#define LWIP_IPV6_FRAG              1
#define LWIP_IPV6_REASS             1
#define LWIP_ND6                    1
#define LWIP_ND6_ALLOW_RA_UPDATES   1
#define LWIP_IPV6_AUTOCONFIG        1
#define LWIP_IPV6_SEND_ROUTER_SOLICIT 1
#define LWIP_ND6_RTR_SOLICITATION_INTERVAL 4
#define LWIP_IPV6_NUM_ADDRESSES     4

/* ---- 传输层 ---- */
#define LWIP_TCP                    1
#define LWIP_TCP_KEEPALIVE          1
#define LWIP_TCP_QUEUE_OOSEQ        1
#define LWIP_TCP_SACK_OUT           1
#define LWIP_TCP_WND                32768
#define LWIP_TCP_MSS                1460
#define LWIP_TCP_SND_BUF            (2 * LWIP_TCP_MSS)
#define LWIP_TCP_RECVMBOX_SIZE      16
#define LWIP_TCP_PERF               1
#define LWIP_UDP                    1
#define LWIP_RAW                    1
#define LWIP_TCPIP_CORE_LOCKING     0
#define LWIP_TCPIP_CORE_LOCKING_INPUT 0
#define SYS_LIGHTWEIGHT_PROT         0

/* ---- Socket / Netconn API (NO_SYS=1 禁用) ---- */
#define LWIP_NETCONN                0
#define LWIP_SOCKET                 0
#define LWIP_NETIF_API              0

/* ---- 网络接口 ---- */
#define LWIP_NETIF_HOSTNAME         1
#define LWIP_NETIF_STATUS_CALLBACK  1
#define LWIP_NETIF_LINK_CALLBACK    1
#define LWIP_NETIF_REMOVE_CALLBACK  1
#define LWIP_NETIF_HWADDRHINT       1
#define LWIP_NETIF_TX_SINGLE_PBUF   1
#define LWIP_NETIF_EXT_STATUS_CALLBACK 1
#define LWIP_NUM_NETIF_CLIENT_DATA     2
#define LWIP_HAVE_LOOPIF            1
#define LWIP_LOOPBACK_MAX_PBUFS     8

/* ---- 以太网 ---- */
#define LWIP_ETHERNET               1
#define ETHARP_SUPPORT_STATIC_ENTRIES 1

/* ---- HTTP 服务器 ---- */
#define LWIP_HTTPD                  1
#define LWIP_HTTPD_CGI              1
#define LWIP_HTTPD_SSI              1
#define LWIP_HTTPD_MAX_CGI_PARAMETERS 8
#define LWIP_HTTPD_DYNAMIC_HEADERS  1
#define LWIP_HTTPD_MAX_TAG_NAME_LEN 8
#define LWIP_HTTPD_MAX_TAG_INSERT_LEN 1024
// 注意: HTTP 数据已由 Rust fsdata.rs 通过 FFI 导出 (FS_ROOT, FsFileEntry)
// 不再需要 C 版本的 qx_fsdata.c
// #define HTTPD_FSDATA_FILE           "qx_fsdata.c"

/* ---- HTTP 客户端 ---- */
#define LWIP_HTTP_CLIENT            1
#define LWIP_HTTPC_POLL_INTERVAL    100
#define LWIP_HTTPC_TIMEOUT_DEFAULT  30

/* ---- mDNS ---- */
#define LWIP_MDNS_RESPONDER         1
#define MDNS_RESP_USENETIF_EXTCONTEXT 1

/* ---- MQTT ---- */
#define LWIP_MQTT                   1

/* ---- NetBIOS ---- */
#define LWIP_NETBIOSNS              1

/* ---- SMTP ---- */
#define LWIP_SMTP                   1

/* ---- SNMP ---- */
#define LWIP_SNMP                   1
#define SNMP_USE_NETCONN            0
#define SNMP_USE_RAW                1
#define SNMP_TRAP_DESTINATIONS      2
#define SNMP_SAFE_STRING            1

/* ---- SNTP ---- */
#define LWIP_SNTP                   1
#define SNTP_SERVER_DNS             1
#define SNTP_CHECK_RESPONSE         2
#define SNTP_MAX_SERVERS            2

/* ---- TFTP ---- */
#define LWIP_TFTP                   1
#define LWIP_TFTP_SERVER            1

/* ---- lwiperf ---- */
#define LWIP_LWIPERF                1

/* ---- PPP (禁用) ---- */
#define PPP_SUPPORT                 0
#define LWIP_PPP_API                0

/* ---- altcp ---- */
#define LWIP_ALTCP                  1
#define LWIP_ALTCP_TLS              0
#define LWIP_ALTCP_TLS_MBEDTLS      0

/* ---- 统计/调试 ---- */
#define LWIP_DEBUG                  0
#define LWIP_STATS                  1
#define LWIP_STATS_DISPLAY          1
#define LWIP_STATS_LARGE            1
#define MIB2_STATS                  1

/* ---- 校验和 ---- */
#define CHECKSUM_GEN_IP             1
#define CHECKSUM_GEN_UDP            1
#define CHECKSUM_GEN_TCP            1
#define CHECKSUM_GEN_ICMP           1
#define CHECKSUM_GEN_ICMP6          1
#define CHECKSUM_CHECK_IP           1
#define CHECKSUM_CHECK_UDP          1
#define CHECKSUM_CHECK_TCP          1
#define CHECKSUM_CHECK_ICMP         1
#define CHECKSUM_CHECK_ICMP6        1
#define LWIP_CHECKSUM_ON_COPY       1

/* ---- 钩子 ---- */
#define LWIP_HOOK_FILENAME          "arch/qx_hooks.h"

#endif /* QX_LWIPOPTS_H */
