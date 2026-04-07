OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

SECTIONS
{
    . = 0x100000;
    
    .multiboot2 : {
        *(.multiboot2)
    }
    
    .text : {
        *(.text)
        *(.text.*)
    }
    
    .rodata : {
        *(.rodata)
        *(.rodata.*)
    }
    
    .data : {
        *(.data)
        *(.data.*)
        
        . = ALIGN(8);
        _GLOBAL_OFFSET_TABLE_ = .;
        *(.got)
        *(.got.plt)
    }
    
    .bss : {
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
        *(.note.*)
        *(.eh_frame*)
    }
    
    _kernel_start = .;
    _kernel_end = .;
}
