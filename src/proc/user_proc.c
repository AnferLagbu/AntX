#include "user_proc.h"
#include "proc.h"
#include "mm.h"
#include "gdt.h"
#include "serial.h"
#include "string.h"
#include "vfs.h"

static struct process *user_proc = NULL;

void user_proc_init(void) {
    user_proc = NULL;
    serial_puts(SERIAL_COM1, "User process manager initialized\n");
}

struct process* user_proc_create(struct user_proc_info *info, uint64_t pwid) {
    struct process *proc = process_create(NULL, 0, pwid);
    if (proc == NULL) {
        serial_puts(SERIAL_COM1, "Failed to create user process\n");
        return NULL;
    }
    
    proc->cr3 = vmm_create_user_page_table();
    if (proc->cr3 == 0) {
        process_exit(proc, 1);
        return NULL;
    }
    
    void *stack_pages = pmm_alloc_pages((USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE);
    if (stack_pages == NULL) {
        vmm_destroy_page_table(proc->cr3);
        process_exit(proc, 1);
        return NULL;
    }
    
    uint64_t stack_phys = (uint64_t)stack_pages;
    uint64_t stack_virt = USER_STACK_TOP - USER_STACK_SIZE - USER_STACK_GUARD;
    
    for (uint64_t i = 0; i < (USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE; i++) {
        vmm_map_page_in_table(proc->cr3, stack_virt + i * PAGE_SIZE, 
                              stack_phys + i * PAGE_SIZE, 
                              PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
    }
    
    proc->user_stack = USER_STACK_TOP;
    proc->kernel_stack = (uint64_t)pmm_alloc_page() + PAGE_SIZE;
    
    if (proc->kernel_stack == PAGE_SIZE) {
        vmm_destroy_page_table(proc->cr3);
        process_exit(proc, 1);
        return NULL;
    }
    
    proc->context.rip = (uint64_t)info->entry;
    proc->context.cs = GDT_USER_CODE | 0x03;
    proc->context.rflags = 0x202;
    proc->context.rsp = proc->user_stack;
    proc->context.ss = GDT_USER_DATA | 0x03;
    
    serial_puts(SERIAL_COM1, "[ELF] Checking kernel space mapping in user page table...\n");
    extern uint64_t kernel_pml4;
    pte_t* user_pml4 = (pte_t*)proc->cr3;
    pte_t* kern_pml4 = (pte_t*)kernel_pml4;
    serial_puts(SERIAL_COM1, "[ELF] User PML4[256] = 0x");
    serial_put_hex(SERIAL_COM1, user_pml4[256].value);
    serial_puts(SERIAL_COM1, " (Kernel PML4[256] = 0x");
    serial_put_hex(SERIAL_COM1, kern_pml4[256].value);
    serial_puts(SERIAL_COM1, ")\n");
    
    proc->state = PROC_READY;
    
    serial_puts(SERIAL_COM1, "User process created: ");
    serial_puts(SERIAL_COM1, info->name);
    serial_puts(SERIAL_COM1, "\n");
    
    return proc;
}

void user_proc_enter(struct process *proc) {
    if (proc == NULL) return;
    
    user_proc = proc;
    proc->state = PROC_RUNNING;
    
    tss_set_kernel_stack(proc->kernel_stack);
    vmm_switch_page_table(proc->cr3);
    
    uint64_t ss_val = GDT_USER_DATA | 0x03;
    uint64_t cs_val = GDT_USER_CODE | 0x03;
    uint64_t rip_val = proc->context.rip;
    uint64_t rsp_val = proc->context.rsp;
    uint64_t rflags_val = proc->context.rflags;
    
    __asm__ volatile (
        "cli\n"
        "movw %w0, %%ax\n"
        "movw %%ax, %%ds\n"
        "movw %%ax, %%es\n"
        "movw %%ax, %%fs\n"
        "movw %%ax, %%gs\n"
        "pushq %0\n"
        "pushq %1\n"
        "pushq %2\n"
        "pushq %3\n"
        "pushq %4\n"
        "iretq\n"
        :
        : "r"(ss_val), "r"(rsp_val), "r"(rflags_val), "r"(cs_val), "r"(rip_val)
        : "ax", "memory"
    );
}

void user_proc_return_to_kernel(void) {
    __asm__ volatile (
        "cli\n"
        "mov $0x10, %%ax\n"
        "mov %%ax, %%ds\n"
        "mov %%ax, %%es\n"
        "mov %%ax, %%fs\n"
        "mov %%ax, %%gs\n"
        "mov %%ax, %%ss\n"
        :
        :
        : "ax", "memory"
    );
}

int sys_proc_create_user(void (*entry)(void), uint64_t pwid) {
    struct user_proc_info info = {
        .entry = entry,
        .name = "user_program",
        .code_size = 0,
        .code_data = NULL
    };
    
    struct process *proc = user_proc_create(&info, pwid);
    if (proc == NULL) {
        return -1;
    }
    
    scheduler_add(proc);
    return proc->pid;
}

static uint8_t elf_read_buf[4096];

int user_proc_load_elf(const char *path, uint64_t pwid) {
    struct vfs_file *file = vfs_open(path, 0, pwid);
    if (file == NULL) {
        serial_puts(SERIAL_COM1, "ELF loader: file not found: ");
        serial_puts(SERIAL_COM1, path);
        serial_puts(SERIAL_COM1, "\n");
        return -1;
    }
    
    struct elf_header header;
    int bytes_read = vfs_read(file, &header, sizeof(header));
    if (bytes_read != sizeof(header)) {
        serial_puts(SERIAL_COM1, "ELF loader: failed to read header\n");
        vfs_close(file);
        return -1;
    }
    
    if (header.magic[0] != 0x7F || header.magic[1] != 'E' ||
        header.magic[2] != 'L' || header.magic[3] != 'F') {
        serial_puts(SERIAL_COM1, "ELF loader: invalid magic\n");
        vfs_close(file);
        return -1;
    }
    
    if (header.class != 2 || header.machine != 0x3E) {
        serial_puts(SERIAL_COM1, "ELF loader: not 64-bit x86\n");
        vfs_close(file);
        return -1;
    }
    
    struct process *proc = process_create(NULL, 0, pwid);
    if (proc == NULL) {
        vfs_close(file);
        return -1;
    }
    
    proc->cr3 = vmm_create_user_page_table();
    if (proc->cr3 == 0) {
        process_exit(proc, 1);
        vfs_close(file);
        return -1;
    }
    
    void *stack_pages = pmm_alloc_pages((USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE);
    if (stack_pages == NULL) {
        vmm_destroy_page_table(proc->cr3);
        process_exit(proc, 1);
        vfs_close(file);
        return -1;
    }
    
    uint64_t stack_phys = (uint64_t)stack_pages;
    uint64_t stack_virt = USER_STACK_TOP - USER_STACK_SIZE - USER_STACK_GUARD;
    
    for (uint64_t i = 0; i < (USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE; i++) {
        uint64_t vaddr = stack_virt + i * PAGE_SIZE;
        vmm_map_page_in_table(proc->cr3, vaddr,
                              stack_phys + i * PAGE_SIZE,
                              PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
    }
    
    proc->user_stack = USER_STACK_TOP;
    proc->kernel_stack = (uint64_t)pmm_alloc_page() + PAGE_SIZE;
    
    if (proc->kernel_stack == PAGE_SIZE) {
        vmm_destroy_page_table(proc->cr3);
        process_exit(proc, 1);
        vfs_close(file);
        return -1;
    }
    
    for (int i = 0; i < header.phnum; i++) {
        struct elf_phdr phdr;
        vfs_seek(file, header.phoff + i * header.phentsize, 0);
        vfs_read(file, &phdr, sizeof(phdr));
        
        if (phdr.p_type != PT_LOAD) continue;
        
        uint64_t vaddr_start = phdr.p_vaddr & ~0xFFF;
        uint64_t vaddr_end = (phdr.p_vaddr + phdr.p_memsz + 0xFFF) & ~0xFFF;
        uint64_t num_pages = (vaddr_end - vaddr_start) / PAGE_SIZE;
        
        for (uint64_t j = 0; j < num_pages; j++) {
            void *page = pmm_alloc_page();
            if (page == NULL) {
                vmm_destroy_page_table(proc->cr3);
                process_exit(proc, 1);
                vfs_close(file);
                return -1;
            }
            
            memset(page, 0, PAGE_SIZE);
            
            uint64_t flags = PAGE_PRESENT | PAGE_USER;
            if (phdr.p_flags & 0x02) flags |= PAGE_WRITABLE;
            
            vmm_map_page_in_table(proc->cr3, vaddr_start + j * PAGE_SIZE,
                                  (uint64_t)page, flags);
        }
        
        if (phdr.p_filesz > 0) {
            vfs_seek(file, phdr.p_offset, 0);
            
            uint64_t remaining = phdr.p_filesz;
            uint64_t dest_addr = phdr.p_vaddr;
            
            while (remaining > 0) {
                uint64_t chunk_size = (remaining > sizeof(elf_read_buf)) ? sizeof(elf_read_buf) : remaining;
                
                int read = vfs_read(file, elf_read_buf, chunk_size);
                if (read <= 0) break;
                
                for (int k = 0; k < read; k++) {
                    uint64_t addr = dest_addr + k;
                    uint64_t page_addr = addr & ~0xFFF;
                    uint64_t offset = addr & 0xFFF;
                    
                    uint64_t phys = 0;
                    pte_t* pml4_ptr = (pte_t*)proc->cr3;
                    pte_t* pdpt = (pte_t*)(uint64_t)(((pte_t*)pml4_ptr)[PML4_INDEX(page_addr)].fields.frame << 12);
                    if (pdpt == NULL) continue;
                    pte_t* pd = (pte_t*)(uint64_t)(pdpt[PDPT_INDEX(page_addr)].fields.frame << 12);
                    if (pd == NULL) continue;
                    pte_t* pt = (pte_t*)(uint64_t)(pd[PD_INDEX(page_addr)].fields.frame << 12);
                    if (pt == NULL) continue;
                    phys = (uint64_t)(pt[PT_INDEX(page_addr)].fields.frame << 12);
                    
                    if (phys != 0) {
                        uint8_t *ptr = (uint8_t*)(phys + offset);
                        *ptr = elf_read_buf[k];
                    }
                }
                
                dest_addr += read;
                remaining -= read;
            }
        }
    }
    
    vfs_close(file);
    
    serial_puts(SERIAL_COM1, "[ELF] Verifying page table for entry=0x");
    serial_put_hex(SERIAL_COM1, header.entry);
    serial_puts(SERIAL_COM1, " cr3=0x");
    serial_put_hex(SERIAL_COM1, proc->cr3);
    serial_puts(SERIAL_COM1, "\n");
    
    uint64_t entry_paddr = vmm_get_physical_in_table(proc->cr3, header.entry);
    serial_puts(SERIAL_COM1, "[ELF] Entry physical address: 0x");
    serial_put_hex(SERIAL_COM1, entry_paddr);
    if (entry_paddr == 0) {
        serial_puts(SERIAL_COM1, " [NOT MAPPED!]\n");
    } else {
        serial_puts(SERIAL_COM1, " [OK]\n");
    }
    
    uint64_t stack_paddr = vmm_get_physical_in_table(proc->cr3, proc->user_stack - 16);
    serial_puts(SERIAL_COM1, "[ELF] Stack physical address: 0x");
    serial_put_hex(SERIAL_COM1, stack_paddr);
    if (stack_paddr == 0) {
        serial_puts(SERIAL_COM1, " [NOT MAPPED!]\n");
    } else {
        serial_puts(SERIAL_COM1, " [OK]\n");
    }
    
    proc->context.rip = header.entry;
    proc->context.cs = GDT_USER_CODE | 0x03;
    proc->context.rflags = 0x202;
    proc->context.rsp = proc->user_stack;
    proc->context.ss = GDT_USER_DATA | 0x03;
    
    proc->state = PROC_READY;
    
    serial_puts(SERIAL_COM1, "ELF loaded: ");
    serial_puts(SERIAL_COM1, path);
    serial_puts(SERIAL_COM1, " entry=0x");
    serial_put_hex(SERIAL_COM1, header.entry);
    serial_puts(SERIAL_COM1, "\n");
    
    scheduler_add(proc);
    return proc->pid;
}

int user_proc_create_from_binary(const uint8_t *code, uint64_t code_size, uint64_t pwid) {
    struct process *proc = process_create(NULL, 0, pwid);
    if (proc == NULL) {
        serial_puts(SERIAL_COM1, "Failed to create user process from binary\n");
        return -1;
    }
    
    proc->cr3 = vmm_create_user_page_table();
    if (proc->cr3 == 0) {
        process_exit(proc, 1);
        return -1;
    }
    
    uint64_t num_code_pages = (code_size + PAGE_SIZE - 1) / PAGE_SIZE;
    for (uint64_t i = 0; i < num_code_pages; i++) {
        void *page = pmm_alloc_page();
        if (page == NULL) {
            vmm_destroy_page_table(proc->cr3);
            process_exit(proc, 1);
            return -1;
        }
        
        memset(page, 0, PAGE_SIZE);
        
        uint64_t copy_size = (code_size - i * PAGE_SIZE > PAGE_SIZE) ? PAGE_SIZE : (code_size - i * PAGE_SIZE);
        memcpy(page, code + i * PAGE_SIZE, copy_size);
        
        vmm_map_page_in_table(proc->cr3, USER_CODE_BASE + i * PAGE_SIZE,
                              (uint64_t)page, PAGE_PRESENT | PAGE_USER);
    }
    
    void *stack_pages = pmm_alloc_pages((USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE);
    if (stack_pages == NULL) {
        vmm_destroy_page_table(proc->cr3);
        process_exit(proc, 1);
        return -1;
    }
    
    uint64_t stack_phys = (uint64_t)stack_pages;
    uint64_t stack_virt = USER_STACK_TOP - USER_STACK_SIZE - USER_STACK_GUARD;
    
    for (uint64_t i = 0; i < (USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE; i++) {
        vmm_map_page_in_table(proc->cr3, stack_virt + i * PAGE_SIZE,
                              stack_phys + i * PAGE_SIZE,
                              PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
    }
    
    proc->user_stack = USER_STACK_TOP;
    proc->kernel_stack = (uint64_t)pmm_alloc_page() + PAGE_SIZE;
    
    if (proc->kernel_stack == PAGE_SIZE) {
        vmm_destroy_page_table(proc->cr3);
        process_exit(proc, 1);
        return -1;
    }
    
    uint64_t kstack_phys = proc->kernel_stack - PAGE_SIZE;
    vmm_map_page_in_table(proc->cr3, kstack_phys, kstack_phys, 
                          PAGE_PRESENT | PAGE_WRITABLE);
    
    proc->context.rip = USER_CODE_BASE;
    proc->context.cs = GDT_USER_CODE | 0x03;
    proc->context.rflags = 0x202;
    proc->context.rsp = proc->user_stack;
    proc->context.ss = GDT_USER_DATA | 0x03;
    
    proc->state = PROC_READY;
    
    serial_puts(SERIAL_COM1, "User process created from binary, entry=0x400000\n");
    
    scheduler_add(proc);
    return proc->pid;
}

int user_proc_load_elf_from_memory(const uint8_t *elf_data, uint64_t elf_size, uint64_t pwid) {
    if (elf_data == NULL || elf_size < sizeof(struct elf_header)) {
        serial_puts(SERIAL_COM1, "ELF loader: invalid parameters\n");
        return -1;
    }
    
    struct elf_header *header = (struct elf_header *)elf_data;
    
    if (header->magic[0] != 0x7F || header->magic[1] != 'E' ||
        header->magic[2] != 'L' || header->magic[3] != 'F') {
        serial_puts(SERIAL_COM1, "ELF loader: invalid magic\n");
        return -1;
    }
    
    if (header->class != 2 || header->machine != 0x3E) {
        serial_puts(SERIAL_COM1, "ELF loader: not 64-bit x86\n");
        return -1;
    }
    
    struct process *proc = process_create(NULL, 0, pwid);
    if (proc == NULL) {
        return -1;
    }
    
    proc->cr3 = vmm_create_user_page_table();
    if (proc->cr3 == 0) {
        process_exit(proc, 1);
        return -1;
    }
    
    void *stack_pages = pmm_alloc_pages((USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE);
    if (stack_pages == NULL) {
        vmm_destroy_page_table(proc->cr3);
        process_exit(proc, 1);
        return -1;
    }
    
    uint64_t stack_phys = (uint64_t)stack_pages;
    uint64_t stack_virt = USER_STACK_TOP - USER_STACK_SIZE - USER_STACK_GUARD;
    
    for (uint64_t i = 0; i < (USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE; i++) {
        vmm_map_page_in_table(proc->cr3, stack_virt + i * PAGE_SIZE,
                              stack_phys + i * PAGE_SIZE,
                              PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
    }
    
    proc->user_stack = USER_STACK_TOP;
    proc->kernel_stack = (uint64_t)pmm_alloc_page() + PAGE_SIZE;
    
    if (proc->kernel_stack == PAGE_SIZE) {
        vmm_destroy_page_table(proc->cr3);
        process_exit(proc, 1);
        return -1;
    }
    
    uint64_t kstack_phys = proc->kernel_stack - PAGE_SIZE;
    vmm_map_page_in_table(proc->cr3, kstack_phys, kstack_phys, 
                          PAGE_PRESENT | PAGE_WRITABLE);
    
    serial_puts(SERIAL_COM1, "[PROC] Kernel stack mapped: vaddr=0x");
    serial_put_hex(SERIAL_COM1, kstack_phys);
    serial_puts(SERIAL_COM1, " phys=0x");
    serial_put_hex(SERIAL_COM1, kstack_phys);
    serial_puts(SERIAL_COM1, "\n");
    
    for (int i = 0; i < header->phnum; i++) {
        struct elf_phdr *phdr = (struct elf_phdr *)(elf_data + header->phoff + i * header->phentsize);
        
        if (phdr->p_type != PT_LOAD) continue;
        
        uint64_t vaddr_start = phdr->p_vaddr & ~0xFFF;
        uint64_t vaddr_end = (phdr->p_vaddr + phdr->p_memsz + 0xFFF) & ~0xFFF;
        uint64_t num_pages = (vaddr_end - vaddr_start) / PAGE_SIZE;
        
        uint64_t *page_phys_list = (uint64_t *)0x100000;
        static uint64_t temp_page_storage[64];
        page_phys_list = temp_page_storage;
        
        for (uint64_t j = 0; j < num_pages && j < 64; j++) {
            void *page = pmm_alloc_page();
            if (page == NULL) {
                vmm_destroy_page_table(proc->cr3);
                process_exit(proc, 1);
                return -1;
            }
            
            page_phys_list[j] = (uint64_t)page;
            
            memset(page, 0, PAGE_SIZE);
            
            uint64_t flags = PAGE_PRESENT | PAGE_USER;
            if (phdr->p_flags & 0x02) flags |= PAGE_WRITABLE;
            
            vmm_map_page_in_table(proc->cr3, vaddr_start + j * PAGE_SIZE,
                                  (uint64_t)page, flags);
        }
        
        if (phdr->p_filesz > 0) {
            uint64_t first_page_idx = 0;
            uint64_t offset_in_first = phdr->p_vaddr & 0xFFF;
            
            for (uint64_t k = 0; k < phdr->p_filesz; k++) {
                uint64_t page_idx = (offset_in_first + k) / PAGE_SIZE;
                uint64_t offset_in_page = (offset_in_first + k) % PAGE_SIZE;
                
                if (page_idx < num_pages && page_idx < 64) {
                    uint64_t phys = page_phys_list[page_idx];
                    *((uint8_t*)phys + offset_in_page) = elf_data[phdr->p_offset + k];
                }
            }
        }
    }
    
    proc->context.rip = header->entry;
    proc->context.cs = GDT_USER_CODE | 0x03;
    proc->context.rflags = 0x202;
    proc->context.rsp = proc->user_stack;
    proc->context.ss = GDT_USER_DATA | 0x03;
    proc->context.cr3 = proc->cr3;
    
    serial_puts(SERIAL_COM1, "ELF: entry=0x");
    serial_put_hex(SERIAL_COM1, header->entry);
    serial_puts(SERIAL_COM1, " cr3=0x");
    serial_put_hex(SERIAL_COM1, proc->cr3);
    serial_puts(SERIAL_COM1, " stack=0x");
    serial_put_hex(SERIAL_COM1, proc->user_stack);
    serial_puts(SERIAL_COM1, "\n");
    
    uint64_t entry_paddr2 = vmm_get_physical_in_table(proc->cr3, header->entry);
    serial_puts(SERIAL_COM1, "[ELF] Entry phys: 0x");
    serial_put_hex(SERIAL_COM1, entry_paddr2);
    if (entry_paddr2 == 0) {
        serial_puts(SERIAL_COM1, " [NOT MAPPED!]\n");
    } else {
        serial_puts(SERIAL_COM1, " [OK]\n");
    }
    
    serial_puts(SERIAL_COM1, "[ELF] Dumping PTE for entry address:\n");
    uint64_t entry_addr = header->entry;
    pte_t* pml4_ptr = (pte_t*)proc->cr3;
    pte_t* pml4e = &pml4_ptr[PML4_INDEX(entry_addr)];
    serial_puts(SERIAL_COM1, "  PML4["); serial_put_dec(SERIAL_COM1, PML4_INDEX(entry_addr));
    serial_puts(SERIAL_COM1, "] = 0x"); serial_put_hex(SERIAL_COM1, pml4e->value); serial_puts(SERIAL_COM1, "\n");
    
    if (pml4e->fields.present) {
        pte_t* pdpt_ptr = (pte_t*)(uint64_t)(pml4e->fields.frame << 12);
        pte_t* pdpte = &pdpt_ptr[PDPT_INDEX(entry_addr)];
        serial_puts(SERIAL_COM1, "  PDPT["); serial_put_dec(SERIAL_COM1, PDPT_INDEX(entry_addr));
        serial_puts(SERIAL_COM1, "] = 0x"); serial_put_hex(SERIAL_COM1, pdpte->value); serial_puts(SERIAL_COM1, "\n");
        
        if (pdpte->fields.present) {
            pte_t* pd_ptr = (pte_t*)(uint64_t)(pdpte->fields.frame << 12);
            pte_t* pde = &pd_ptr[PD_INDEX(entry_addr)];
            serial_puts(SERIAL_COM1, "  PD["); serial_put_dec(SERIAL_COM1, PD_INDEX(entry_addr));
            serial_puts(SERIAL_COM1, "] = 0x"); serial_put_hex(SERIAL_COM1, pde->value); serial_puts(SERIAL_COM1, "\n");
            
            if (pde->fields.present) {
                pte_t* pt_ptr = (pte_t*)(uint64_t)(pde->fields.frame << 12);
                pte_t* pte = &pt_ptr[PT_INDEX(entry_addr)];
                serial_puts(SERIAL_COM1, "  PT["); serial_put_dec(SERIAL_COM1, PT_INDEX(entry_addr));
                serial_puts(SERIAL_COM1, "] = 0x"); serial_put_hex(SERIAL_COM1, pte->value); serial_puts(SERIAL_COM1, "\n");
                serial_puts(SERIAL_COM1, "  PTE flags: P="); serial_put_dec(SERIAL_COM1, pte->fields.present);
                serial_puts(SERIAL_COM1, " RW="); serial_put_dec(SERIAL_COM1, pte->fields.rw);
                serial_puts(SERIAL_COM1, " US="); serial_put_dec(SERIAL_COM1, pte->fields.user);
                serial_puts(SERIAL_COM1, " XD="); serial_put_dec(SERIAL_COM1, pte->fields.xd);
                serial_puts(SERIAL_COM1, "\n");
            }
        }
    }
    
    uint64_t stack_paddr2 = vmm_get_physical_in_table(proc->cr3, proc->user_stack - 16);
    serial_puts(SERIAL_COM1, "[ELF] Stack phys: 0x");
    serial_put_hex(SERIAL_COM1, stack_paddr2);
    if (stack_paddr2 == 0) {
        serial_puts(SERIAL_COM1, " [NOT MAPPED!]\n");
    } else {
        serial_puts(SERIAL_COM1, " [OK]\n");
    }
    
    serial_puts(SERIAL_COM1, "[ELF] Dumping PTE for stack address (push target):\n");
    uint64_t stack_addr = proc->user_stack - 8;
    pte_t* stack_pml4_ptr = (pte_t*)proc->cr3;
    pte_t* stack_pml4e = &stack_pml4_ptr[PML4_INDEX(stack_addr)];
    serial_puts(SERIAL_COM1, "  PML4["); serial_put_dec(SERIAL_COM1, PML4_INDEX(stack_addr));
    serial_puts(SERIAL_COM1, "] = 0x"); serial_put_hex(SERIAL_COM1, stack_pml4e->value); serial_puts(SERIAL_COM1, "\n");
    
    if (stack_pml4e->fields.present) {
        pte_t* stack_pdpt_ptr = (pte_t*)(uint64_t)(stack_pml4e->fields.frame << 12);
        pte_t* stack_pdpte = &stack_pdpt_ptr[PDPT_INDEX(stack_addr)];
        serial_puts(SERIAL_COM1, "  PDPT["); serial_put_dec(SERIAL_COM1, PDPT_INDEX(stack_addr));
        serial_puts(SERIAL_COM1, "] = 0x"); serial_put_hex(SERIAL_COM1, stack_pdpte->value); serial_puts(SERIAL_COM1, "\n");
        
        if (stack_pdpte->fields.present) {
            pte_t* stack_pd_ptr = (pte_t*)(uint64_t)(stack_pdpte->fields.frame << 12);
            pte_t* stack_pde = &stack_pd_ptr[PD_INDEX(stack_addr)];
            serial_puts(SERIAL_COM1, "  PD["); serial_put_dec(SERIAL_COM1, PD_INDEX(stack_addr));
            serial_puts(SERIAL_COM1, "] = 0x"); serial_put_hex(SERIAL_COM1, stack_pde->value); serial_puts(SERIAL_COM1, "\n");
            
            if (stack_pde->fields.present) {
                pte_t* stack_pt_ptr = (pte_t*)(uint64_t)(stack_pde->fields.frame << 12);
                pte_t* stack_pte = &stack_pt_ptr[PT_INDEX(stack_addr)];
                serial_puts(SERIAL_COM1, "  PT["); serial_put_dec(SERIAL_COM1, PT_INDEX(stack_addr));
                serial_puts(SERIAL_COM1, "] = 0x"); serial_put_hex(SERIAL_COM1, stack_pte->value); serial_puts(SERIAL_COM1, "\n");
                serial_puts(SERIAL_COM1, "  Stack PTE flags: P="); serial_put_dec(SERIAL_COM1, stack_pte->fields.present);
                serial_puts(SERIAL_COM1, " RW="); serial_put_dec(SERIAL_COM1, stack_pte->fields.rw);
                serial_puts(SERIAL_COM1, " US="); serial_put_dec(SERIAL_COM1, stack_pte->fields.user);
                serial_puts(SERIAL_COM1, " XD="); serial_put_dec(SERIAL_COM1, stack_pte->fields.xd);
                serial_puts(SERIAL_COM1, "\n");
            }
        }
    }
    
    proc->state = PROC_READY;
    
    serial_puts(SERIAL_COM1, "Process created: PID=");
    serial_put_dec(SERIAL_COM1, proc->pid);
    serial_puts(SERIAL_COM1, "\n");
    
    scheduler_add(proc);
    return proc->pid;
}
