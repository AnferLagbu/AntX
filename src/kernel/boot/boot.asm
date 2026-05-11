BITS 32

section .multiboot1
align 4

MULTIBOOT1_MAGIC    equ 0x1BADB002
MULTIBOOT1_FLAGS    equ 0x00010003
MULTIBOOT1_CHECKSUM equ -(MULTIBOOT1_MAGIC + MULTIBOOT1_FLAGS)

multiboot1_header:
    dd MULTIBOOT1_MAGIC
    dd MULTIBOOT1_FLAGS
    dd MULTIBOOT1_CHECKSUM
    dd multiboot1_header
    dd 0x100000
    dd 0
    dd 0
    dd _start

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

KERNEL_PHYS_BASE     equ 0x100000
KERNEL_VIRT_BASE     equ 0xFFFF800001000000

section .bootbss
align 4096
pml4:
    resb 4096
pdpt_low:
    resb 4096
pdpt_high:
    resb 4096
pd_low:
    resb 4096
pd_high:
    resb 4096
gdt64:
    resb 30
align 16
stack_bottom:
    resb 65536
global stack_top
stack_top:

kernel_info:
    resq 3

section .text
global _start
extern kernel_init
extern stack_top

_start:
    cli

    call .get_eip
.get_eip:
    pop ebx
    sub ebx, .get_eip - _start

    lea edi, [ebx + (pml4 - _start)]
    xor eax, eax
    mov ecx, 6144
    rep stosd

    lea edi, [ebx + (pml4 - _start)]
    lea eax, [ebx + (pdpt_low - _start)]
    or eax, 3
    xor edx, edx
    mov [edi], eax
    mov [edi + 4], edx

    lea edi, [ebx + (pml4 - _start) + 256 * 8]
    lea eax, [ebx + (pdpt_high - _start)]
    or eax, 3
    mov [edi], eax
    mov [edi + 4], edx

    lea edi, [ebx + (pdpt_low - _start)]
    lea eax, [ebx + (pd_low - _start)]
    or eax, 3
    mov [edi], eax
    mov [edi + 4], edx

    lea edi, [ebx + (pdpt_high - _start)]
    lea eax, [ebx + (pd_high - _start)]
    or eax, 3
    mov [edi], eax
    mov [edi + 4], edx

    lea edi, [ebx + (pd_low - _start)]
    mov eax, 0x87
    xor edx, edx
    mov ecx, 512
.map_low:
    mov [edi], eax
    mov [edi + 4], edx
    add eax, 0x200000
    adc edx, 0
    add edi, 8
    dec ecx
    jnz .map_low

    lea edi, [ebx + (pd_high - _start)]
    mov eax, 0x87
    xor edx, edx
    mov ecx, 512
.map_high:
    mov [edi], eax
    mov [edi + 4], edx
    add eax, 0x200000
    adc edx, 0
    add edi, 8
    dec ecx
    jnz .map_high

    lea edi, [ebx + (gdt64 - _start)]
    mov word [edi], 23
    lea eax, [ebx + (gdt64 - _start + 6)]
    mov dword [edi + 2], eax

    mov dword [edi + 6], 0
    mov dword [edi + 10], 0
    mov dword [edi + 14], 0x0000FFFF
    mov dword [edi + 18], 0x00AF9A00
    mov dword [edi + 22], 0x0000FFFF
    mov dword [edi + 26], 0x00CF9200

    lgdt [edi]

    lea eax, [ebx + (pml4 - _start)]
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

    lea eax, [ebx + (trampoline64 - _start)]
    push dword 0x08
    push eax
    retf

BITS 64
trampoline64:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    jmp trampoline64_high

BITS 64
trampoline64_high:
    mov rsp, qword stack_top

    call kernel_init

    cli
.halt:
    hlt
    jmp .halt

section .note.GNU-stack noalloc noexec nowrite progbits
