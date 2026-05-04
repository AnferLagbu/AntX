; minimal.asm — 最小化用户态测试二进制
; 调用 SYS_PROC_EXIT(42) 后立即退出
; 用于验证 ring3 上下文切换和系统调用往返

BITS 64
global _start

section .text
_start:
    mov rax, 2        ; SYS_PROC_EXIT
    mov rbx, 42       ; exit code
    int 0x80          ; 系统调用
    jmp $             ; 不应到达此处
