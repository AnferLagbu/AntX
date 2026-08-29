BITS 64

section .text
global process_switch_asm
global user_entry_trampoline
extern user_entry_target
extern user_entry_cr3
; P2.B + F-13 (DECISION-051): GDT 选择子强绑定.
; O-04 (proc/switch.asm:113 `mov ax, 0x23` 硬编码) 同期处理.
extern SELECTOR_USER_CODE

; void process_switch_asm(ProcessContext* prev, const ProcessContext* next);
; RDI = *mut ProcessContext (save current register state here)
; RSI = *const ProcessContext (load next register state from here)
;
; ProcessContext layout (each field 8 bytes):
;   +0:   r15
;   +8:   r14
;  +16:   r13
;  +24:   r12
;  +32:   rbx
;  +40:   rbp
;  +48:   rax
;  +56:   rip
;  +64:   rsp
;  +72:   rflags
;  +80:   cr3
;  +88:   cs
;  +96:   ds
; +104:   es
; +112:   fs
; +120:   gs
; +128:   ss
; +136:   _fpu_pad (8 bytes padding for 16-byte alignment)
; +144:   fpu_state[64] (512 bytes, Phase 2: fxsave/fxrstor)
; +656:   fpcr  (aarch64)
; +664:   fpsr  (aarch64)
; +672:   extra_regs[8] (B05-55: rdi/rsi/rdx/rcx/r8/r9/r10/r11, 首次进入用户态
;                      的进程 (fork 子进程) 由 iretq 前恢复, 见 types.rs)

process_switch_asm:
    cli

    ; Save current context to [RDI] (prev)
    mov [rdi + 0], r15
    mov [rdi + 8], r14
    mov [rdi + 16], r13
    mov [rdi + 24], r12
    mov [rdi + 32], rbx
    mov [rdi + 40], rbp
    mov [rdi + 48], rax

    ; Save rip, rsp, rflags from stack
    mov rax, [rsp]
    mov [rdi + 56], rax        ; rip (return address)
    lea rax, [rsp + 8]
    mov [rdi + 64], rax        ; rsp
    pushfq
    pop rax
    mov [rdi + 72], rax        ; rflags

    ; Save cr3
    mov rax, cr3
    mov [rdi + 80], rax

    ; Save segment registers
    mov [rdi + 88], cs
    mov [rdi + 96], ds
    mov [rdi + 104], es
    mov [rdi + 112], fs
    mov [rdi + 120], gs
    mov [rdi + 128], ss

    ; Save FPU/SSE state (fxsave requires 16-byte aligned memory)
    ; fpu_state is at offset 144 (17 fields + 1 padding = 18 * 8 = 144 bytes)
    lea rax, [rdi + 144]
    fxsave [rax]

    ; Restore next context from [RSI] (next)
    mov r15, [rsi + 0]
    mov r14, [rsi + 8]
    mov r13, [rsi + 16]
    mov r12, [rsi + 24]
    mov rbx, [rsi + 32]
    mov rbp, [rsi + 40]

    ; Set cr3
    mov rax, [rsi + 80]
    mov cr3, rax

    ; Restore segment registers (ds, es, fs, gs)
    ; cs and ss are restored via iretq frame
    ; B05-55 修复: 调度器上下文 (syscall/中断入口 swapgs 后) GS base=per_cpu,
    ; KERNEL_GS_BASE=0. 切到用户进程前先 swapgs, 使 KERNEL_GS_BASE=per_cpu —
    ; 否则用户态异常/中断入口 isr_common/irq_common 的 swapgs 会把 KERNEL_GS_BASE
    ; (0) 换入 GS base → [gs:KERNEL_PML4_OFF] 访问地址 8 → #PF → 死循环.
    ; GS base 随后由 mov gs (用户数据段 base=0) 恢复为 0 (用户 GS).
    ; 仅 next 为用户态 (cs=0x23) 时 swapgs; 内核线程切换 (cs=0x08) 不 swapgs.
    cmp word [rsi + 88], 0x23
    jne .no_swapgs_next
    swapgs
