pub const PAGE_SIZE: u64 = 4096;
pub const KERNEL_BASE: u64 = 0xFFFF800000000000u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysAddr(pub u64);

impl PhysAddr {
    pub fn as_u64(&self) -> u64 { self.0 }
    pub fn to_virt(&self) -> VirtAddr {
        VirtAddr(self.0 + KERNEL_BASE)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtAddr(pub u64);

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct PageFlags: u64 {
        const PRESENT = 1;
        const WRITABLE = 2;
        const USER_ACCESSIBLE = 4;
        const COW = 1 << 62;
        const NX = 1 << 63;
    }
}

#[macro_export]
macro_rules! klog_ffi {
    ($fn_name:ident, $($arg:tt)*) => {};
}

#[macro_export]
macro_rules! klog_info {
    ($cat:ident, $($arg:tt)*) => {};
}

#[macro_export]
macro_rules! klog_err {
    ($cat:ident, $($arg:tt)*) => {};
}

#[macro_export]
macro_rules! klog_boot_info {
    ($($arg:tt)*) => {};
}

#[macro_export]
macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

pub fn get_pmm() -> &'static PmmMock {
    static PMM: PmmMock = PmmMock;
    &PMM
}

pub struct PmmMock;

impl PmmMock {
    pub fn alloc_page(&self) -> Option<PhysAddr> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0x100000);
        let addr = NEXT.fetch_add(PAGE_SIZE, Ordering::Relaxed);
        let page = addr as *mut u8;
        unsafe { core::ptr::write_bytes(page, 0, PAGE_SIZE as usize); }
        Some(PhysAddr(addr))
    }

    pub fn alloc_pages(&self, count: usize) -> Option<PhysAddr> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0x200000);
        let addr = NEXT.fetch_add(PAGE_SIZE * count as u64, Ordering::Relaxed);
        let page = addr as *mut u8;
        unsafe { core::ptr::write_bytes(page, 0, (PAGE_SIZE * count as u64) as usize); }
        Some(PhysAddr(addr))
    }

    pub fn free_pages(&self, _phys: PhysAddr, _count: usize) {}

    pub fn free_page(&self, _phys: PhysAddr) {}
}

pub struct VmmMock;

impl VmmMock {
    pub fn get_physical(&self, _virt: VirtAddr) -> Option<PhysAddr> {
        None
    }

    pub fn map_page(&self, _virt: VirtAddr, _phys: PhysAddr, _flags: PageFlags) -> Result<(), ()> {
        Ok(())
    }

    pub fn unmap_page(&self, _virt: VirtAddr) {}

    pub fn flush_tlb(&self, _addr: u64) {}
}

pub fn get_vmm() -> &'static VmmMock {
    static VMM: VmmMock = VmmMock;
    &VMM
}
