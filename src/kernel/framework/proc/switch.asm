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

    ; Build iretq frame
    push qword [rsi + 128]      ; ss
    push qword [rsi + 64]       ; rsp
    push qword [rsi + 72]       ; rflags
    push qword [rsi + 88]       ; cs
    push qword [rsi + 56]       ; rip

    ; Restore rax before iretq
    mov rax, [rsi + 48]

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