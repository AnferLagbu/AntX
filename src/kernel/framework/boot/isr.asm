; =============================================================================
; isr.asm — x86_64 中断服务程序汇编 stub
;
; 为 IDT 初始化提供 32 ISR + 16 IRQ + syscall + recovery 入口。
; 栈帧布局与 InterruptFrame #[repr(C, packed)] 一致。
;
; ⚠ 教训 (TRACK-INIT-RING3): 绝对禁止在任何入口点 (syscall_entry,
; isr_common, irq_common) 的寄存器保存 (push) 之前插入修改通用寄存器
; 的调试代码 (如 out 0xe9, al)。入口点寄存器承载调用约定约定的值
; (syscall 号、异常码、中断向量), 修改即破坏, 不可恢复。
; 若需调试入口到达, 使用不修改寄存器的机制 (如内存写、LAPIC 调试)。
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
; 栈布局 (进入 isr_common 时):
;   [rsp+0]  = int_no
;   [rsp+8]  = err_code
;   [rsp+16] = RIP      (CPU 推入)
;   [rsp+24] = CS       (CPU 推入)
;   [rsp+32] = RFLAGS   (CPU 推入)
;   [rsp+40] = RSP      (CPU 推入)
;   [rsp+48] = SS       (CPU 推入)
isr_common:
    ; ── KPTI: 如果来自用户态, 切换到内核页表 ──────────────────────
    ; 检查栈上 CS: 用户代码段 = 0x23, 内核代码段 = 0x08
    ; 来自用户态时 GS 仍为用户 GS, 需要 swapgs 才能读 per-CPU PML4
    cmp word [rsp+24], 0x23
    jne .isr_no_kpti_enter
    swapgs
    mov rax, [gs:KERNEL_PML4_OFF]
    mov cr3, rax
    swapgs
.isr_no_kpti_enter:

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

    ; ── KPTI: 如果返回用户态, 切换到用户页表 ──────────────────────
    ; add rsp, 16 后栈布局: [rsp+0]=RIP, [rsp+8]=CS, [rsp+16]=RFLAGS
    ; CS 在 [rsp+8], 不是 [rsp+24] (入口时 CS 在 [rsp+24] 是因为
    ; ISR stub 推入了 int_no+err_code, 但 add rsp,16 已跳过它们)
    cmp word [rsp+8], 0x23
    jne .isr_no_kpti_exit
    swapgs
    mov rax, [gs:USER_PML4_OFF]
    mov cr3, rax
    swapgs
.isr_no_kpti_exit:

    iretq

; ── SYSCALL 指令入口 (替代 int 0x80) ─────────────────────────────────────
; 控制流：
;   1. 用户态执行 syscall 指令
;   2. CPU 保存 RIP→RCX, RFLAGS→R11, 加载 CS=STAR[47:32], SS=STAR[47:32]+8
;   3. swapgs → GS 指向 per-CPU SyscallPerCpu 数据
;   4. xchg rsp, [gs:0] → 切换到该 CPU 独占的内核栈, 用户 RSP 存入 per-CPU
;   5. 构建 InterruptFrame, 调用 syscall_dispatch_from_frame
;   6. iretq 返回用户态
;
; SMP 安全: 每个 CPU 有独立的 SyscallPerCpu 和内核栈,
; IA32_KERNEL_GS_BASE 在 gdt_init/gdt_init_ap 中分别设置。

; SyscallPerCpu 字段偏移 (与 gdt.rs SyscallPerCpu 结构体布局一致)
KERNEL_RSP_OFF  equ 0
KERNEL_PML4_OFF equ 8
USER_PML4_OFF   equ 16

