#ifndef _USER_PROC_H
#define _USER_PROC_H

#include "types.h"
#include "proc.h"

#define USER_STACK_TOP    0x7FFFFFFFFFFF
#define USER_STACK_SIZE   (64 * 1024)
#define USER_CODE_BASE    0x400000
#define USER_DATA_BASE    0x600000

struct user_proc_info {
    void (*entry)(void);
    const char *name;
    uint64_t code_size;
    uint8_t *code_data;
};

struct elf_header {
    uint8_t  magic[4];
    uint8_t  class;
    uint8_t  endian;
    uint8_t  version;
    uint8_t  os_abi;
    uint8_t  abi_version;
    uint8_t  padding[7];
    uint16_t type;
    uint16_t machine;
    uint32_t elf_version;
    uint64_t entry;
    uint64_t phoff;
    uint64_t shoff;
    uint32_t flags;
    uint16_t ehsize;
    uint16_t phentsize;
    uint16_t phnum;
    uint16_t shentsize;
    uint16_t shnum;
    uint16_t shstrndx;
} __attribute__((packed));

struct elf_phdr {
    uint32_t p_type;
    uint32_t p_flags;
    uint64_t p_offset;
    uint64_t p_vaddr;
    uint64_t p_paddr;
    uint64_t p_filesz;
    uint64_t p_memsz;
    uint64_t p_align;
} __attribute__((packed));

#define PT_LOAD 1

#define PML4_INDEX(addr) (((addr) >> 39) & 0x1FF)
#define PDPT_INDEX(addr) (((addr) >> 30) & 0x1FF)
#define PD_INDEX(addr)   (((addr) >> 21) & 0x1FF)
#define PT_INDEX(addr)   (((addr) >> 12) & 0x1FF)

void user_proc_init(void);
struct process* user_proc_create(struct user_proc_info *info, uint64_t pwid);
void user_proc_enter(struct process *proc);
void user_proc_return_to_kernel(void);

int sys_proc_create_user(void (*entry)(void), uint64_t pwid);
int user_proc_load_elf(const char *path, uint64_t pwid);
int user_proc_create_from_binary(const uint8_t *code, uint64_t code_size, uint64_t pwid);
int user_proc_load_elf_from_memory(const uint8_t *elf_data, uint64_t elf_size, uint64_t pwid);

extern struct user_proc_info user_programs[];

#endif
