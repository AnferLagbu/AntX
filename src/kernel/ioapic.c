#include "ioapic.h"
#include "klog.h"
#include "string.h"

extern int  vmm_split_2mb_page(uint64_t vaddr);
extern int  vmm_map_page(uint64_t vaddr, uint64_t paddr, uint64_t flags);

#define KERNEL_BASE  0xFFFF800000000000ULL
#define PAGE_PRESENT 1
#define PAGE_WRITABLE 2
#define PAGE_NOCACHE 0x18  /* PCD + PWT for MMIO: disable caching */

ioapic_t g_ioapic;

static inline void ioapic_write(uint8_t index, uint32_t value) {
    g_ioapic.mmio_base[IOAPIC_REG_INDEX / 4] = index;
    g_ioapic.mmio_base[IOAPIC_REG_DATA  / 4] = value;
}

static inline uint32_t ioapic_read(uint8_t index) {
    g_ioapic.mmio_base[IOAPIC_REG_INDEX / 4] = index;
    return g_ioapic.mmio_base[IOAPIC_REG_DATA / 4];
}

static void ioapic_write_redtbl(uint8_t idx, uint64_t value) {
    uint8_t reg = (uint8_t)(IOAPIC_REDTBL_BASE + idx * 2);
    ioapic_write(reg,     (uint32_t)(value & 0xFFFFFFFF));
    ioapic_write(reg + 1, (uint32_t)(value >> 32));
}

int ioapic_init(void) {
    memset(&g_ioapic, 0, sizeof(g_ioapic));

    uint64_t phys_base = IOAPIC_DEFAULT_BASE;
    /* Map IOAPIC MMIO page via kernel page tables (uncached for MMIO) */
    uint64_t virt_base = KERNEL_BASE + phys_base;
    vmm_split_2mb_page(virt_base);
    if (vmm_map_page(virt_base, phys_base, PAGE_PRESENT | PAGE_WRITABLE | PAGE_NOCACHE) < 0) {
        klog_kern("IOAPIC: Failed to map MMIO at 0x%llx", phys_base);
        return -1;
    }

    g_ioapic.mmio_base = (uint32_t *)virt_base;

    uint32_t ver = ioapic_read(IOAPIC_REG_VER);
    g_ioapic.version  = (uint8_t)(ver & 0xFF);
    g_ioapic.max_redir = (uint8_t)((ver >> 16) & 0xFF) + 1;
    g_ioapic.ioapic_id = (uint8_t)((ioapic_read(IOAPIC_REG_ID) >> 24) & 0x0F);

    if (g_ioapic.max_redir < 16) {
        klog_kern_warn("IOAPIC: only %d redirection entries (min 16 expected)", g_ioapic.max_redir);
    }

    g_ioapic.present = 1;
    g_ioapic.gsi_base = 0;

    klog_kern("IOAPIC: v%d found (id=%d, %d redir entries) at 0x%llx",
              g_ioapic.version, g_ioapic.ioapic_id, g_ioapic.max_redir, phys_base);

    /* Mask all interrupts before reprogramming */
    for (int i = 0; i < g_ioapic.max_redir; i++) {
        ioapic_write_redtbl(i, IOAPIC_MASKED);
    }

    return 0;
}

void ioapic_redirect_irq(uint8_t irq, uint8_t vector, uint8_t dest_apic_id, uint32_t flags) {
    if (!g_ioapic.present) return;
    if (irq >= g_ioapic.max_redir) {
        klog_kern_warn("IOAPIC: IRQ %d exceeds max entries %d", irq, g_ioapic.max_redir);
        return;
    }

    uint64_t entry = (uint64_t)vector;
    entry |= (uint64_t)(flags & 0xFFFF) << 16;
    entry |= (uint64_t)dest_apic_id << 56;

    ioapic_write_redtbl(irq, entry);
}

void ioapic_mask_irq(uint8_t irq) {
    if (!g_ioapic.present || irq >= g_ioapic.max_redir) return;
    uint8_t reg = (uint8_t)(IOAPIC_REDTBL_BASE + irq * 2);
    uint32_t lo = ioapic_read(reg);
    lo |= IOAPIC_MASKED;
    ioapic_write(reg, lo);
}

void ioapic_unmask_irq(uint8_t irq) {
    if (!g_ioapic.present || irq >= g_ioapic.max_redir) return;
    uint8_t reg = (uint8_t)(IOAPIC_REDTBL_BASE + irq * 2);
    uint32_t lo = ioapic_read(reg);
    lo &= ~IOAPIC_MASKED;
    ioapic_write(reg, lo);
}

void ioapic_send_eoi(uint8_t irq) {
    (void)irq;
    if (!g_ioapic.present) return;
    /* EOI is sent to Local APIC, not IOAPIC. This is done via LAPIC EOI register.
     * We let the caller handle LAPIC EOI separately. */
}

void ioapic_dump_info(void) {
    if (!g_ioapic.present) {
        klog_kern("IOAPIC: not present");
        return;
    }
    klog_kern("IOAPIC: id=%d ver=%d entries=%d base=0x%llx (virt=0x%llx)",
              g_ioapic.ioapic_id, g_ioapic.version, g_ioapic.max_redir,
              (uint64_t)IOAPIC_DEFAULT_BASE, (uint64_t)g_ioapic.mmio_base);
    for (int i = 0; i < g_ioapic.max_redir; i++) {
        uint32_t lo = ioapic_read((uint8_t)(IOAPIC_REDTBL_BASE + i * 2));
        uint32_t hi = ioapic_read((uint8_t)(IOAPIC_REDTBL_BASE + i * 2 + 1));
        if (lo == IOAPIC_MASKED && hi == 0) continue;
        klog_kern("  IRQ%d: vector=%d dest=%d masked=%d low=0x%08x high=0x%08x",
                  i, lo & 0xFF, (hi >> 24) & 0xFF, (lo >> 16) & 1, lo, hi);
    }
}

int ioapic_is_present(void) {
    return g_ioapic.present;
}
