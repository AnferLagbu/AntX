OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

PHDRS
{
    boot PT_LOAD FLAGS(7);
    kernel PT_LOAD FLAGS(7);
}

SECTIONS
{
    . = 0x100000;
    
    _kernel_start = .;
    
    .stage1_entry : {
        *(.stage1_entry)
    } :boot
    
    .multiboot1 : {
        *(.multiboot1)
    } :boot
    
    .multiboot2 : {
        *(.multiboot2)
    } :boot
    
    . = ALIGN(4096);
    .boot.text : {
        build/boot.o(.text)
        __boot_text_end = .;
    } :boot
    
    . = ALIGN(4096);
    .bootbss (NOLOAD) : {
        *(.bootbss)
    } :boot
    
    . = ALIGN(4096);
    _boot_end = .;
    
    _kernel_text_lma = .;
    _kernel_text_vma = 0xFFFF800001000000 + .;
    
    .text : AT(_kernel_text_lma) {
        _kernel_text_start = .;
        *(.text)
        *(.text.*)
        *(.trampoline)
        _kernel_text_end = .;
    } :kernel
    
    . = ALIGN(4096);
    .rodata : AT(_kernel_text_lma + (ADDR(.rodata) - _kernel_text_lma)) {
        *(.rodata)
        *(.rodata.*)
    } :kernel
    
    . = ALIGN(4096);
    .data : AT(_kernel_text_lma + (ADDR(.data) - _kernel_text_lma)) {
        *(.data)
        *(.data.*)
        
        . = ALIGN(8);
        _GLOBAL_OFFSET_TABLE_ = .;
        *(.got)
        *(.got.plt)
    } :kernel
    
    . = ALIGN(4096);
    __bss_start = .;
    .bss (NOLOAD) : AT(_kernel_text_lma + (ADDR(.bss) - _kernel_text_lma)) {
        *(.bss)
        *(.bss.*)
        *(COMMON)
    } :kernel
    
    . = ALIGN(4096);
    .lbss (NOLOAD) : AT(_kernel_text_lma + (ADDR(.lbss) - _kernel_text_lma)) {
        *(.lbss)
        *(.lbss.*)
        *(LARGE_COMMON)
    } :kernel
    
    .rela.dyn : {
        *(.rela.init)
        *(.rela.text .rela.text.*)
        *(.rela.rodata .rela.rodata.*)
        *(.rela.data .rela.data.*)
        *(.rela.got)
        *(.rela.bss .rela.bss.*)
        *(.rela.ifunc)
    }
    
    .dynsym : {
        *(.dynsym)
    }
    
    .dynstr : {
        *(.dynstr)
    }
    
    /DISCARD/ : {
        *(.comment)
        *(.eh_frame*)
    }
    
    . = ALIGN(4096);
    _kernel_end = .;
    _kernel_size = _kernel_end - _kernel_text_vma;
    _kernel_end_phys = _kernel_text_lma + _kernel_size;
}
