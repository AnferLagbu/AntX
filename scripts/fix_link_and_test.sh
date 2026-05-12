#!/bin/bash
# 关键修复: netif_set_link_up
set -e
cd /home/anfer/Code/C/AntX

cp src/kernel/net/arch/net_glue.c backup/net_dhcp_fix/net_glue.c.bak_link

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
    netif_set_link_up(netif);
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
echo "Fixed: netif_set_link_up added to init"

cd src/rust && cargo check 2>&1 | head -3 && echo "---" && cd ../.. && rm -rf build/net build/kernel.bin && make all 2>&1 | tail -2 && make iso 2>/dev/null && echo "=== QEMU 35s ===" && timeout 35 qemu-system-x86_64 -cdrom build/antx.iso -serial stdio -display none -no-reboot -m 128M -device e1000,netdev=n0 -netdev user,id=n0 2>&1 | grep -E "DHCP|E1000|NET\]|Ready|bound|Status|IP" ; echo "EXIT:$?"