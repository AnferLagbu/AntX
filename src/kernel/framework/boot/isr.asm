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

; 用户态 CR3 临时保存 (KPTI: 汇编在切换到内核页表前写入, Rust page fault handler 读取)
section .bss
align 8
global USER_CR3_SAVE
USER_CR3_SAVE: resq 1

; 切换回 .text 段, 后续代码必须在 .text 段 (不能在 .bss)
section .text

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
    ; ⚠ 关键修复 (TRACK-INIT-RING3): 必须在 push rax 之前检查 CS,
    ; 否则栈偏移会被诊断代码破坏.
    cmp word [rsp+24], 0x23
    jne .isr_no_kpti_enter
    swapgs

    ; ═══ 自检式调试: ISR KPTI swapgs 后 IA32_GS_BASE 验证 ═══
    ; 与 irq_common 'K' 诊断对称, 使用标记 'T'
    ; 输出: 'T' + IA32_GS_BASE (16 hex) + '!' 若零 (BUG 标记)
    push rax
    push rdx
    push rcx
    push r14
    push r15
    mov dx, 0x3F8
    mov al, 0x54                    ; 'T' - ISR KPTI swapgs 自检
    mov ecx, 0xC0000101            ; IA32_GS_BASE
    rdmsr                           ; EDX:EAX = IA32_GS_BASE
    shl rdx, 32
    or rdx, rax                     ; RDX = 完整 64 位值
    mov r14, rdx
    mov r15, 16
.isr_t_gs_loop:
    rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb .isr_t_gs_digit
    add al, 0x27
.isr_t_gs_digit:
    add al, 0x30
    mov dx, 0x3F8
    dec r15
    jnz .isr_t_gs_loop
    ; 自检: IA32_GS_BASE == 0 → 输出 '!' BUG 标记
    test r14, r14
    jnz .isr_t_gs_ok
    mov dx, 0x3F8
    mov al, 0x21                    ; '!' - BUG: swapgs 后 GS_BASE=0!
.isr_t_gs_ok:
    pop r15
    pop r14
    pop rcx
    pop rdx
    pop rax
    ; ═══ 自检式调试: GS_BASE 验证结束 ═══

    ; 保存用户 CR3: 硬件 CR3 此时仍是用户页表
    mov rax, cr3

    ; ═══ 自检式调试: USER_CR3_SAVE 写入前标记 ═══
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x58                    ; 'X' - ISR 即将写入 USER_CR3_SAVE
    pop rdx
    pop rax
    ; ═══ 自检式调试结束 ═══

    mov [USER_CR3_SAVE], rax

    ; ═══ 自检式调试: kernel_pml4 值验证 ═══
    ; 输出: 'U' + kernel_pml4 (16 hex) + '!' 若零
    ; 若有 'X' 但无 'U' → USER_CR3_SAVE 写入 #PF
    mov rax, [gs:KERNEL_PML4_OFF]
    push rax                        ; 保存 kernel_pml4 值
    push rdx
    push rcx
    push r14
    push r15
    mov r14, rax
    mov dx, 0x3F8
    mov al, 0x55                    ; 'U' - ISR kernel_pml4 自检
    mov r15, 16
.isr_u_pml4_loop:
    rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb .isr_u_pml4_digit
    add al, 0x27
.isr_u_pml4_digit:
    add al, 0x30
    mov dx, 0x3F8
    dec r15
    jnz .isr_u_pml4_loop
    ; 自检: kernel_pml4 == 0 → 输出 '!' BUG 标记
    test r14, r14
    jnz .isr_u_pml4_ok
    mov dx, 0x3F8
    mov al, 0x21                    ; '!' - BUG: kernel_pml4=0!
