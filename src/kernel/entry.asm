BITS 64

section .text
global _kernel_entry
extern kernel_main

_kernel_entry:
    mov dx, 0x3F8
    mov al, 'K'
    out dx, al

    mov al, 'E'
    out dx, al

    mov al, 'N'
    out dx, al

    mov al, 'T'
    out dx, al

    mov al, 'R'
    out dx, al

    mov al, 'Y'
    out dx, al

    mov al, '\n'
    out dx, al

    mov rax, kernel_main
    call rax

    mov al, 'R'
    out dx, al

    mov al, 'E'
    out dx, al

    mov al, 'T'
    out dx, al

    mov al, 'U'
    out dx, al

    mov al, 'R'
    out dx, al

    mov al, 'N'
    out dx, al

    cli
.halt:
    hlt
    jmp .halt
