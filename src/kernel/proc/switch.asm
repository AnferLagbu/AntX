BITS 64

section .text
global process_switch_asm
global user_entry_trampoline
extern user_entry_target
extern user_entry_cr3

; void process_switch_asm(ProcessContext* prev, const ProcessContext* next);
; RDI = *mut ProcessContext (save current register state here)
; RSI = *const ProcessContext (load next register state from here)
;
; ProcessContext layout (each field 8 bytes):
;   +0:   rip
;   +8:   rsp
;  +16:   rbp
;  +24:   rflags
;  +32:   cr3
;  +40:   rbx
;  +48:   r12
;  +56:   r13
;  +64:   r14
;  +72:   r15
;  +80:   cs
;  +88:   ds
;  +96:   es
; +104:   fs
; +112:   gs
; +120:   ss

process_switch_asm:
    cli

    mov rax, [rsp]
    mov [rdi + 0], rax
    lea rax, [rsp + 8]
    mov [rdi + 8], rax
    mov [rdi + 16], rbp
    pushfq
    pop rax
    mov [rdi + 24], rax
    mov rax, cr3
    mov [rdi + 32], rax
    mov [rdi + 40], rbx
    mov [rdi + 48], r12
    mov [rdi + 56], r13
    mov [rdi + 64], r14
    mov [rdi + 72], r15
    mov [rdi + 80], cs
    mov [rdi + 88], ds
    mov [rdi + 96], es
    mov [rdi + 104], fs
    mov [rdi + 112], gs
    mov [rdi + 120], ss

    mov r15, [rsi + 72]
    mov r14, [rsi + 64]
    mov r13, [rsi + 56]
    mov r12, [rsi + 48]
    mov rbx, [rsi + 40]
    mov rbp, [rsi + 16]

    push qword [rsi + 120]
    push qword [rsi + 8]
    push qword [rsi + 24]
    push qword [rsi + 80]
    push qword [rsi + 0]

    mov rax, [rsi + 32]
    mov cr3, rax

    iretq

user_entry_trampoline:
    mov ax, 0x23
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    mov rax, [rel user_entry_cr3]
    mov cr3, rax

    jmp [rel user_entry_target]

section .note.GNU-stack noalloc noexec nowrite progbits