.no_swapgs_next:
    mov ax, [rsi + 96]
    mov ds, ax
    mov ax, [rsi + 104]
    mov es, ax
    mov ax, [rsi + 112]
    mov fs, ax
    mov ax, [rsi + 120]
    mov gs, ax

    ; Restore FPU/SSE state (fxrstor requires 16-byte aligned memory)
    ; fpu_state is at offset 144 (17 fields + 1 padding = 18 * 8 = 144 bytes)
    lea rax, [rsi + 144]
    fxrstor [rax]

    ; B05-55 修复: 切到 next 进程用户页表后, 当前栈 (per-CPU syscall_stack,
    ; 低 LMA) 在用户表中不可寻址. 参照 syscall_entry 返回路径, 将 RSP 转为
    ; 高半区直接映射别名 (KERNEL_BASE + RSP): 该别名经内核高半区恒等映射
    ; 在每个用户页表中均可见, push/iretq 才能在同一物理帧上执行.
    mov rax, 0xFFFF800000000000    ; KERNEL_BASE
    add rsp, rax

    ; Build iretq frame
    push qword [rsi + 128]      ; ss
    push qword [rsi + 64]       ; rsp
    push qword [rsi + 72]       ; rflags
    push qword [rsi + 88]       ; cs
    push qword [rsi + 56]       ; rip

    ; B05-55 修复: 恢复 caller-saved 寄存器 (rdi/rsi/rdx/rcx/r8-r11) — 仅用户态.
    ; ProcessContext 布局: fpu_state[64] @ 144 (512B), fpcr @ 656, fpsr @ 664,
    ; extra_regs[8] @ 672 (B05-55 新增, 见 services/proc/types.rs).
    ; 已运行过的进程返回用户态时, 寄存器由 syscall/中断栈的 InterruptFrame
    ; (schedule 后 iretq) 覆盖恢复, 此处不影响. 首次被调度的进程 (fork 子进程)
    ; 用这些继承的寄存器值 (fork 时父进程的 rdi 等) 进入用户态.
    ; ⚠ 必须放在所有 [rsi] (ProcessContext) 访问之后、iretq 之前, 且恢复 rax
    ; 之后 (rax 是 fork 返回值, 不能被覆盖).
    cmp word [rsi + 88], 0x23
    jne .no_restore_callersaved
    mov rax, rsi                ; rax 暂存 ctx 指针 (rsi 即将被覆盖)
    mov rdi, [rax + 672]        ; rdi
    mov rsi, [rax + 680]        ; rsi (从 [rax+...] 读, rax 保持 ctx)
    mov rdx, [rax + 688]
    mov rcx, [rax + 696]
    mov r8,  [rax + 704]
    mov r9,  [rax + 712]
    mov r10, [rax + 720]
    mov r11, [rax + 728]
    mov rax, [rax + 48]         ; rax = fork 返回值 (最后恢复)
    jmp .iretq_now
.no_restore_callersaved:
    ; Restore rax before iretq (内核线程切换)
    mov rax, [rsi + 48]
.iretq_now:
    iretq

user_entry_trampoline:
    ; P2.B + F-13 (DECISION-051 简化方案): CS = 用户代码段 (DPL=3).
    ; 字节长度与原 mov ax, 0x23 一致 (4 字节), 避免 label 偏移重定义.
    ; 单一来源: src/kernel/framework/link/x86_64.ld SELECTOR_USER_CODE_RPL3 与
    ; gdt.rs pub const SELECTOR_USER_CODE 同步 (host-tests 校验).
    mov ax, 0x23
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    mov rax, [rel user_entry_cr3]
    mov cr3, rax

    jmp [rel user_entry_target]

section .note.GNU-stack noalloc noexec nowrite progbits