BITS 32

section .multiboot2
align 8

MULTIBOOT2_MAGIC     equ 0xE85250D6
MULTIBOOT2_ARCH_I386 equ 0
HEADER_LENGTH        equ multiboot2_header_end - multiboot2_header_start

multiboot2_header_start:
    dd MULTIBOOT2_MAGIC
    dd MULTIBOOT2_ARCH_I386
    dd HEADER_LENGTH
    dd -(MULTIBOOT2_MAGIC + MULTIBOOT2_ARCH_I386 + HEADER_LENGTH)
    
    dw 0
    dw 0
    dd 8
multiboot2_header_end:

section .bss
align 4096
pml4:
    resb 4096
pdpt:
    resb 4096
pd:
    resb 4096
stack_bottom:
    resb 65536
stack_top:

section .rodata
align 16
gdt64:
    dw gdt64_end - gdt64_start - 1
    dq gdt64_start

gdt64_start:
    dq 0
.code64:
    dq 0x00AF9A000000FFFF
.data64:
    dq 0x00CF92000000FFFF
gdt64_end:

section .text
global _start
extern kernel_main

_start:
    cli

    mov edi, pml4
    xor eax, eax
    mov ecx, 3072
    rep stosd

    lea edi, [pml4]
    lea eax, [pdpt]
    or eax, 3
    mov [edi], eax

    lea edi, [pdpt]
    lea eax, [pd]
    or eax, 3
    mov [edi], eax

    lea edi, [pd]
    mov eax, 0x83
    mov ecx, 512
.map_page:
    mov [edi], eax
    add eax, 0x200000
    add edi, 8
    dec ecx
    jnz .map_page

    lea eax, [pml4]
    mov cr3, eax

    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax

    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    mov eax, cr0
    or eax, 1 << 31
    mov cr0, eax

    lgdt [gdt64]

    jmp 0x08:long_mode_start

BITS 64
long_mode_start:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    mov rsp, stack_top

    call kernel_main

    cli
.halt:
    hlt
    jmp .halt

section .note.GNU-stack noalloc noexec nowrite progbits
