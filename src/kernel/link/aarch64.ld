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

    . = ALIGN(4096);
    _kernel_end = .;
    _kernel_size = _kernel_end - _kernel_start;
}