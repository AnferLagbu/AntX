; antx-stage1.asm — AntX Stage 1 Bootloader
;
; BIOS → sector 0 (0x7C00), 读内核 → 0x100000, E820 → Multiboot2 info,
; 切换保护模式 → 跳转内核入口。大小 < 440 字节。

BITS 16
ORG 0x7C00

KERNEL_LOAD    equ 0x100000
SECT_PER_READ  equ 64
TOTAL_SECT     equ 2047
MB2_INFO       equ 0x9000
E820_BUF       equ 0x8C00
MAGIC          equ 0xE85250D6

start:
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    mov [drive], dl

    mov cx, (TOTAL_SECT + SECT_PER_READ - 1) / SECT_PER_READ
    mov dword [da_lba], 1
    mov dword [da_lba+4], 0
    mov word [da_buf], KERNEL_LOAD & 0xFFFF
    mov word [da_buf+2], (KERNEL_LOAD >> 16) & 0xFFFF

.rd:
    push cx
    mov ah, 0x42
    mov dl, [drive]
    mov si, dap
    int 0x13
    jc err
    add dword [da_lba], SECT_PER_READ
    adc dword [da_lba+4], 0
    add word [da_buf], SECT_PER_READ * 512
    adc word [da_buf+2], 0
    pop cx
    loop .rd

    ; 魔数校验移入保护模式 (32-bit 平坦寻址)，16-bit 模式下无法直接访问 0x100000+

    xor ebx, ebx
    mov di, E820_BUF
    xor bp, bp

.e820:
    mov eax, 0xE820
    mov ecx, 24
    mov edx, 0x534D4150
    int 0x15
    jc .e820_done
    cmp eax, 0x534D4150
    jne .e820_done
    inc bp
    add di, 24
    cmp ebx, 0
    jne .e820

.e820_done:
    xor ax, ax
    mov es, ax

    mov edi, MB2_INFO
    movzx eax, bp
    mov ecx, eax
    shl eax, 4
    shl ecx, 3
    add eax, ecx
    add eax, 32
    stosd
    xor eax, eax
    stosd

    mov eax, 6
    stosd
    movzx eax, bp
    mov ecx, eax
    shl eax, 4
    shl ecx, 3
    add eax, ecx
    add eax, 16
    stosd
    mov eax, 24
    stosd
    xor eax, eax
    stosd

    movzx ecx, bp
    shl ecx, 2
    add ecx, ecx
    mov esi, E820_BUF
    a32 rep movsd

    xor eax, eax
    stosd
    mov eax, 8
    stosd

    mov eax, 0x36D76289
    mov ebx, MB2_INFO
    cli
    lgdt [gdtr]
    mov ecx, cr0
    or cl, 1
    mov cr0, ecx
    jmp 0x08:.pm32

BITS 32
.pm32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    
    ; 在保护模式下校验 Multiboot2 魔数 (flat binary 偏移 40)
    cmp dword [KERNEL_LOAD + 40], MAGIC
    jne err_pm
    
    ; 从 Multiboot1 头读取入口地址 (偏移 8 + 28 = 36)
    mov eax, [KERNEL_LOAD + 36]
    push 0x08
    push eax
    retf

err_pm:
    hlt
    jmp err_pm

err:
    mov si, msg
.loop:
    lodsb
    or al, al
    jz halt
    mov ah, 0x0E
    mov bx, 7
    int 0x10
    jmp .loop

halt:
    hlt
    jmp halt

msg: db "ERR", 0

align 4
dap:
    db 0x10
    db 0
    dw SECT_PER_READ
da_buf: dd 0, 0
da_lba: dq 0
drive:  db 0

align 8
gdt:
    dq 0
    dq 0x00CF9A000000FFFF
    dq 0x00CF92000000FFFF
gdt_end:

gdtr:
    dw gdt_end - gdt - 1
    dd gdt

times 440 - ($ - $$) db 0
