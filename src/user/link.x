OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

SECTIONS
{
    . = 0x400000;

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

    .rela.dyn : {
        *(.rela.*)
    }

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame)
    }
}
