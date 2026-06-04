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

; ── SYSCALL 指令入口 (替代 int 0x80) ─────────────────────────────────────
; 控制流：
;   1. 用户态执行 syscall 指令
;   2. CPU 保存 RIP→RCX, RFLAGS→R11, 加载 CS=STAR[47:32], SS=STAR[47:32]+8
;   3. swapgs → GS 指向 per-CPU SyscallPerCpu 数据
;   4. xchg rsp, [gs:0] → 切换到该 CPU 独占的内核栈, 用户 RSP 存入 per-CPU
;   5. 构建 InterruptFrame, 调用 syscall_dispatch_from_frame
;   6. sysretq 返回用户态
;
; SMP 安全: 每个 CPU 有独立的 SyscallPerCpu 和内核栈,
; IA32_KERNEL_GS_BASE 在 gdt_init/gdt_init_ap 中分别设置。

; SyscallPerCpu.kernel_rsp 在结构体中的偏移 (字段顺序保证)
KERNEL_RSP_OFF equ 0

global syscall_entry
syscall_entry:
    swapgs
    xchg rsp, [gs:KERNEL_RSP_OFF]

    ; 构建 InterruptFrame (与 int 0x80 中断帧布局一致)
    push 0x1B                         ; SS = 用户数据段 (0x18|3)
    push qword [gs:KERNEL_RSP_OFF]    ; 用户 RSP (xchg 时已存入 per-CPU)
    push r11                          ; RFLAGS
    push 0x23                         ; CS = 用户代码段 (0x20|3)
    push rcx                          ; RIP

    push 0                            ; err_code
    push 0x80                         ; int_no

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

    add rsp, 16                       ; 跳过 int_no + err_code

    pop rcx                           ; 用户 RIP
    add rsp, 8                        ; 跳过 CS
    pop r11                           ; 用户 RFLAGS
    add rsp, 16                       ; 跳过 RSP + SS

    xchg rsp, [gs:KERNEL_RSP_OFF]     ; 恢复用户栈, 保存内核栈指针
    swapgs                            ; 恢复用户 GS 段
    sysretq

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
