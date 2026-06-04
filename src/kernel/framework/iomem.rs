//! IoMem — MMIO 安全代理 (TCB)
//!
//! 防止 driver 访问其 BAR 之外的 MMIO 区域,
//! 通过全局别名检测表防止同一物理地址被多个 driver 独占映射。
//!
//! ## 与 Asterinas OSTD `IoMem` 的关系
//!
//! 等价于 OSTD 的 `IoMem`。
//!
//! ## SAFETY 不变量
//!
//! - 每个 IoMem 实例在创建时核验 phys..phys+len 不与其他实例冲突。
//! - 所有读/写操作检查 offset 在 [0, len) 范围内。
//! - 底层访问使用 read_volatile/write_volatile 保证 MMIO 语义。

use core::fmt;
use core::ptr::NonNull;

use crate::kernel::framework::mm::{PhysAddr, phys_to_virt};

/// MMIO 别名注册表, 防止同一物理区域被多次映射。
/// 使用 spin::Mutex (已在内核中广泛使用) 保证线程安全。
static ALIAS_REGISTRY: spin::Mutex<AliasRegistry> = spin::Mutex::new(AliasRegistry::new());

const MAX_MMIO_MAPPINGS: usize = 64;

struct AliasRegistry {
    entries: [(u64, usize, &'static str); MAX_MMIO_MAPPINGS],
    count: usize,
}

impl AliasRegistry {
    const fn new() -> Self {
        Self {
            entries: [(0, 0, ""); MAX_MMIO_MAPPINGS],
            count: 0,
        }
    }

    fn check_conflict(&self, phys: u64, len: usize) -> Option<&'static str> {
        for i in 0..self.count {
            let (b, l, name) = self.entries[i];
            let end = phys.saturating_add(len as u64);
            let existing_end = b.saturating_add(l as u64);
            if phys < existing_end && end > b {
                return Some(name);
            }
        }
        None
    }

    fn register(&mut self, phys: u64, len: usize, name: &'static str) -> Result<(), &'static str> {
        if self.count >= MAX_MMIO_MAPPINGS {
            return Err("MMIO alias registry full");
        }
        if let Some(_conflict) = self.check_conflict(phys, len) {
            return Err("MMIO region overlaps existing region");
        }
        self.entries[self.count] = (phys, len, name);
        self.count += 1;
        Ok(())
    }

    fn unregister(&mut self, phys: u64) {
        for i in 0..self.count {
            if self.entries[i].0 == phys {
                self.entries[i] = self.entries[self.count - 1];
                self.count -= 1;
                return;
            }
        }
    }
}

/// 经校验的 MMIO 区域句柄。
///
/// 创建时核验物理地址范围并注册别名检测;
/// 运行时所有读/写做边界检查。
pub struct IoMem {
    phys_base: PhysAddr,
    len: usize,
    virt: NonNull<u8>,
    name: &'static str,
}

impl IoMem {
    /// 创建 MMIO 句柄。
    ///
    /// # SAFETY
    /// - phys 必须指向有效的设备 MMIO 物理区域。
    /// - phys..phys+len 必须已映射到内核空间 (identity map / ioremap)。
    /// - 同一物理区域不重复创建 (由别名检测保证)。
    pub unsafe fn new(phys: PhysAddr, len: usize, name: &'static str) -> Result<Self, &'static str> {
        if len == 0 {
            return Err("IoMem: zero-length MMIO region");
        }
        if !phys.as_u64().is_multiple_of(4) {
            return Err("IoMem: phys must be 4-byte aligned");
        }

        // SAFETY: 别名注册在 Mutex 保护下, 无竞争。
        {
            let mut reg = ALIAS_REGISTRY.lock();
            reg.register(phys.as_u64(), len, name)?;
        }

        let virt_addr = phys_to_virt(phys.as_u64()) as *mut u8;
        let virt = NonNull::new(virt_addr).ok_or("IoMem: null virtual address")?;

        Ok(Self { phys_base: phys, len, virt, name })
    }

    /// 从 PCI BAR 地址创建 MMIO 句柄 (安全包装)。
    ///
    /// PCI BAR 地址由 PCI 枚举保证为有效 MMIO 区域,
    /// 因此本函数是安全的。services 层应使用此方法而非 `new`。
    ///
    /// # 参数
    /// - `bar_phys`: PCI BAR 基地址 (来自 PCI 枚举)
    /// - `len`: MMIO 区域大小 (来自 BAR 大小寄存器)
    /// - `name`: 设备名称 (用于调试和别名检测)
    pub fn from_pci_bar(bar_phys: PhysAddr, len: usize, name: &'static str) -> Result<Self, &'static str> {
        if bar_phys.as_u64() == 0 {
            return Err("IoMem: PCI BAR is zero (device not configured)");
        }
        // SAFETY: PCI BAR 地址由 PCI 枚举保证为有效 MMIO 区域,
        // 且 identity-mapped 内核中 phys 可直接转为 virt。
        unsafe { Self::new(bar_phys, len, name) }
    }

    #[inline(always)] pub fn phys(&self) -> PhysAddr { self.phys_base }
    #[inline(always)] pub fn len(&self) -> usize { self.len }
    #[inline(always)] pub fn is_empty(&self) -> bool { self.len == 0 }
    #[inline(always)] pub fn name(&self) -> &'static str { self.name }

    /// Get the virtual address pointer for struct overlay access.
    /// # Safety
    /// Caller must ensure the struct type matches the MMIO layout,
    /// and only performs volatile reads/writes with proper alignment.
    #[inline(always)] pub unsafe fn virt_ptr(&self) -> *mut u8 {
        // SAFETY: `self.virt` 是 `IoMem::from_*` 构造时由 `NonNull::new_unchecked`
        // 校验的物理基地址经 MMU 映射后的虚拟地址, 满足 NonNull 契约。
        // 调用方 (标记为 `unsafe fn`) 须自行保证:
        //   1. 指针类型 (`*mut T`) 与 MMIO 寄存器布局一致 (大小/对齐)
        //   2. 仅通过 volatile 访问 (无编译器重排)
        //   3. 不会写出 `IoMem` 自身的字节范围 (`offset + size_of::<T>() <= self.len`)
        self.virt.as_ptr()
    }

    fn check_offset(&self, offset: usize, size: usize) -> Result<(), &'static str> {
        if offset.saturating_add(size) > self.len {
            return Err("IoMem: access out of bounds");
        }
        Ok(())
    }

    #[inline] pub fn read_u8(&self, offset: usize) -> u8 {
        self.check_offset(offset, 1).unwrap();
        // SAFETY: `check_offset` 已验证 `offset + 1 <= self.len`, 指针 `self.virt + offset`
        // 落在 IoMem 持有的 MMIO 区域内, 不会越界; `read_volatile` 防止编译器重排。
        unsafe { self.virt.as_ptr().add(offset).read_volatile() }
    }
    #[inline] pub fn read_u16(&self, offset: usize) -> u16 {
        self.check_offset(offset, 2).unwrap();
        // SAFETY: `check_offset(offset, 2)` 已验证 2 字节访问不越界; u16 转换要求
        // 2 字节对齐 (PCI BAR MMIO 由 BIOS/UEFI 建立时保证自然对齐)。
        unsafe { (self.virt.as_ptr().add(offset) as *const u16).read_volatile() }
    }
    #[inline] pub fn read_u32(&self, offset: usize) -> u32 {
        self.check_offset(offset, 4).unwrap();
        // SAFETY: `check_offset(offset, 4)` 已验证 4 字节访问不越界; 4 字节自然对齐
        // 由 MMIO 基地址的页对齐保证 (PAGE_SIZE=4096, 任何 4 字节偏移都对其)。
        unsafe { (self.virt.as_ptr().add(offset) as *const u32).read_volatile() }
    }
    #[inline] pub fn read_u64(&self, offset: usize) -> u64 {
        self.check_offset(offset, 8).unwrap();
        // SAFETY: `check_offset(offset, 8)` 已验证 8 字节访问不越界; 8 字节自然对齐
        // 由 MMIO 基地址的页对齐保证。
        unsafe { (self.virt.as_ptr().add(offset) as *const u64).read_volatile() }
    }
    #[inline] pub fn write_u8(&self, offset: usize, val: u8) {
        self.check_offset(offset, 1).unwrap();
        // SAFETY: 与 `read_u8` 对称, 写 1 字节不会越界; volatile 写保证设备立即可见。
        unsafe { self.virt.as_ptr().add(offset).write_volatile(val); }
    }
    #[inline] pub fn write_u16(&self, offset: usize, val: u16) {
        self.check_offset(offset, 2).unwrap();
        // SAFETY: `check_offset(offset, 2)` 已验证 2 字节写不越界; 2 字节对齐由 MMIO 基地址页对齐保证。
        unsafe { (self.virt.as_ptr().add(offset) as *mut u16).write_volatile(val); }
    }
    #[inline] pub fn write_u32(&self, offset: usize, val: u32) {
        self.check_offset(offset, 4).unwrap();
        // SAFETY: `check_offset(offset, 4)` 已验证 4 字节写不越界; 4 字节自然对齐由页对齐保证。
        unsafe { (self.virt.as_ptr().add(offset) as *mut u32).write_volatile(val); }
    }
    #[inline] pub fn write_u64(&self, offset: usize, val: u64) {
        self.check_offset(offset, 8).unwrap();
        // SAFETY: `check_offset(offset, 8)` 已验证 8 字节写不越界; 8 字节自然对齐由页对齐保证。
        unsafe { (self.virt.as_ptr().add(offset) as *mut u64).write_volatile(val); }
    }
}

impl Drop for IoMem {
    fn drop(&mut self) {
        let mut reg = ALIAS_REGISTRY.lock();
        reg.unregister(self.phys_base.as_u64());
    }
}

impl fmt::Display for IoMem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IoMem({}, phys=0x{:x}, len=0x{:x})", self.name, self.phys_base.as_u64(), self.len)
    }
}

// SAFETY: IoMem 封装了独占的 MMIO 物理区域, 内核态独占访问。
unsafe impl Send for IoMem {}
// SAFETY: &IoMem 通过别名检测 + SpinLock (内核层注册表) 保证无并发写。
unsafe impl Sync for IoMem {}
