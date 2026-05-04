#ifndef QX_LWIPOPTS_H
#define QX_LWIPOPTS_H

/* ============================================================
 * lwIP 2.2.1 配置 — AntX (QueenX) 内核定制
 *
 * 策略: 能用的模块全部启用, 构建完整的现代网络栈
 * ============================================================ */

/* ---- 操作系统模式 ---- */
#define NO_SYS                      1   /* Raw API 单线程模式 */
#define LWIP_TIMERS                 1
#define LWIP_TIMERS_CUSTOM          0

/* ---- 内存配置 ---- */
#define MEM_LIBC_MALLOC             0   /* 使用 lwIP 内部内存池 */
#define MEMP_MEM_MALLOC             0   /* 使用静态 memp 数组 */
#define MEM_ALIGNMENT               4
#define MEM_SIZE                    (64 * 1024)       /* 堆 64KB */
#define MEMP_NUM_PBUF               32
#define MEMP_NUM_UDP_PCB            16
#define MEMP_NUM_TCP_PCB            32
#define MEMP_NUM_TCP_PCB_LISTEN    8
#define MEMP_NUM_TCP_SEG            256
#define MEMP_NUM_SYS_TIMEOUT        16
#define MEMP_NUM_NETBUF             16
#define MEMP_NUM_TCPIP_MSG_API      32
#define MEMP_NUM_TCPIP_MSG_INPKT    32
#define MEMP_NUM_ARP_QUEUE          16
#define MEMP_NUM_IGMP_GROUP         8
#define MEMP_NUM_NETDB              8
#define MEMP_NUM_LOCALHOSTLIST      4
#define MEMP_NUM_RAW_PCB            8
#define PBUF_POOL_SIZE              64
#define PBUF_POOL_BUFSIZE           1536  /* 以太网 MTU */

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
#define LWIP_ACD                    1   /* IPv4 地址冲突检测 */
#define LWIP_DNS                    1
#define LWIP_DNS_MAX_SERVERS        2
#define LWIP_IPV4_FRAG              1

/* ---- IPv6 ---- */
#define LWIP_IPV6                   1
#define LWIP_IPV6_DHCP6             1
#define LWIP_IPV6_MLD               1
#define LWIP_IPV6_FRAG              0
#define LWIP_IPV6_REASS             0
#define LWIP_ND6                    1
#define LWIP_ND6_ALLOW_RA_UPDATES   1
#define LWIP_IPV6_AUTOCONFIG        1
#define LWIP_IPV6_SEND_ROUTER_SOLICIT 1
#define LWIP_ND6_RTR_SOLICITATION_INTERVAL 4

/* ---- 传输层 ---- */
#define LWIP_TCP                    1
#define LWIP_TCP_KEEPALIVE          1
#define LWIP_TCP_QUEUE_OOSEQ        1
#define LWIP_TCP_SACK_OUT           1
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
#define HTTPD_USE_CUSTOM_FSDATA     0

/* ---- HTTP 客户端 ---- */
#define LWIP_HTTP_CLIENT            1

/* ---- mDNS ---- */
#define LWIP_MDNS                   1
#define MDNS_RESP_USENETIF_EXTCONTEXT 1

/* ---- MQTT ---- */
#define LWIP_MQTT                   1

/* ---- NetBIOS ---- */
#define LWIP_NETBIOSNS              1

/* ---- SMTP ---- */
#define LWIP_SMTP                   1

/* ---- SNMP (Phase 2) ---- */
#define LWIP_SNMP                   0

/* ---- SNTP ---- */
#define LWIP_SNTP                   1
#define SNTP_SERVER_DNS             1
#define SNTP_CHECK_RESPONSE         2

/* ---- TFTP ---- */
#define LWIP_TFTP                   1
#define LWIP_TFTP_SERVER            1

/* ---- lwiperf ---- */
#define LWIP_LWIPERF                1

/* ---- PPP (条件启用) ---- */
#define PPP_SUPPORT                 0
#define LWIP_PPP_API                0

/* ---- altcp / TLS (Phase 3) ---- */
#define LWIP_ALTCP                  1
#define LWIP_ALTCP_TLS              0
#define LWIP_ALTCP_TLS_MBEDTLS      0

/* ---- 统计/调试 ---- */
#define LWIP_DEBUG                  0
#define LWIP_STATS                  1
#define LWIP_STATS_DISPLAY          1
#define LWIP_STATS_LARGE            1

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

/* ---- 钩子/Hooks ---- */
#define LWIP_HOOK_FILENAME          "arch/qx_hooks.h"

#endif /* QX_LWIPOPTS_H */
