; =============================================================================
; isr.asm — x86_64 中断服务程序汇编 stub
;
; 为 IDT 初始化提供 32 ISR + 16 IRQ + syscall + recovery 入口。
; 栈帧布局与 InterruptFrame #[repr(C, packed)] 一致。
; =============================================================================

BITS 64

section .text

extern exception_handler
extern irq_handler

; ── 通用 ISR stub (无 CPU 错误码) ───────────────────────────────────────
%macro isr_noerr 1
global isr%1
isr%1:
    cli
    push 0
    push %1
    jmp isr_common
%endmacro

; ── ISR stub (CPU 已推入错误码: 8,10-14,17) ────────────────────────────
%macro isr_err 1
global isr%1
isr%1:
    cli
    push %1
    jmp isr_common
%endmacro

; ── 通用 IRQ stub ───────────────────────────────────────────────────────
%macro irq_stub 2
global irq%1
irq%1:
    cli
    push 0
    push %2
    jmp irq_common
%endmacro

; ── 通用入口: 保存寄存器 → exception_handler ────────────────────────────
isr_common:
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp
    call exception_handler

    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rbp
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    iretq

; ── 通用入口: 保存寄存器 → irq_handler ──────────────────────────────────
irq_common:
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp
    call irq_handler

    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rbp
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    iretq

; ── 实例化 ──────────────────────────────────────────────────────────────
isr_noerr 0
isr_noerr 1
isr_noerr 2
isr_noerr 3
isr_noerr 4
isr_noerr 5
isr_noerr 6
isr_noerr 7
isr_err   8
isr_noerr 9
isr_err   10
isr_err   11
isr_err   12
isr_err   13
isr_err   14
isr_noerr 15
isr_noerr 16
isr_err   17
isr_noerr 18
isr_noerr 19
isr_noerr 20
isr_noerr 21
isr_noerr 22
isr_noerr 23
isr_noerr 24
isr_noerr 25
isr_noerr 26
isr_noerr 27
isr_noerr 28
isr_noerr 29
isr_noerr 30
isr_noerr 31

irq_stub 0,  32
irq_stub 1,  33
irq_stub 2,  34
irq_stub 3,  35
irq_stub 4,  36
irq_stub 5,  37
irq_stub 6,  38
irq_stub 7,  39
irq_stub 8,  40
irq_stub 9,  41
irq_stub 10, 42
irq_stub 11, 43
irq_stub 12, 44
irq_stub 13, 45
irq_stub 14, 46
irq_stub 15, 47

; ── syscall / recovery ─────────────────────────────────────────────────
extern syscall_dispatch_from_frame

global syscall_handler
syscall_handler:
    cli
    push 0
    push 0x80
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp
    cld
    call syscall_dispatch_from_frame

    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rbp
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    iretq

global isr0x82
isr0x82:
    cli
    push 0
    push 0x82
    push rax
    push rbx
    push rcx
    push rdx
    push rbp
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    mov rdi, rsp
    cld
    call exception_handler

    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rbp
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    iretq
