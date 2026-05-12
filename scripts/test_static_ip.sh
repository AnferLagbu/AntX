#!/bin/bash
# Static IP test — verify hardware TX/RX without DHCP
set -e
cd /home/anfer/Code/C/AntX

cp src/kernel/net/arch/net_glue.c backup/net_dhcp_fix/net_glue.c.bak_static

cat > src/kernel/net/arch/net_glue.c << 'GEOFIX'
#include "lwip/netif.h"
#include "lwip/etharp.h"
#include "lwip/ethip6.h"
#include "lwip/pbuf.h"
#include "lwip/ip_addr.h"

extern err_t e1000_send(struct netif *netif, struct pbuf *p);

void antx_netif_init(struct netif *netif, const uint8_t *mac) {
    netif->hwaddr_len = 6;
    int i;
    for (i = 0; i < 6; i++) netif->hwaddr[i] = mac[i];
    netif->mtu = 1500;
    netif->flags = NETIF_FLAG_BROADCAST | NETIF_FLAG_ETHARP | NETIF_FLAG_ETHERNET
                 | NETIF_FLAG_IGMP | NETIF_FLAG_MLD6;
    netif->output = etharp_output;
    netif->output_ip6 = ethip6_output;
    netif->linkoutput = e1000_send;
    netif->name[0] = 'e';
    netif->name[1] = 'n';
}

err_t antx_rx_packet(struct netif *netif, const void *data, u16_t len) {
    struct pbuf *p;
    u16_t copied;

    if (netif == NULL || data == NULL || len == 0) return ERR_VAL;

    p = pbuf_alloc(PBUF_RAW, len, PBUF_POOL);
    if (p == NULL) return ERR_MEM;

    copied = pbuf_take(p, data, len);
    if (copied != len) { pbuf_free(p); return ERR_MEM; }

    if (netif->input(p, netif) != ERR_OK) { pbuf_free(p); return ERR_IF; }
    return ERR_OK;
}

void antx_pbuf_copyout(struct pbuf *p, void *buf, u16_t *out_len) {
    u16_t total = 0;
    struct pbuf *q = p;
    u8_t *dst = (u8_t *)buf;
    while (q != NULL && total + q->len <= *out_len) {
        memcpy(dst + total, q->payload, q->len);
        total += q->len;
        q = q->next;
    }
    *out_len = total;
}
GEOFIX
echo "Switched to static IP glue (removed DHCP, added static config)"

# Also disable DHCP in netif.rs
python3 << 'PYEOF'
with open("src/kernel/net/netif.rs", "r") as f:
    c = f.read()

# Comment out dhcp_start call
old = '''    // 启动 DHCP
    klog_net("Starting DHCP on E1000...\\0".as_ptr() as *const i8);
    
    let dhcp_result = dhcp_start(result);
    
    // 输出DHCP启动结果
    if dhcp_result == 0 {
        klog_net("DHCP client started successfully\\0".as_ptr() as *const i8);
    } else {
        klog_net_err("DHCP start failed\\0".as_ptr() as *const i8);
    }'''

new = '''    // 静态 IP 测试: 10.0.2.15/255.255.255.0 gw 10.0.2.2
    // (QEMU user-mode 默认网关)
    klog_net("Setting static IP 10.0.2.15\\0".as_ptr() as *const i8);
    extern "C" {
        fn netif_set_ipaddr(netif: *mut core::ffi::c_void, ipaddr: *const core::ffi::c_void);
        fn netif_set_netmask(netif: *mut core::ffi::c_void, netmask: *const core::ffi::c_void);
        fn netif_set_gw(netif: *mut core::ffi::c_void, gw: *const core::ffi::c_void);
        fn ipaddr_addr(addr: *const i8) -> u32;
        fn set_ip4_addr(addr: *mut u32, ip: u32);
    }
    let mut ip: u32 = 0;
    let ip_raw = ipaddr_addr(b"10.0.2.15\\0".as_ptr() as *const i8);
    set_ip4_addr(&mut ip, ip_raw);
    netif_set_ipaddr(result, &ip as *const u32 as *const core::ffi::c_void);
    let mut nm: u32 = 0;
    let nm_raw = ipaddr_addr(b"255.255.255.0\\0".as_ptr() as *const i8);
    set_ip4_addr(&mut nm, nm_raw);
    netif_set_netmask(result, &nm as *const u32 as *const core::ffi::c_void);
    let mut gw: u32 = 0;
    let gw_raw = ipaddr_addr(b"10.0.2.2\\0".as_ptr() as *const i8);
    set_ip4_addr(&mut gw, gw_raw);
    netif_set_gw(result, &gw as *const u32 as *const core::ffi::c_void);
    klog_net("Static IP configured\\0".as_ptr() as *const i8);'''

if old in c:
    c = c.replace(old, new)
    print("  DHCP → Static IP")
else:
    print("  WARN: pattern not found")

with open("src/kernel/net/netif.rs", "w") as f:
    f.write(c)
PYEOF

echo "Done"
