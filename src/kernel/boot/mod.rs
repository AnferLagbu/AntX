//! Boot Information Module
//!
//! Parses Multiboot information to obtain memory map and other boot parameters.

use core::mem::size_of;

pub const MULTIBOOT1_MAGIC: u32 = 0x2BADB002;
pub const MULTIBOOT2_MAGIC: u32 = 0x36D76289;

pub const MBOOT1_FLAG_MEM: u32 = 1 << 0;
pub const MBOOT1_FLAG_MMAP: u32 = 1 << 6;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Multiboot1Info {
    pub flags: u32,
    pub mem_lower: u32,
    pub mem_upper: u32,
    pub boot_device: u32,
    pub cmdline: u32,
    pub mods_count: u32,
    pub mods_addr: u32,
    pub syms: [u32; 4],
    pub mmap_length: u32,
    pub mmap_addr: u32,
    pub drives_length: u32,
    pub drives_addr: u32,
    pub config_table: u32,
    pub boot_loader_name: u32,
    pub apm_table: u32,
    pub vbe_control_info: u32,
    pub vbe_mode_info: u32,
    pub vbe_mode: u16,
    pub vbe_interface_seg: u16,
    pub vbe_interface_off: u16,
    pub vbe_interface_len: u16,
    pub vbe_control_info_high: u64,
    pub vbe_mode_info_high: u64,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MemoryMapEntry {
    pub size: u32,
    pub base_addr_low: u32,
    pub base_addr_high: u32,
    pub length_low: u32,
    pub length_high: u32,
    pub mtype: u32,
}

impl MemoryMapEntry {
    pub fn base_addr(&self) -> u64 {
        (self.base_addr_high as u64) << 32 | (self.base_addr_low as u64)
    }

    pub fn length(&self) -> u64 {
        (self.length_high as u64) << 32 | (self.length_low as u64)
    }

    pub fn is_available(&self) -> bool {
        self.mtype == 1
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BootInfo {
    pub mem_size: u64,
    pub kernel_end: u64,
    pub mmap_entries: usize,
}

impl BootInfo {
    pub const fn new() -> Self {
        Self {
            mem_size: 0,
            kernel_end: 0,
            mmap_entries: 0,
        }
    }
}

static mut BOOT_INFO: BootInfo = BootInfo::new();
static mut MULTIBOOT_INFO_PTR: *const Multiboot1Info = core::ptr::null();

extern "C" {
    static _kernel_end: u8;
}

pub fn get_boot_info() -> &'static BootInfo {
    unsafe { &BOOT_INFO }
}

#[no_mangle]
pub extern "C" fn boot_set_multiboot_info(ptr: *const Multiboot1Info) {
    unsafe {
        MULTIBOOT_INFO_PTR = ptr;
    }
}

pub fn init() -> BootInfo {
    unsafe {
        let kernel_end = &_kernel_end as *const u8 as u64;
        
        let mut mem_size: u64 = 128 * 1024 * 1024;
        
        if !MULTIBOOT_INFO_PTR.is_null() {
            let mbi = &*MULTIBOOT_INFO_PTR;
            
            if mbi.flags & MBOOT1_FLAG_MEM != 0 {
                mem_size = (mbi.mem_upper as u64 + 1024) * 1024;
            }
            
            if mbi.flags & MBOOT1_FLAG_MMAP != 0 {
                let mmap_start = mbi.mmap_addr as *const MemoryMapEntry;
                let mmap_end = (mbi.mmap_addr + mbi.mmap_length) as *const MemoryMapEntry;
                let mut max_addr: u64 = 0;
                let mut entry_count = 0usize;
                
                let mut current = mmap_start;
                while current < mmap_end {
                    let entry = &*current;
                    let end = entry.base_addr() + entry.length();
                    if end > max_addr && entry.is_available() {
                        max_addr = end;
                    }
                    entry_count += 1;
                    current = (current as *const u8).add(entry.size as usize + 4) as *const MemoryMapEntry;
                }
                
                if max_addr > 0 {
                    mem_size = max_addr;
                }
                
                BOOT_INFO.mmap_entries = entry_count;
            }
        }
        
        BOOT_INFO.mem_size = mem_size;
        BOOT_INFO.kernel_end = kernel_end;
        
        BOOT_INFO
    }
}

#[no_mangle]
pub extern "C" fn boot_get_mem_size() -> u64 {
    unsafe { BOOT_INFO.mem_size }
}

#[no_mangle]
pub extern "C" fn boot_get_kernel_end() -> u64 {
    unsafe { BOOT_INFO.kernel_end }
}
