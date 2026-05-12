#!/bin/bash
# 最小修复: RX/TX 数据路径 (仅 3 个文件)
set -e
cd /home/anfer/Code/C/AntX

echo "=== net_glue.c ==="
cat > src/kernel/net/arch/net_glue.c << 'GEOFIX'
#include "lwip/netif.h"
#include "lwip/etharp.h"
#include "lwip/ethip6.h"
#include "lwip/pbuf.h"

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
echo "  OK"

echo "=== netif.rs ==="
python3 << 'PYEOF'
with open("src/kernel/net/netif.rs", "r") as f:
    c = f.read()

# Expand buffer
c = c.replace(
    "static mut G_NETIF_BUFFER: [u8; 512] = [0u8; 512];",
    "static mut G_NETIF_BUFFER: [u8; 2048] = [0u8; 2048];")
c = c.replace(
    "core::ptr::write_bytes(G_NETIF_BUFFER.as_mut_ptr(), 0, 512);",
    "core::ptr::write_bytes(G_NETIF_BUFFER.as_mut_ptr(), 0, 2048);")

# Fix ethernet_input_from_e1000: use antx_rx_packet
old_fn = '''pub unsafe extern "C" fn ethernet_input_from_e1000(
    data: *mut core::ffi::c_void,
    len: u16,
) -> i32 {
    // 检查网络接口是否已初始化
    if G_NETIF_PTR.is_null() || data.is_null() || len == 0 {
        return LwipErr::Val as i32; // 无效参数
    }

    // 调用 lwIP ethernet_input 处理数据包
    // 注意: 这里需要将原始数据包装成 pbuf 结构，或者直接使用内存指针
    // 简化实现: 直接传递给 ethernet_input (假设 lwIP 能处理原始指针)
    let result = ethernet_input(data, G_NETIF_PTR);

    result
}'''

new_fn = '''pub unsafe extern "C" fn ethernet_input_from_e1000(
    data: *mut core::ffi::c_void,
    len: u16,
) -> i32 {
    extern "C" {
        fn antx_rx_packet(netif: *mut core::ffi::c_void, data: *const core::ffi::c_void, len: u16) -> i32;
    }
    if G_NETIF_PTR.is_null() || data.is_null() || len == 0 {
        return LwipErr::Val as i32;
    }
    antx_rx_packet(G_NETIF_PTR, data as *const core::ffi::c_void, len)
}'''

if old_fn in c:
    c = c.replace(old_fn, new_fn)
    print("  ethernet_input_from_e1000 fixed")
else:
    print("  WARN: pattern not found")

with open("src/kernel/net/netif.rs", "w") as f:
    f.write(c)
PYEOF

echo "Done"
