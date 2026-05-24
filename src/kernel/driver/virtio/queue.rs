//! VirtIO virtqueue implementation (VirtIO 1.0 split ring).
//!
//! Each virtqueue consists of three physically-contiguous memory regions:
//! - Descriptor Table: array of 16-byte descriptors
//! - Available Ring: driver→device notification ring
//! - Used Ring: device→driver completion ring
//!
//! Memory layout follows the VirtIO 1.0 specification section 2.6 "Split Virtqueues".

use crate::kernel::mm::KERNEL_BASE;

/// Maximum number of virtqueue entries (must be a power of 2).
pub const VQ_SIZE: u16 = 32;

/// Split virtqueue descriptor.
#[repr(C)]
pub struct VqDesc {
    pub addr:  u64,   // Guest-physical address of buffer
    pub len:   u32,   // Length of buffer
    pub flags: u16,   // VRING_DESC_F_*
    pub next:  u16,   // Index of next descriptor (for chained descriptors)
}

/// Available ring header + ring array.
#[repr(C)]
pub struct VqAvail {
    pub flags:     u16,  // VRING_AVAIL_F_NO_INTERRUPT
    pub idx:       u16,  // Next available ring index (driver increments)
    pub ring:      [u16; VQ_SIZE as usize],
    // used_event follows ring if VIRTIO_F_EVENT_IDX is negotiated
}

/// Used ring header + ring array.
#[repr(C)]
pub struct VqUsedElem {
    pub id:   u32,  // Descriptor chain head index
    pub len:  u32,  // Total bytes written by device
}

#[repr(C)]
pub struct VqUsed {
    pub flags:     u16,  // VRING_USED_F_NO_NOTIFY
    pub idx:       u16,  // Next used ring index (device increments)
    pub ring:      [VqUsedElem; VQ_SIZE as usize],
    // avail_event follows ring if VIRTIO_F_EVENT_IDX is negotiated
}

// ── Descriptor flags ──

pub const VQ_DESC_F_NEXT:    u16 = 1;  // Chain continues with this.next
pub const VQ_DESC_F_WRITE:   u16 = 2;  // Device writes to this buffer
pub const VQ_DESC_F_INDIRECT: u16 = 4;  // Indirect descriptor table

// ── Available ring flags ──

pub const VQ_AVAIL_F_NO_INTERRUPT: u16 = 1;

// ── Used ring flags ──

pub const VQ_USED_F_NO_NOTIFY: u16 = 1;

/// A single virtqueue.
pub struct VirtQueue {
    pub desc:   *mut VqDesc,
    pub avail:  *mut VqAvail,
    pub used:   *mut VqUsed,
    pub queue_size: u16,
    pub free_head:  u16,       // Head of free descriptor chain
    pub last_used_idx: u16,    // Last seen used ring index
    pub next_avail_idx: u16,   // Next index for driver to use in avail ring
    // --- Ownership tracking (for DMA safety) ---
    /// Physical addresses of allocated pages (for phys-virt conversion on x86_64).
    pub desc_phys:  u64,
    pub avail_phys: u64,
    pub used_phys:  u64,
}

// SAFETY: VirtQueue is only accessed from a single CPU (no SMP yet).
// Raw pointers point to PMM-allocated DMA memory.
unsafe impl Send for VirtQueue {}
unsafe impl Sync for VirtQueue {}

