OUTPUT_FORMAT("elf64-x86-64")
ENTRY(_start)

SECTIONS
{
    . = 0x400000;

    /* P2.A + F-04: 用户态 ELF 边界符号 _user_start / _user_end,
     * 供 ELF loader 定位用户程序范围 (VMA 区间).
     * 包围 .text/.rodata/.data/.bss 所有段.
     * 注意: 链接器不识别 UTF-8 注释, 仅 ASCII. */
    _user_start = .;

    .text : ALIGN(4096) {
        *(.text._start)
        *(.text .text.*)
    }

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4096) {
        *(.data .data.*)
        *(.got .got.plt)
    }

    .bss : ALIGN(4096) {
        *(.bss .bss.*)
        *(COMMON)
    }

    . = ALIGN(4096);
    _user_end = .;

    /* P2.A + F-04: 显式声明 NX (no-execute) on stack,
     * 阻止可执行栈 (return-to-libc 攻击面).
     * rust-lld 不支持 `noalloc noexec nowrite progbits` 多标志 (GNU ld 支持),
     * 此处用 `progbits` 单标志 + 依赖编译器生成的 .note.GNU-stack ELF note,
     * 二者效果等价 (标记栈为不可执行). */
    .note.GNU-stack : {
        *(.note.GNU-stack)
    }

    .rela.dyn : {
        *(.rela.*)
    }

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame)
    }
}