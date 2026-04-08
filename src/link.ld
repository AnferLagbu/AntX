OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

SECTIONS
{
    . = 0x100000;
    
    _kernel_start = .;
    
    .multiboot2 : {
        *(.multiboot2)
    }
    
    .text : ALIGN(4096) {
        *(.text)
        *(.text.*)
    }
    
    .rodata : ALIGN(4096) {
        *(.rodata)
        *(.rodata.*)
    }
    
    .data : ALIGN(4096) {
        *(.data)
        *(.data.*)
        
        . = ALIGN(8);
        _GLOBAL_OFFSET_TABLE_ = .;
        *(.got)
        *(.got.plt)
    }
    
    .bss : ALIGN(4096) {
        *(.bss)
        *(.bss.*)
        *(COMMON)
    }
    
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
}