impl VirtQueue {
    /// Allocate and initialize a split virtqueue.
    ///
    /// When `legacy` is true, the used ring is aligned to a 4096-byte boundary
    /// (as required by QEMU's legacy VirtIO transport — VIRTIO_PCI_VRING_ALIGN).
    /// This requires 2 pages instead of 1.
    pub fn new(legacy: bool) -> Option<Self> {
        let desc_size  = VQ_SIZE as usize * core::mem::size_of::<VqDesc>();
        let avail_size = core::mem::size_of::<VqAvail>() + 2 /* event index padding */;
        let used_size  = core::mem::size_of::<VqUsed>() + 2 /* event index padding */;

        // Compute offsets: desc | avail (4-aligned) | used
        let desc_off  = 0usize;
        let avail_off = desc_off + desc_size;
        let used_off  = if legacy {
            4096  // Legacy: used ring must be page-aligned (QEMU uses VIRTIO_PCI_VRING_ALIGN)
        } else {
            align_up(avail_off + avail_size, 4)
        };
        let total_size = align_up(used_off + used_size, 4096);

        let pages = (total_size + 4095) / 4096;
        extern "C" { fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void; }
        let mem = unsafe { pmm_alloc_pages(pages as u64) };
        if mem.is_null() { return None; }

        let mem_phys = mem as u64;
        let mem_virt = (mem_phys + KERNEL_BASE) as *mut u8;

        unsafe {
            core::ptr::write_bytes(mem_virt, 0, total_size);

            let desc_ptr  = mem_virt as *mut VqDesc;
            let avail_ptr = mem_virt.add(avail_off as usize) as *mut VqAvail;
            let used_ptr  = mem_virt.add(used_off as usize) as *mut VqUsed;

            for i in 0..VQ_SIZE {
                let desc = &mut *desc_ptr.add(i as usize);
                desc.flags = 0;
                desc.next = if i + 1 < VQ_SIZE { i + 1 } else { 0 };
            }

            // Zero avail and used rings (critical: device sees these)
            core::ptr::write_bytes(avail_ptr as *mut u8, 0, avail_size);
            core::ptr::write_bytes(used_ptr as *mut u8, 0, used_size);

            Some(VirtQueue {
                desc:         desc_ptr,
                avail:        avail_ptr,
                used:         used_ptr,
                queue_size:   VQ_SIZE,
                free_head:    0,
                last_used_idx: 0,
                next_avail_idx: 0,
                desc_phys:    mem_phys + desc_off as u64,
                avail_phys:   mem_phys + avail_off as u64,
                used_phys:    mem_phys + used_off as u64,
            })
        }
    }

    /// Get the physical address of the descriptor table.
    pub fn desc_paddr(&self) -> u64 { self.desc_phys }
    /// Get the physical address of the available ring.
    pub fn avail_paddr(&self) -> u64 { self.avail_phys }
    /// Get the physical address of the used ring.
    pub fn used_paddr(&self) -> u64 { self.used_phys }

    /// Prepare a descriptor chain and return the head index.
    /// For a simple read/write operation, this creates a single-descriptor chain.
    pub fn prepare_desc(&mut self, buf_paddr: u64, buf_len: u32, write: bool) -> u16 {
        let head = self.free_head;
        unsafe {
            let desc = &mut *self.desc.add(head as usize);
            let next_free = desc.next;  // Save before overwriting
            desc.addr = buf_paddr;
            desc.len  = buf_len;
            desc.flags = if write { VQ_DESC_F_WRITE } else { 0 };
            desc.next = 0;
            // Move free_head to next free descriptor
            self.free_head = next_free;
        }
        head
    }

    /// Submit a descriptor chain to the device (kicks the device).
    /// Returns the available ring index that was submitted.
    pub fn submit(&mut self, desc_head: u16) -> u16 {
        unsafe {
            core::ptr::write_volatile(
                &mut (*self.avail).ring[self.next_avail_idx as usize % VQ_SIZE as usize],
                desc_head,
            );
        }
        let idx = self.next_avail_idx;
        self.next_avail_idx = self.next_avail_idx.wrapping_add(1);
        idx
    }

    /// Notify device after submission (caller must set avail->idx and write QueueNotify).
    pub fn commit_and_kick(&mut self) {
        unsafe {
            // Full memory barrier: ensure descriptor and ring writes are globally visible
            crate::kernel::sync::arch::fence();
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            core::ptr::write_volatile(&mut (*self.avail).idx, self.next_avail_idx);
        }
    }

    /// Check if any used descriptors are available and return them.
    pub fn pop_used(&mut self) -> Option<(u16, u32)> {
        unsafe {
            let used_idx = core::ptr::read_volatile(&(*self.used).idx);
            if self.last_used_idx == used_idx {
                return None;
            }
            let elem = &(*self.used).ring[self.last_used_idx as usize % VQ_SIZE as usize];
            let id  = elem.id as u16;
            let len = elem.len;
            self.last_used_idx = self.last_used_idx.wrapping_add(1);
            Some((id, len))
        }
    }

    /// Return a descriptor to the free list after completion.
    pub fn reclaim_desc(&mut self, head: u16) {
        unsafe {
            let desc = &mut *self.desc.add(head as usize);
            desc.next = self.free_head;
            self.free_head = head;
        }
    }
}

/// Align `val` up to the next multiple of `align`.
fn align_up(val: usize, align: usize) -> usize {
    (val + align - 1) & !(align - 1)
}