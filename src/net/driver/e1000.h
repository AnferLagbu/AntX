#ifndef E1000_H
#define E1000_H

#include "e1000_regs.h"
#include "lwipopts.h"
#include "lwip/pbuf.h"
#include "lwip/netif.h"
#include "lwip/err.h"

/* ============================================================
 * E1000 网卡驱动 — AntX (QueenX) 内核
 *
 * 支持 Intel 82540EM (QEMU 默认)
 * ============================================================ */

/* ---- 发送描述符 ---- */
typedef struct {
    volatile uint64_t addr;      /* 数据缓冲区物理地址 */
    volatile uint16_t length;    /* 数据长度 */
    volatile uint8_t  cso;       /* 校验和偏移 */
    volatile uint8_t  cmd;       /* 命令 */
    volatile uint8_t  status;    /* 状态 (DD bit) */
    volatile uint8_t  css;       /* 校验和起始 */
    volatile uint16_t special;   /* 特殊字段 */
} __attribute__((packed)) e1000_tx_desc_t;

/* ---- 接收描述符 ---- */
typedef struct {
    volatile uint64_t addr;
    volatile uint16_t length;
    volatile uint16_t checksum;
    volatile uint8_t  status;
    volatile uint8_t  errors;
    volatile uint16_t special;
} __attribute__((packed)) e1000_rx_desc_t;

/* ---- 驱动状态 ---- */
typedef struct {
    /* PCI 信息 */
    uint8_t  bus, device, func;
    uint8_t  irq;
    uint8_t  mac[6];

    /* MMIO 基地址 */
    volatile uint8_t *mmio_base;
    uint64_t mmio_phys;

    /* 描述符环 */
    e1000_tx_desc_t *tx_descs;
    e1000_rx_desc_t *rx_descs;
    uint8_t         *rx_buffers[E1000_RX_RING_SIZE];
    volatile uint16_t tx_tail;       /* 下一个发送位置 */
    volatile uint16_t rx_tail;       /* 下一个接收位置 */

    struct netif     *netif;

    volatile uint64_t isr_count;
    volatile uint64_t rx_count;
    volatile uint64_t tx_count;
    volatile uint64_t link_change_count;
} e1000_dev_t;

int   e1000_probe(void);
err_t e1000_init(struct netif *netif);
err_t e1000_send(struct netif *netif, struct pbuf *p);
void  e1000_isr(void);
void  e1000_poll(void);
void  e1000_dump_stats(void);

extern e1000_dev_t g_e1000;

#endif /* E1000_H */
