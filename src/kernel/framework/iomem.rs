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

use crate::kernel::mm::{PhysAddr, phys_to_virt};

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
        if phys.as_u64() % 4 != 0 {
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
    #[inline(always)] pub fn name(&self) -> &'static str { self.name }

    /// Get the virtual address pointer for struct overlay access.
    /// # Safety
    /// Caller must ensure the struct type matches the MMIO layout,
    /// and only performs volatile reads/writes with proper alignment.
    #[inline(always)] pub unsafe fn virt_ptr(&self) -> *mut u8 {
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
        unsafe { self.virt.as_ptr().add(offset).read_volatile() }
    }
    #[inline] pub fn read_u16(&self, offset: usize) -> u16 {
        self.check_offset(offset, 2).unwrap();
        unsafe { (self.virt.as_ptr().add(offset) as *const u16).read_volatile() }
    }
    #[inline] pub fn read_u32(&self, offset: usize) -> u32 {
        self.check_offset(offset, 4).unwrap();
        unsafe { (self.virt.as_ptr().add(offset) as *const u32).read_volatile() }
    }
    #[inline] pub fn read_u64(&self, offset: usize) -> u64 {
        self.check_offset(offset, 8).unwrap();
        unsafe { (self.virt.as_ptr().add(offset) as *const u64).read_volatile() }
    }
    #[inline] pub fn write_u8(&self, offset: usize, val: u8) {
        self.check_offset(offset, 1).unwrap();
        unsafe { self.virt.as_ptr().add(offset).write_volatile(val); }
    }
    #[inline] pub fn write_u16(&self, offset: usize, val: u16) {
        self.check_offset(offset, 2).unwrap();
        unsafe { (self.virt.as_ptr().add(offset) as *mut u16).write_volatile(val); }
    }
    #[inline] pub fn write_u32(&self, offset: usize, val: u32) {
        self.check_offset(offset, 4).unwrap();
        unsafe { (self.virt.as_ptr().add(offset) as *mut u32).write_volatile(val); }
    }
    #[inline] pub fn write_u64(&self, offset: usize, val: u64) {
        self.check_offset(offset, 8).unwrap();
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

unsafe impl Send for IoMem {}
unsafe impl Sync for IoMem {}