.isr_u_pml4_ok:
    pop r15
    pop r14
    pop rcx
    pop rdx
    pop rax                         ; 恢复 kernel_pml4 到 rax
    ; ═══ 自检式调试: kernel_pml4 验证结束 ═══

    mov cr3, rax

    ; ═══ 自检式调试: CR3 切换成功验证 ═══
    ; 输出: 'V' 标记 (若到达此处说明 CR3 切换成功)
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x56                    ; 'V' - ISR CR3 切换成功
    pop rdx
    pop rax
    ; ═══ 自检式调试: CR3 切换验证结束 ═══

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

    ; ═══ 自检式调试: 异常入口到达 (TRACK-INIT-RING3) ═══
    ; 在寄存器全部保存后输出 'E' + 异常向量号 (2 hex digits)
    ; #PF (vector 14=0x0E) 时额外输出 'P' + CR2 (16 hex) 故障地址
    ; 此时栈偏移安全, 不会影响 KPTI 检查 (已在上方完成)
    push rax
    push rdx
    push r14
    push r15
    mov dx, 0x3F8
    mov al, 0x45                    ; 'E' - exception entry
    ; 异常向量号: 原始栈上 [rsp+0] = int_no, 但 push 15 个寄存器后偏移 +120
    ; 加上 push rax/rdx/r14/r15 (4 个) 后偏移 +32, 所以 int_no 在 [rsp + 152]
    mov r14b, [rsp + 152]           ; int_no (低字节)
    ; 输出高 nibble
    mov al, r14b
    shr al, 4
    and al, 0x0F
    cmp al, 10
    jb .isr_e_hi_digit
    add al, 0x27
.isr_e_hi_digit:
    add al, 0x30
    mov dx, 0x3F8
    ; 输出低 nibble
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb .isr_e_lo_digit
    add al, 0x27
.isr_e_lo_digit:
    add al, 0x30
    mov dx, 0x3F8
    ; ── #PF (vector 14=0x0E) 特殊处理: 输出 CR2 故障地址 ──
    cmp r14b, 14
    jne .isr_e_no_pf
    mov al, 0x50                    ; 'P' - #PF CR2 标记
    mov dx, 0x3F8
    mov r14, cr2                    ; CR2 = 故障线性地址
    mov r15, 16
.isr_e_cr2_loop:
    rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb .isr_e_cr2_digit
    add al, 0x27
.isr_e_cr2_digit:
    add al, 0x30
    mov dx, 0x3F8
    dec r15
    jnz .isr_e_cr2_loop
    ; 自检: CR2 == 0 → 可能是空指针解引用, 输出 '!' 提示
    mov r14, cr2
    test r14, r14
    jnz .isr_e_no_pf
    mov dx, 0x3F8
    mov al, 0x21                    ; '!' - BUG: #PF CR2=0 (null ptr?)
.isr_e_no_pf:
    pop r15
    pop r14
    pop rdx
    pop rax
    ; ═══ 自检式调试结束 ═══

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

    ; ═══ 自检式调试: swapgs 前 MSR 值 (CR2=0x1 根因诊断) ═══
    ; 输出 'Y' + IA32_GS_BASE (16 hex) + IA32_KERNEL_GS_BASE (16 hex)
    ; 使用用户栈 (已映射) 保存/恢复寄存器, 不破坏 RAX (syscall 号)
    push rax
    push rdx
    push rcx
    push r14
    push r15

    mov dx, 0x3F8
    mov al, 0x59                    ; 'Y' - syscall 入口 MSR 自检

    mov ecx, 0xC0000101            ; IA32_GS_BASE
    rdmsr                           ; EDX:EAX = IA32_GS_BASE
    shl rdx, 32
    or rdx, rax                     ; RDX = 完整 64 位值
    mov r14, rdx
    mov r15, 16
.y_gs_loop:
    rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb .y_gs_digit
    add al, 0x27
.y_gs_digit:
    add al, 0x30
    mov dx, 0x3F8
    dec r15
    jnz .y_gs_loop

    mov ecx, 0xC0000102            ; IA32_KERNEL_GS_BASE
    rdmsr                           ; EDX:EAX = IA32_KERNEL_GS_BASE
    shl rdx, 32
    or rdx, rax                     ; RDX = 完整 64 位值
    mov r14, rdx
    mov r15, 16
