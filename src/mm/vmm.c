#include "mm.h"
#include "serial.h"
#include "assert.h"

uint64_t kernel_pml4;

static pte_t* get_page_table(uint64_t table, uint64_t index, int create) {
    pte_t* tables = (pte_t*)(table & 0x000FFFFFFFFFF000ULL);
    pte_t* entry = &tables[index];
    
    if (entry->fields.present) {
        if (entry->fields.pat) {
            if (!create) {
                return NULL;
            }
            
            uint64_t large_page_phys = entry->fields.frame << 12;
            uint64_t large_page_flags = entry->value & 0xFFF;
            
            void* new_table = pmm_alloc_page();
            if (new_table == NULL) {
                return NULL;
            }
            
            for (int i = 0; i < 512; i++) {
                pte_t* new_entry = &((pte_t*)new_table)[i];
                new_entry->value = 0;
                new_entry->fields.present = 1;
                new_entry->fields.rw = 1;
                new_entry->fields.user = 1;
                new_entry->fields.frame = (large_page_phys + i * PAGE_SIZE) >> 12;
            }
            
            entry->value = 0;
            entry->fields.present = 1;
            entry->fields.rw = 1;
            entry->fields.user = 1;
            entry->fields.frame = (uint64_t)new_table >> 12;
            
            return (pte_t*)new_table;
        }
        
        return (pte_t*)(uint64_t)(entry->fields.frame << 12);
    }
    
    if (create) {
        void* new_table = pmm_alloc_page();
        ASSERT(new_table != NULL);
        
        for (int i = 0; i < 512; i++) {
            pte_t* new_entry = &((pte_t*)new_table)[i];
            new_entry->value = 0;
        }
        
        entry->fields.present = 1;
        entry->fields.rw = 1;
        entry->fields.user = 1;
        entry->fields.frame = (uint64_t)new_table >> 12;
        
        return (pte_t*)new_table;
    }
    
    return NULL;
}

void vmm_init(void) {
    __asm__ volatile ("mov %%cr3, %0" : "=r"(kernel_pml4));
    
    serial_puts(SERIAL_COM1, "VMM initialized\n");
}

void vmm_map_page(uint64_t virt, uint64_t phys, uint64_t flags) {
    pte_t* pml4 = (pte_t*)kernel_pml4;
    
    pte_t* pdpt = get_page_table((uint64_t)pml4, PML4_INDEX(virt), 1);
    if (pdpt == NULL) return;
    
    pte_t* pd = get_page_table((uint64_t)pdpt, PDPT_INDEX(virt), 1);
    if (pd == NULL) return;
    
    pte_t* pt = get_page_table((uint64_t)pd, PD_INDEX(virt), 1);
    if (pt == NULL) return;
    
    pte_t* entry = &pt[PT_INDEX(virt)];
    entry->value = 0;
    entry->fields.present = (flags & PAGE_PRESENT) ? 1 : 0;
    entry->fields.rw = (flags & PAGE_WRITABLE) ? 1 : 0;
    entry->fields.user = (flags & PAGE_USER) ? 1 : 0;
    entry->fields.xd = (flags & PAGE_NX) ? 1 : 0;
    entry->fields.frame = phys >> 12;
}

void vmm_unmap_page(uint64_t virt) {
    pte_t* pml4 = (pte_t*)kernel_pml4;
    
    pte_t* pdpt = get_page_table((uint64_t)pml4, PML4_INDEX(virt), 0);
    if (pdpt == NULL) return;
    
    pte_t* pd = get_page_table((uint64_t)pdpt, PDPT_INDEX(virt), 0);
    if (pd == NULL) return;
    
    pte_t* pt = get_page_table((uint64_t)pd, PD_INDEX(virt), 0);
    if (pt == NULL) return;
    
    pte_t* entry = &pt[PT_INDEX(virt)];
    entry->value = 0;
    
    __asm__ volatile ("invlpg (%0)" : : "r"(virt) : "memory");
}

uint64_t vmm_get_physical(uint64_t virt) {
    pte_t* pml4 = (pte_t*)kernel_pml4;
    
    pte_t* pdpt = get_page_table((uint64_t)pml4, PML4_INDEX(virt), 0);
    if (pdpt == NULL) return 0;
    
    pte_t* pd = get_page_table((uint64_t)pdpt, PDPT_INDEX(virt), 0);
    if (pd == NULL) return 0;
    
    pte_t* pt = get_page_table((uint64_t)pd, PD_INDEX(virt), 0);
    if (pt == NULL) return 0;
    
    pte_t* entry = &pt[PT_INDEX(virt)];
    if (!entry->fields.present) return 0;
    
    return (entry->fields.frame << 12) | (virt & 0xFFF);
}

