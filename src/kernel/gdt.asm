BITS 64

section .text
global gdt_flush
global idt_flush
global tss_flush

gdt_flush:
    lgdt [rdi]
    
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    
    pop rdi
    mov rax, 0x08
    push rax
    push rdi
    retfq

idt_flush:
    lidt [rdi]
    ret

tss_flush:
    mov ax, 0x28
    ltr ax
    ret
