BITS 64

section .text
global _kernel_entry
extern kernel_init

_kernel_entry:
    mov rax, kernel_init
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
