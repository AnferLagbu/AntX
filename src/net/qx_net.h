#ifndef QX_NET_H
#define QX_NET_H

/* ============================================================
 * QX 网络子系统总头文件
 * ============================================================ */

#include "lwip-version.h"
#include "lwip/opt.h"
#include "lwip/init.h"
#include "lwip/netif.h"
#include "lwip/tcpip.h"
#include "lwip/dhcp.h"
#include "lwip/dns.h"

#include "arch/sys_arch.h"
#include "arch/cc.h"

/* ---- 网络子系统初始化 ---- */
void qx_net_init(void);

/* ---- 网卡驱动注册 ---- */
int  qx_netif_register_e1000(void);

/* ---- Socket 系统调用 ---- */
int  qx_socket_register_syscalls(void);

#endif /* QX_NET_H */
