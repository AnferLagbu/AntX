//! Boot Information Module
//!
//! Parses Multiboot1 and Multiboot2 information to obtain memory map and
//! other boot parameters.
//!
//! # Safety
//! Interior mutability for `BOOT_INFO` is achieved via `spin::Once` (write-once
//! at boot, then read-only). `MULTIBOOT_INFO_PTR` uses `spin::Mutex` since it
//! is set before init and read during init.

#[cfg(target_arch = "aarch64")]
pub mod aarch64;
pub mod multiboot2_fb;

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

struct MultibootPtr(#[allow(dead_code)] *const u8);
// SAFETY: MultibootPtr wraps a raw pointer to boot info data that is
// set once during early boot and read-only afterwards. Access is
// protected by MULTIBOOT_INFO_PTR Mutex.
unsafe impl Send for MultibootPtr {}
unsafe impl Sync for MultibootPtr {}

static BOOT_INFO: spin::Once<BootInfo> = spin::Once::new();
static MULTIBOOT_INFO_PTR: spin::Mutex<MultibootPtr> =
    spin::Mutex::new(MultibootPtr(core::ptr::null()));
static MULTIBOOT_MAGIC: spin::Mutex<u32> = spin::Mutex::new(0);

extern "C" {
    static _kernel_end: u8;
}

pub fn get_boot_info() -> &'static BootInfo {
    BOOT_INFO
        .get()
        .expect("[BOOT] accessed before initialization")
}

#[no_mangle]
pub extern "C" fn boot_set_multiboot_info(magic: u32, ptr: *const u8) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'J', options(nostack));
    }
    *MULTIBOOT_MAGIC.lock() = magic;
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'K', options(nostack));
    }
    *MULTIBOOT_INFO_PTR.lock() = MultibootPtr(ptr);
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'L', options(nostack));
    }
}

#[cfg(target_arch = "x86_64")]
fn parse_multiboot1(ptr: *const u8) -> (u64, usize) {
    let mbi = unsafe { &*(ptr as *const Multiboot1Info) };
    let mut mem_size: u64 = 128 * 1024 * 1024;
    let mut mmap_entries: usize = 0;

    if mbi.flags & MBOOT1_FLAG_MEM != 0 {
        mem_size = (mbi.mem_upper as u64 + 1024) * 1024;
    }

    if mbi.flags & MBOOT1_FLAG_MMAP != 0 {
        let mmap_start = mbi.mmap_addr as *const MemoryMapEntry;
        let mmap_end = (mbi.mmap_addr + mbi.mmap_length) as *const MemoryMapEntry;
        let mut max_addr: u64 = 0;

        let mut current = mmap_start;
        while current < mmap_end {
            let entry = unsafe { &*current };
            let end = entry.base_addr() + entry.length();
            if end > max_addr && entry.is_available() {
                max_addr = end;
            }
            mmap_entries += 1;
            current = unsafe {
                (current as *const u8).add(entry.size as usize + 4) as *const MemoryMapEntry
            };
        }

        if max_addr > 0 {
            mem_size = max_addr;
        }
    }

    (mem_size, mmap_entries)
}

#[cfg(target_arch = "x86_64")]
fn parse_multiboot2(ptr: *const u8) -> (u64, usize) {
    let total_size = unsafe { *(ptr as *const u32) };
    let mut mem_size: u64 = 128 * 1024 * 1024;
    let mut mmap_entries: usize = 0;

    let mut offset: usize = 8;
    let end = total_size as usize;

    while offset + 8 <= end {
        let tag_ptr = unsafe { ptr.add(offset) };
        let tag_type = unsafe { *(tag_ptr as *const u32) };
        let tag_size = unsafe { *((tag_ptr as *const u32).add(1)) };

        if tag_type == 0 || tag_size == 0 {
            break;
        }

        match tag_type {
            4 => {
                let basic_ptr = unsafe { tag_ptr.add(8) };
                let _mem_lower = unsafe { *(basic_ptr as *const u32) };
                let mem_upper = unsafe { *((basic_ptr as *const u32).add(1)) };
                mem_size = (mem_upper as u64 + 1024) * 1024;
            }
            6 => {
                let entry_size = unsafe { *(tag_ptr.add(8) as *const u32) };
                let _entry_version = unsafe { *((tag_ptr.add(8) as *const u32).add(1)) };
                let entries_start = unsafe { tag_ptr.add(16) };
                let entries_end = unsafe { tag_ptr.add(tag_size as usize) };
                let mut max_addr: u64 = 0;

                let mut pos = entries_start;
                while unsafe { pos.add(entry_size as usize) <= entries_end } {
                    let base = unsafe {
                        let lo = *(pos as *const u32);
                        let hi = *((pos as *const u32).add(1));
                        ((hi as u64) << 32) | (lo as u64)
                    };
                    let len = unsafe {
                        let lo = *((pos as *const u32).add(2));
                        let hi = *((pos as *const u32).add(3));
                        ((hi as u64) << 32) | (lo as u64)
                    };
                    let mtype = unsafe { *((pos as *const u32).add(4)) };

                    if mtype == 1 {
                        let end_addr = base + len;
                        if end_addr > max_addr {
                            max_addr = end_addr;
                        }
                    }
                    mmap_entries += 1;
                    pos = unsafe { pos.add(entry_size as usize) };
                }

                if max_addr > 0 {
                    mem_size = max_addr;
                }
            }
            8 => {
                multiboot2_fb::parse_framebuffer_tag(unsafe { tag_ptr.add(8) }, tag_size);
            }
            _ => {}
        }

        offset += tag_size as usize;
        offset = (offset + 7) & !7;
    }

    (mem_size, mmap_entries)
}

pub fn init() -> BootInfo {
    let kernel_end = unsafe { &_kernel_end as *const u8 as u64 };

    #[cfg(target_arch = "x86_64")]
    let (mem_size, mmap_entries) = {
        let magic = *MULTIBOOT_MAGIC.lock();
        let ptr = MULTIBOOT_INFO_PTR.lock().0;

        let mut ms: u64 = 128 * 1024 * 1024;
        let mut me: usize = 0;

        if !ptr.is_null() {
            match magic {
                MULTIBOOT1_MAGIC => {
                    let (m, e) = parse_multiboot1(ptr);
                    ms = m;
                    me = e;
                }
                MULTIBOOT2_MAGIC => {
                    let (m, e) = parse_multiboot2(ptr);
                    ms = m;
                    me = e;
                }
                _ => {}
            }
        }
        (ms, me)
    };

    #[cfg(target_arch = "aarch64")]
    let (mem_size, mmap_entries) = {
        // On QEMU virt machine, default to 512MB.
        // Can be overridden by AARCH64_MEM_SIZE environment/build variable.
        let ms: u64 = option_env!("AARCH64_MEM_MB")
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mb| mb * 1024 * 1024)
            .unwrap_or(512 * 1024 * 1024);
        (ms, 0)
    };

    let info = BootInfo {
        mem_size,
        kernel_end,
        mmap_entries,
    };

    BOOT_INFO.call_once(|| info);

    info
}

#[no_mangle]
pub extern "C" fn boot_get_mem_size() -> u64 {
    get_boot_info().mem_size
}

#[no_mangle]
pub extern "C" fn boot_get_kernel_end() -> u64 {
    get_boot_info().kernel_end
}
