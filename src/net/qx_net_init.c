/* ============================================================
 * qx_net_init.c — QX 网络子系统初始化入口
 * ============================================================ */

#include "qx_net.h"
#include "klog.h"
#include "spinlock.h"

extern void sys_init(void);
extern int  e1000_probe(void);
extern int  qx_netif_register_e1000(void);

void qx_net_init(void)
{
    klog_init_msg("--- Network Subsystem Init ---");

    lwip_init();
    klog_net("lwIP core initialized");

    sys_init();
    klog_net("sys_arch ready");

    if (e1000_probe() == 0) {
        klog_net("E1000 detected, registering netif");
        qx_netif_register_e1000();
    } else {
        klog_net_warn("No NIC found, running without network");
    }

    klog_init_msg("--- Network Subsystem Ready ---");
}

int qx_socket_register_syscalls(void)
{
    klog_net("Socket syscalls not yet registered");
    return 0;
}
