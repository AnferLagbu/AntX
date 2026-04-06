BITS 64

section .text
global process_switch_asm
global process_start_user_asm

process_switch_asm:
    mov [rdi + 0], r15
    mov [rdi + 8], r14
    mov [rdi + 16], r13
    mov [rdi + 24], r12
    mov [rdi + 32], r11
    mov [rdi + 40], r10
    mov [rdi + 48], r9
    mov [rdi + 56], r8
    
    mov [rdi + 64], rdi
    mov [rdi + 72], rsi
    mov [rdi + 80], rbp
    mov [rdi + 88], rbx
    mov [rdi + 96], rdx
    mov [rdi + 104], rcx
    mov [rdi + 112], rax
    
    mov rax, [rsp]
    mov [rdi + 120], rax
    
    mov ax, cs
    mov [rdi + 128], rax
    
    pushfq
    pop rax
    mov [rdi + 136], rax
    
    mov rax, rsp
    add rax, 8
    mov [rdi + 144], rax
    
    mov ax, ss
    mov [rdi + 152], rax
    
    mov r15, [rsi + 0]
    mov r14, [rsi + 8]
    mov r13, [rsi + 16]
    mov r12, [rsi + 24]
    mov r11, [rsi + 32]
    mov r10, [rsi + 40]
    mov r9, [rsi + 48]
    mov r8, [rsi + 56]
    
    mov rbp, [rsi + 80]
    mov rbx, [rsi + 88]
    mov rdx, [rsi + 96]
    mov rcx, [rsi + 104]
    mov rax, [rsi + 112]
    
    mov rsp, [rsi + 144]
    
    push qword [rsi + 152]
    push qword [rsi + 144]
    push qword [rsi + 136]
    push qword [rsi + 128]
    push qword [rsi + 120]
    
    mov rdi, [rsi + 64]
    mov rsi, [rsi + 72]
    
    iretq

process_start_user_asm:
    mov rsp, rdi
    
    cli
    
    mov rbx, rsi
    
    mov r12, [rbx + 160]
    
    mov r15, [rbx + 0]
    mov r14, [rbx + 8]
    mov r13, [rbx + 16]
    mov r11, [rbx + 32]
    mov r10, [rbx + 40]
    mov r9, [rbx + 48]
    mov r8, [rbx + 56]
    
    mov rbp, [rbx + 80]
    mov rdx, [rbx + 96]
    mov rcx, [rbx + 104]
    mov rax, [rbx + 112]
    mov rdi, [rbx + 64]
    mov rsi, [rbx + 72]
    
    push qword [rbx + 152]
    push qword [rbx + 144]
    push qword [rbx + 136]
    push qword [rbx + 128]
    push qword [rbx + 120]
    
    mov rbx, [rbx + 88]
    
    mov cr3, r12
    
    sti
    
    iretq
