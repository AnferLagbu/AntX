#ifndef _IOAPIC_H
#define _IOAPIC_H

#include "types.h"

#define IOAPIC_DEFAULT_BASE    0xFEC00000
#define IOAPIC_MMIO_SIZE       0x1000

#define IOAPIC_REG_INDEX       0x00
#define IOAPIC_REG_DATA        0x10

#define IOAPIC_REG_ID          0x00
#define IOAPIC_REG_VER         0x01
#define IOAPIC_REG_ARB         0x02
#define IOAPIC_REDTBL_BASE     0x10

#define IOAPIC_DELIVERY_FIXED     0x00
#define IOAPIC_DELIVERY_LOWPRI    0x01
#define IOAPIC_DELIVERY_SMI       0x02
#define IOAPIC_DELIVERY_NMI       0x04
#define IOAPIC_DELIVERY_INIT      0x05
#define IOAPIC_DELIVERY_EXTINT    0x07

#define IOAPIC_DESTMODE_PHYSICAL  0x00
#define IOAPIC_DESTMODE_LOGICAL   0x01

#define IOAPIC_POLARITY_HIGH      0x00
#define IOAPIC_POLARITY_LOW       0x01

#define IOAPIC_TRIGGER_EDGE       0x00
#define IOAPIC_TRIGGER_LEVEL      0x01

#define IOAPIC_MASKED             (1 << 16)
#define IOAPIC_UNMASKED           0x00000000

typedef struct {
    uint32_t *mmio_base;   /* mapped virtual address of IOAPIC MMIO */
    uint32_t gsi_base;     /* global system interrupt base */
    uint8_t  ioapic_id;    /* this IOAPIC's ID */
    uint8_t  version;      /* IOAPIC version */
    uint8_t  max_redir;    /* maximum redirection entries */
    uint8_t  present;      /* 1 if IOAPIC detected and operational */
} ioapic_t;

int  ioapic_init(void);
void ioapic_redirect_irq(uint8_t irq, uint8_t vector, uint8_t dest_apic_id, uint32_t flags);
void ioapic_mask_irq(uint8_t irq);
void ioapic_unmask_irq(uint8_t irq);
void ioapic_send_eoi(uint8_t irq);
void ioapic_dump_info(void);
int  ioapic_is_present(void);

extern ioapic_t g_ioapic;

#endif