global syscall_entry
syscall_entry:
    ; ═══════════════════════════════════════════════════════════════════
    ; 教训 (TRACK-INIT-RING3): 入口处绝对禁止修改任何通用寄存器
    ; 在 push/pop 保存上下文之前. 此前曾在此处插入调试代码
    ;   mov al, 0x53          ; 'S' → 破坏 RAX 低字节
    ;   out 0xe9, al
    ; 导致 RAX 中的 syscall 号被覆盖 (write=1 → 0x53=83=mkdir),
    ; 使 write syscall 被误判为 mkdir, 且因 mkdir 恰好也是 83
    ; 而未被发现. 入口点修改寄存器 = 破坏调用约定 = 不可恢复.
    ; ═══════════════════════════════════════════════════════════════════
    swapgs
    xchg rsp, [gs:KERNEL_RSP_OFF]

    ; ── 保存 RAX (含 syscall 号), 因为后续 KPTI 切换会破坏 RAX ──
    push rax

    ; ── KPTI: 切换到内核页表 ──────────────────────────────────────
    ; swapgs 后 GS 指向 per-CPU SyscallPerCpu, [gs:KERNEL_PML4_OFF]
    ; 含内核 PML4 物理地址. KPTI 未激活时 kernel_pml4 == user_pml4,
    ; 此 mov cr3 无实际切换效果.
    mov rax, [gs:KERNEL_PML4_OFF]
    mov cr3, rax

    ; 恢复 RAX (syscall 号)
    pop rax

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

    ; ── 返回路径: 使用 iretq 代替 sysretq ──────────────────────────
    ; 栈上已有完整的 iretq 帧: RIP, CS, RFLAGS, RSP, SS
    ; 不需要 xchg rsp 切换到用户栈 (iretq 从栈上读取 RSP),
    ; 避免在 Ring 0 使用用户栈时被中断导致中断帧推入用户栈.
    ;
    ; 栈布局 (add rsp, 16 后):
    ;   [rsp+0]  = RIP   (用户返回地址)
    ;   [rsp+8]  = CS    (0x23, 用户代码段)
    ;   [rsp+16] = RFLAGS
    ;   [rsp+24] = RSP   (用户栈)
    ;   [rsp+32] = SS    (0x1B, 用户数据段)

    cli                                ; 禁用中断: KPTI 切换 CR3 期间不可中断

    ; ── KPTI: 切换到用户页表 ──────────────────────────────────────
    ; iretq 前 CR3 必须切回 USER_PML4, 否则用户态无法寻址.
    ; 当前在内核栈上, [gs:OFF] 可安全访问.
    ;
    ; 教训: mov cr3, rax 会覆盖 RAX, 入口路径有 push/pop rax 保护,
    ; 但退出路径此前遗漏了该保护, 导致所有 syscall 返回值被用户页表
    ; 物理地址覆盖, 表现为用户态看到随机的 "成功" 返回值.
    push rax                           ; 保护 syscall 返回值
    mov rax, [gs:USER_PML4_OFF]
    mov cr3, rax
    pop rax                            ; 恢复 syscall 返回值

    swapgs                            ; 恢复用户 GS 段
    iretq

; ── 通用入口: 保存寄存器 → irq_handler ──────────────────────────────────
; 栈布局同 isr_common
irq_common:
    ; ── KPTI: 如果来自用户态, 切换到内核页表 ──────────────────────
    cmp word [rsp+24], 0x23
    jne .irq_no_kpti_enter
    swapgs
    mov rax, [gs:KERNEL_PML4_OFF]
    mov cr3, rax
    swapgs
.irq_no_kpti_enter:

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

    ; ── KPTI: 如果返回用户态, 切换到用户页表 ──────────────────────
    ; add rsp, 16 后栈布局: [rsp+0]=RIP, [rsp+8]=CS, [rsp+16]=RFLAGS
    ; CS 在 [rsp+8], 不是 [rsp+24] (入口时 CS 在 [rsp+24] 是因为
    ; ISR stub 推入了 int_no+err_code, 但 add rsp,16 已跳过它们)
    cmp word [rsp+8], 0x23
    jne .irq_no_kpti_exit
    swapgs
    mov rax, [gs:USER_PML4_OFF]
    mov cr3, rax
    swapgs
.irq_no_kpti_exit:

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

