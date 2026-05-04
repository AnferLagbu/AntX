/* ============================================================
 * qx_net_init.c — QX 网络子系统初始化入口
 * ============================================================ */

#include "qx_net.h"
#include "serial.h"
#include "spinlock.h"

extern void sys_init(void);
extern int  e1000_probe(void);
extern int  qx_netif_register_e1000(void);

void qx_net_init(void)
{
    serial_puts(SERIAL_COM1, "[NET] ============ Network Subsystem Init ============\n");

    /* 1. 初始化移植层 */
    sys_init();

    /* 2. 初始化 lwIP 核心 */
    tcpip_init(NULL, NULL);
    serial_puts(SERIAL_COM1, "[NET] lwIP " LWIP_VERSION_STR " core initialized\n");

    /* 3. E1000 NIC 驱动探测 */
    if (e1000_probe() == 0) {
        serial_puts(SERIAL_COM1, "[NET] E1000 detected, registering netif...\n");
        qx_netif_register_e1000();
    } else {
        serial_puts(SERIAL_COM1, "[NET] No NIC found, running without network\n");
    }

    serial_puts(SERIAL_COM1, "[NET] ==============================================\n");
}

/* ---- Socket syscall stubs (Phase 4) ---- */
int qx_socket_register_syscalls(void)
{
    serial_puts(SERIAL_COM1, "[NET] Socket syscalls not yet registered (Phase 4)\n");
    return 0;
}
