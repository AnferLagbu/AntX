#include "lwip/netif.h"
#include "lwip/etharp.h"
#include "lwip/ethip6.h"
#include "lwip/pbuf.h"
#include "lwip/ip4_addr.h"

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

u32_t antx_netif_ip4_addr_u32(const struct netif *netif) {
    if (netif == NULL) return 0;
    return ip4_addr_get_u32(netif_ip4_addr(netif));
}