.y_kgs_loop:
    rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb .y_kgs_digit
    add al, 0x27
.y_kgs_digit:
    add al, 0x30
    mov dx, 0x3F8
    dec r15
    jnz .y_kgs_loop

    pop r15
    pop r14
    pop rcx
    pop rdx
    pop rax
    ; ═══ 自检结束 ═══

    swapgs

    ; ═══ 自检式调试: swapgs 后 IA32_GS_BASE ═══
    ; 输出 'Z' + IA32_GS_BASE (16 hex)
    push rax
    push rdx
    push rcx
    push r14
    push r15

    mov dx, 0x3F8
    mov al, 0x5A                    ; 'Z' - swapgs 后 MSR 自检

    mov ecx, 0xC0000101            ; IA32_GS_BASE (swapgs 后)
    rdmsr
    shl rdx, 32
    or rdx, rax
    mov r14, rdx
    mov r15, 16
.z_gs_loop:
    rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb .z_gs_digit
    add al, 0x27
.z_gs_digit:
    add al, 0x30
    mov dx, 0x3F8
    dec r15
    jnz .z_gs_loop

    pop r15
    pop r14
    pop rcx
    pop rdx
    pop rax
    ; ═══ 自检结束 ═══

    ; ═══════════════════════════════════════════════════════════════════
    ; 教训 (TRACK-INIT-RING3-CR3): CR3 切换必须在栈切换之前.
    ; 此前流程: 先切内核栈 (mov rsp, r14) → push rax → 页错误.
    ; 根因: 内核栈 (syscall_stack) 不在用户页表中, 但此时 CR3 仍指向
    ; 用户页表, push 触发 #PF. 正确顺序: 先切 CR3 (mov cr3, r12)
    ; → 再切 RSP (mov rsp, r14), 确保 push 在内核页表保护下执行.
    ; ═══════════════════════════════════════════════════════════════════

    xor r15d, r15d                  ; R15 = 0 = KERNEL_RSP_OFF
    mov r14, [gs:r15]               ; R14 = kernel_rsp (暂存, CR3 切换后使用)

    ; 使用用户栈暂存 R12 作为 CR3 操作临时寄存器.
    ; push/pop 配对, 用户栈净效果为零, 中断已由 SFMASK 禁用.
    push r12                        ; (a) 保存用户 R12 到用户栈
    mov r12, cr3                    ; R12 = 用户 CR3
    mov [USER_CR3_SAVE], r12        ; 保存用户 CR3 (USER_CR3_SAVE 在用户页表中已映射)
    pop r12                         ; (b) 恢复用户 R12, 用户栈恢复原状

    mov [gs:r15], rsp               ; 保存用户 RSP (pop r12 后, 即原始值)

    mov r12, [gs:KERNEL_PML4_OFF]   ; R12 = 内核 PML4 物理地址
    mov cr3, r12                    ; ← 切换到内核页表 (此后所有访存走内核页表)

    mov rsp, r14                    ; 切换到内核 RSP (安全: 内核页表已加载)

    ; ── 诊断: 标记 syscall 入口到达 ──────────────────────────────
    push rax
    mov dx, 0x3F8
    mov al, 0x53                    ; 'S' - syscall 入口到达
    pop rax                         ; 恢复原始 RAX (syscall 号)

    ; 输出 syscall 号 (RAX), 16 个 hex 数字
    push rax                        ; 保存 syscall 号
    mov r14, rax                    ; r14 = syscall 号 (用于 hex 输出)
    mov r15, 16
.syscall_hex_loop:
    rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb .syscall_hex_digit
    add al, 0x27
