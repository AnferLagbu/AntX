/* ============================================================
 * qx_net_init.c — QX 网络子系统初始化入口
 * ============================================================ */

#include "qx_net.h"
#include "serial.h"
#include "spinlock.h"

extern void sys_init(void);

void qx_net_init(void)
{
    serial_puts(SERIAL_COM1, "[NET] Initializing network subsystem...\n");

    /* 1. 初始化移植层 */
    sys_init();

    /* 2. 初始化 lwIP 核心 */
    lwip_init();
    serial_puts(SERIAL_COM1, "[NET] lwIP " LWIP_VERSION_STR " core initialized\n");

    /* 3. NIC 驱动探测 (Phase 2) */
    serial_puts(SERIAL_COM1, "[NET] NIC driver not yet implemented (Phase 2)\n");

    serial_puts(SERIAL_COM1, "[NET] Network subsystem ready (lwIP core only)\n");
}

/* ---- Socket syscall stubs (Phase 4) ---- */
int qx_socket_register_syscalls(void)
{
    serial_puts(SERIAL_COM1, "[NET] Socket syscalls not yet registered (Phase 4)\n");
    return 0;
}
