OUTPUT_FORMAT("elf64-littleaarch64")
OUTPUT_ARCH(aarch64)
ENTRY(_start)

PHDRS
{
    boot PT_LOAD FLAGS(7);
    kernel PT_LOAD FLAGS(7);
}

SECTIONS
{
    . = 0x40080000;

    _kernel_start = .;

    .text.boot : {
        *(.text.boot)
    } :boot

    . = ALIGN(4096);
    .text : {
        _kernel_text_start = .;
        *(.text)
        *(.text.*)
        _kernel_text_end = .;
    } :kernel

    . = ALIGN(2048);
    .vectors : {
        *(.vectors)
    } :kernel

    . = ALIGN(4096);
    .rodata : {
        *(.rodata)
        *(.rodata.*)
    } :kernel

    . = ALIGN(4096);
    .data : {
        *(.data)
        *(.data.*)
    } :kernel

    . = ALIGN(4096);
    __bss_start = .;
    .bss (NOLOAD) : {
        *(.bss)
        *(.bss.*)
        *(COMMON)
    } :kernel

    __bss_end = .;

    . = ALIGN(4096);
    _kernel_end = .;
    /* P1.D + F-03: Distinguish VMA/LMA semantics (both equal on aarch64
     * because VMA == LMA in the low-half kernel mapping region). */
    _kernel_size_vma = _kernel_end - _kernel_start;
    _kernel_size_lma = _kernel_end - _kernel_start;
    _kernel_size = _kernel_size_lma;
}