.syscall_hex_digit:
    add al, 0x30
    mov dx, 0x3F8
    dec r15
    jnz .syscall_hex_loop
    pop rax                         ; 恢复 syscall 号到 RAX

    ; 构建 InterruptFrame (与 int 0x80 中断帧布局一致)
    push 0x1B                         ; SS = 用户数据段 (0x18|3)
    ; ── 诊断: 标记 push SS 完成 ──
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x31                    ; '1' - push SS done
    pop rdx
    pop rax

    push qword [gs:KERNEL_RSP_OFF]    ; 用户 RSP (xchg 时已存入 per-CPU)
    ; ── 诊断: 标记 push RSP 完成 ──
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x32                    ; '2' - push RSP done
    pop rdx
    pop rax

    push r11                          ; RFLAGS
    ; ── 诊断: 标记 push RFLAGS 完成 ──
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x33                    ; '3' - push RFLAGS done
    pop rdx
    pop rax

    push 0x23                         ; CS = 用户代码段 (0x20|3)
    ; ── 诊断: 标记 push CS 完成 ──
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x34                    ; '4' - push CS done
    pop rdx
    pop rax

    push rcx                          ; RIP
    ; ── 诊断: 标记 push RIP 完成 ──
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x35                    ; '5' - push RIP done
    pop rdx
    pop rax

    push 0                            ; err_code
    ; ── 诊断: 标记 push err_code 完成 ──
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x36                    ; '6' - push err_code done
    pop rdx
    pop rax

    push 0x80                         ; int_no
    ; ── 诊断: 标记 push int_no 完成 ──
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x37                    ; '7' - push int_no done
    pop rdx
    pop rax

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

    ; ── 诊断: syscall dispatch 返回 ──
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x64                    ; 'd' - dispatch returned
    pop rdx
    pop rax

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

    ; ═══ 自检式调试: IRQ KPTI swapgs 后 IA32_GS_BASE 验证 ═══
    ; swapgs 后: IA32_GS_BASE = per_cpu_addr (内核 per-CPU 数据)
    ; 若 IA32_GS_BASE = 0 → swapgs 前 IA32_KERNEL_GS_BASE = 0 → BUG
    ; 输出: 'K' + IA32_GS_BASE (16 hex) + '!' 若零 (BUG 标记)
    push rax
    push rdx
    push rcx
    push r14
    push r15
    mov dx, 0x3F8
    mov al, 0x4B                    ; 'K' - IRQ KPTI swapgs 自检
    mov ecx, 0xC0000101            ; IA32_GS_BASE
    rdmsr                           ; EDX:EAX = IA32_GS_BASE
    shl rdx, 32
    or rdx, rax                     ; RDX = 完整 64 位值
    mov r14, rdx
    mov r15, 16
.irq_k_gs_loop:
    rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb .irq_k_gs_digit
    add al, 0x27
.irq_k_gs_digit:
    add al, 0x30
    mov dx, 0x3F8
    dec r15
    jnz .irq_k_gs_loop
    ; 自检: IA32_GS_BASE == 0 → 输出 '!' BUG 标记
    test r14, r14
    jnz .irq_k_gs_ok
    mov dx, 0x3F8
    mov al, 0x21                    ; '!' - BUG: swapgs 后 GS_BASE=0!
.irq_k_gs_ok:
    pop r15
    pop r14
    pop rcx
    pop rdx
    pop rax
    ; ═══ 自检式调试: GS_BASE 验证结束 ═══

    ; 保存用户 CR3
    mov rax, cr3

    ; ═══ 自检式调试: USER_CR3_SAVE 写入前标记 ═══
    ; 若输出 'N' 后崩溃 → USER_CR3_SAVE 页面未映射到用户页表
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x4E                    ; 'N' - 即将写入 USER_CR3_SAVE
    pop rdx
    pop rax
    ; ═══ 自检式调试结束 ═══

    mov [USER_CR3_SAVE], rax

    ; ═══ 自检式调试: USER_CR3_SAVE 写入成功, 读取 kernel_pml4 ═══
    ; 输出 'L' + kernel_pml4 (16 hex) + '!' 若零
    ; 若输出 'N' 但无 'L' → USER_CR3_SAVE 写入触发 #PF (页面未映射)
    mov rax, [gs:KERNEL_PML4_OFF]
    push rax                        ; 保存 kernel_pml4 值 (后续 mov cr3 需要)
    push rdx
    push rcx
    push r14
    push r15
    mov r14, rax
    mov dx, 0x3F8
    mov al, 0x4C                    ; 'L' - kernel_pml4 自检
    mov r15, 16