; ── MSI 向量 (0x40-0x7F) ─────────────────────────────────────────────
; 64 个 MSI 向量 stub, 使用 irq_common 入口 → irq_handler FFI
irq_stub 16, 64
irq_stub 17, 65
irq_stub 18, 66
irq_stub 19, 67
irq_stub 20, 68
irq_stub 21, 69
irq_stub 22, 70
irq_stub 23, 71
irq_stub 24, 72
irq_stub 25, 73
irq_stub 26, 74
irq_stub 27, 75
irq_stub 28, 76
irq_stub 29, 77
irq_stub 30, 78
irq_stub 31, 79
irq_stub 32, 80
irq_stub 33, 81
irq_stub 34, 82
irq_stub 35, 83
irq_stub 36, 84
irq_stub 37, 85
irq_stub 38, 86
irq_stub 39, 87
irq_stub 40, 88
irq_stub 41, 89
irq_stub 42, 90
irq_stub 43, 91
irq_stub 44, 92
irq_stub 45, 93
irq_stub 46, 94
irq_stub 47, 95
irq_stub 48, 96
irq_stub 49, 97
irq_stub 50, 98
irq_stub 51, 99
irq_stub 52, 100
irq_stub 53, 101
irq_stub 54, 102
irq_stub 55, 103
irq_stub 56, 104
irq_stub 57, 105
irq_stub 58, 106
irq_stub 59, 107
irq_stub 60, 108
irq_stub 61, 109
irq_stub 62, 110
irq_stub 63, 111
irq_stub 64, 112
irq_stub 65, 113
irq_stub 66, 114
irq_stub 67, 115
irq_stub 68, 116
irq_stub 69, 117
irq_stub 70, 118
irq_stub 71, 119
irq_stub 72, 120
irq_stub 73, 121
irq_stub 74, 122
irq_stub 75, 123
irq_stub 76, 124
irq_stub 77, 125
irq_stub 78, 126
irq_stub 79, 127

; ── syscall / recovery ─────────────────────────────────────────────────
extern syscall_dispatch_from_frame

global syscall_handler
syscall_handler:
    cli
    push 0
    push 0x80

    ; ── KPTI: 如果来自用户态, 切换到内核页表 ──────────────────────
    cmp word [rsp+24], 0x23
    jne .syscall_handler_no_kpti_enter
    swapgs
    mov rax, [gs:KERNEL_PML4_OFF]
    mov cr3, rax
    swapgs
.syscall_handler_no_kpti_enter:

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

    ; ── KPTI: 如果返回用户态, 切换到用户页表 ──────────────────────
    ; add rsp, 16 后栈布局: [rsp+0]=RIP, [rsp+8]=CS, [rsp+16]=RFLAGS
    ; CS 在 [rsp+8], 不是 [rsp+24]
    cmp word [rsp+8], 0x23
    jne .syscall_handler_no_kpti_exit
    swapgs
    mov rax, [gs:USER_PML4_OFF]
    mov cr3, rax
    swapgs
.syscall_handler_no_kpti_exit:

    iretq

global isr0x82
isr0x82:
    cli
    push 0
    push 0x82

    ; ── KPTI: 如果来自用户态, 切换到内核页表 ──────────────────────
    cmp word [rsp+24], 0x23
    jne .isr0x82_no_kpti_enter
    swapgs
    mov rax, [gs:KERNEL_PML4_OFF]
    mov cr3, rax
    swapgs
.isr0x82_no_kpti_enter:

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

    ; ── KPTI: 如果返回用户态, 切换到用户页表 ──────────────────────
    ; add rsp, 16 后栈布局: [rsp+0]=RIP, [rsp+8]=CS, [rsp+16]=RFLAGS
    ; CS 在 [rsp+8], 不是 [rsp+24]
    cmp word [rsp+8], 0x23
    jne .isr0x82_no_kpti_exit
    swapgs
    mov rax, [gs:USER_PML4_OFF]
    mov cr3, rax
    swapgs
.isr0x82_no_kpti_exit:

    iretq