uint64_t vmm_get_physical_in_table(uint64_t pml4, uint64_t virt) {
    pte_t* pml4_ptr = (pte_t*)pml4;
    
    pte_t* pdpt = get_page_table((uint64_t)pml4_ptr, PML4_INDEX(virt), 0);
    if (pdpt == NULL) return 0;
    
    pte_t* pd = get_page_table((uint64_t)pdpt, PDPT_INDEX(virt), 0);
    if (pd == NULL) return 0;
    
    pte_t* pt = get_page_table((uint64_t)pd, PD_INDEX(virt), 0);
    if (pt == NULL) return 0;
    
    pte_t* entry = &pt[PT_INDEX(virt)];
    if (!entry->fields.present) return 0;
    
    return (entry->fields.frame << 12) | (virt & 0xFFF);
}

void vmm_switch_page_table(uint64_t cr3) {
    __asm__ volatile ("mov %0, %%cr3" : : "r"(cr3) : "memory");
}

static void set_user_bit_recursive(uint64_t table, int level) {
    pte_t* entries = (pte_t*)table;
    for (int i = 0; i < 512; i++) {
        if (entries[i].fields.present && !entries[i].fields.pat) {
            entries[i].fields.user = 1;
            if (level > 1) {
                uint64_t next_table = entries[i].fields.frame << 12;
                set_user_bit_recursive(next_table, level - 1);
            }
        }
    }
}

uint64_t vmm_create_user_page_table(void) {
    uint64_t pml4 = (uint64_t)pmm_alloc_page();
    if (pml4 == 0) {
        return 0;
    }
    
    pte_t* new_pml4 = (pte_t*)pml4;
    for (int i = 0; i < 512; i++) {
        new_pml4[i].value = 0;
    }
    
    for (int i = 256; i < 512; i++) {
        new_pml4[i].value = ((pte_t*)kernel_pml4)[i].value;
    }
    
    for (int i = 0; i < 256; i++) {
        if (((pte_t*)kernel_pml4)[i].fields.present) {
            new_pml4[i].value = ((pte_t*)kernel_pml4)[i].value;
        }
    }
    
    set_user_bit_recursive(pml4, 4);
    
    return pml4;
}

void vmm_map_page_in_table(uint64_t pml4, uint64_t virt, uint64_t phys, uint64_t flags) {
    pte_t* pml4_ptr = (pte_t*)pml4;
    
    pte_t* pdpt = get_page_table((uint64_t)pml4_ptr, PML4_INDEX(virt), 1);
    if (pdpt == NULL) return;
    
    pte_t* pd = get_page_table((uint64_t)pdpt, PDPT_INDEX(virt), 1);
    if (pd == NULL) return;
    
    pte_t* pt = get_page_table((uint64_t)pd, PD_INDEX(virt), 1);
    if (pt == NULL) return;
    
    pte_t* entry = &pt[PT_INDEX(virt)];
    entry->value = 0;
    entry->fields.present = (flags & PAGE_PRESENT) ? 1 : 0;
    entry->fields.rw = (flags & PAGE_WRITABLE) ? 1 : 0;
    entry->fields.user = (flags & PAGE_USER) ? 1 : 0;
    entry->fields.xd = (flags & PAGE_NX) ? 1 : 0;
    entry->fields.frame = phys >> 12;
}

void vmm_destroy_page_table(uint64_t pml4) {
    pte_t* pml4_ptr = (pte_t*)pml4;
    
    for (int i = 256; i < 512; i++) {
        if (pml4_ptr[i].fields.present) {
            uint64_t pdpt_phys = pml4_ptr[i].fields.frame << 12;
            pte_t* pdpt = (pte_t*)pdpt_phys;
            
            for (int j = 0; j < 512; j++) {
                if (pdpt[j].fields.present) {
                    uint64_t pd_phys = pdpt[j].fields.frame << 12;
                    pte_t* pd = (pte_t*)pd_phys;
                    
                    for (int k = 0; k < 512; k++) {
                        if (pd[k].fields.present) {
                            uint64_t pt_phys = pd[k].fields.frame << 12;
                            pmm_free_page((void*)pt_phys);
                        }
                    }
                    pmm_free_page((void*)pd_phys);
                }
            }
            pmm_free_page((void*)pdpt_phys);
        }
    }
    
    pmm_free_page((void*)pml4);
}