.irq_l_pml4_loop:
    rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb .irq_l_pml4_digit
    add al, 0x27
.irq_l_pml4_digit:
    add al, 0x30
    mov dx, 0x3F8
    dec r15
    jnz .irq_l_pml4_loop
    ; 自检: kernel_pml4 == 0 → 输出 '!' BUG 标记
    test r14, r14
    jnz .irq_l_pml4_ok
    mov dx, 0x3F8
    mov al, 0x21                    ; '!' - BUG: kernel_pml4=0!
.irq_l_pml4_ok:
    pop r15
    pop r14
    pop rcx
    pop rdx
    pop rax                         ; 恢复 kernel_pml4 到 rax
    ; ═══ 自检式调试: kernel_pml4 验证结束 ═══

    ; 切换到内核页表
    mov cr3, rax

    ; ═══ 自检式调试: CR3 切换成功验证 ═══
    ; 切换后验证: 读取 [gs:0] 确认内核页表下 per-CPU 数据可访问
    ; 输出: 'M' 标记 (若到达此处说明 CR3 切换成功)
    ; 若有 'L' 但无 'M' → CR3 切换后内核页表无效 → Triple Fault
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x4D                    ; 'M' - CR3 切换成功
    mov rax, [gs:0]                 ; 验证内核页表下 per-CPU 可访问
    test rax, rax
    jnz .irq_cr3_ok
    mov al, 0x21                    ; '!' - BUG: CR3 切换后 [gs:0]=0!
.irq_cr3_ok:
    pop rdx
    pop rax
    ; ═══ 自检式调试: CR3 切换验证结束 ═══

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

    ; ═══ 自检式调试: IRQ 返回用户态 user_pml4 验证 ═══
    ; 输出: 'O' + user_pml4 (16 hex) + '!' 若零
    ; 若 user_pml4 ≠ 进程用户页表 → KPTI 返回到错误页表 → #PF
    push rax
    push rdx
    push rcx
    push r14
    push r15
    mov r14, [gs:USER_PML4_OFF]
    mov dx, 0x3F8
    mov al, 0x4F                    ; 'O' - IRQ 返回用户态 user_pml4 自检
    mov r15, 16
.irq_o_pml4_loop:
    rol r14, 4
    mov al, r14b
    and al, 0x0F
    cmp al, 10
    jb .irq_o_pml4_digit
    add al, 0x27
.irq_o_pml4_digit:
    add al, 0x30
    mov dx, 0x3F8
    dec r15
    jnz .irq_o_pml4_loop
    test r14, r14
    jnz .irq_o_pml4_ok
    mov dx, 0x3F8
    mov al, 0x21                    ; '!' - BUG: user_pml4=0!
.irq_o_pml4_ok:
    pop r15
    pop r14
    pop rcx
    pop rdx
    pop rax
    ; ═══ 自检式调试: user_pml4 验证结束 ═══

    mov rax, [gs:USER_PML4_OFF]
    mov cr3, rax
    swapgs
.irq_no_kpti_exit:

    ; ═══ 自检式调试: iretq 前标记 ═══
    ; 输出 'W' 标记 (若到达此处说明即将执行 iretq)
    ; 若有 'O' 但无 'W' → CR3 切换后/swapgs 后崩溃
    push rax
    push rdx
    mov dx, 0x3F8
    mov al, 0x57                    ; 'W' - iretq 前
    pop rdx
    pop rax
    ; ═══ 自检式调试结束 ═══

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
    ; 保存用户 CR3
    mov rax, cr3
    mov [USER_CR3_SAVE], rax
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
    ; 保存用户 CR3
    mov rax, cr3
    mov [USER_CR3_SAVE], rax
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
