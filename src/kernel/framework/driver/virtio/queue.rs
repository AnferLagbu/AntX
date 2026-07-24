//! VirtIO virtqueue 实现 (VirtIO 1.0 split ring).
//!
//! 每个 virtqueue 由三段物理连续的内存区域组成:
//! - 描述符表 (Descriptor Table): 16 字节描述符数组
//! - 可用环 (Available Ring): 驱动→设备的通知环
//! - 已用环 (Used Ring): 设备→驱动的完成环
//!
//! 内存布局遵循 VirtIO 1.0 规范第 2.6 节 "Split Virtqueues".

use crate::kernel::framework::mm::{KERNEL_BASE, PAGE_SIZE};

/// virtqueue 项最大数量 (必须为 2 的幂).
pub const VQ_SIZE: u16 = 32;

/// Split virtqueue 描述符.
#[repr(C)]
pub struct VqDesc {
    pub addr: u64,  // 缓冲区客户机物理地址
    pub len: u32,   // 缓冲区长度
    pub flags: u16, // VRING_DESC_F_*
    pub next: u16,  // 下一描述符索引 (链式描述符)
}

/// 可用环头 + 环数组.
#[repr(C)]
pub struct VqAvail {
    pub flags: u16, // VRING_AVAIL_F_NO_INTERRUPT
    pub idx: u16,   // 下一个可用环索引 (驱动递增)
    pub ring: [u16; VQ_SIZE as usize],
    // 当协商 VIRTIO_F_EVENT_IDX 时 used_event 紧跟 ring 之后
}

/// 已用环项.
#[repr(C)]
pub struct VqUsedElem {
    pub id: u32,  // 描述符链头索引
    pub len: u32, // 设备写入的总字节数
}

/// 已用环头 + 环数组.
#[repr(C)]
pub struct VqUsed {
    pub flags: u16, // VRING_USED_F_NO_NOTIFY
    pub idx: u16,   // 下一个已用环索引 (设备递增)
    pub ring: [VqUsedElem; VQ_SIZE as usize],
    // 当协商 VIRTIO_F_EVENT_IDX 时 avail_event 紧跟 ring 之后
}

// ── 描述符标志 ──

pub const VQ_DESC_F_NEXT: u16 = 1; // 链接通过 this.next 继续
pub const VQ_DESC_F_WRITE: u16 = 2; // 设备可写入此缓冲区
pub const VQ_DESC_F_INDIRECT: u16 = 4; // 间接描述符表

// ── 可用环标志 ──

pub const VQ_AVAIL_F_NO_INTERRUPT: u16 = 1;

// ── 已用环标志 ──

pub const VQ_USED_F_NO_NOTIFY: u16 = 1;

/// 单个 virtqueue.
pub struct VirtQueue {
    pub desc: *mut VqDesc,
    pub avail: *mut VqAvail,
    pub used: *mut VqUsed,
    pub queue_size: u16,
    pub free_head: u16,      // 空闲描述符链头
    pub last_used_idx: u16,  // 最近见到的 used ring 索引
    pub next_avail_idx: u16, // 驱动下次使用的 avail ring 索引
    // --- 所有权追踪 (用于 DMA 安全性) ---
    /// 已分配页的物理地址 (用于 x86_64 上 phys↔virt 转换).
    pub desc_phys: u64,
    pub avail_phys: u64,
    pub used_phys: u64,
}

// SAFETY: VirtQueue 裸指针指向 PMM 分配的 DMA 页.
// 每个设备实例拥有自己的 virtqueue. 描述符操作使用 volatile 访问与内存屏障.
// SAFETY: VirtQueue 含 MMIO 裸指针, 跨 CPU 安全性由单一所有者 &mut self 访问或外部锁保证.
unsafe impl Send for VirtQueue {}
// SAFETY: 同上, &mut self 或外部锁保证并发安全.
unsafe impl Sync for VirtQueue {}

impl VirtQueue {
    /// 分配并初始化一个 split virtqueue.
    ///
    /// 当 `legacy` 为 true 时, used ring 对齐到 4096 字节边界
    /// (QEMU 旧版 VirtIO 传输所要求 — VIRTIO_PCI_VRING_ALIGN).
    /// 此时需要 2 个页而不是 1 个.
    pub fn new(legacy: bool) -> Option<Self> {
        let desc_size = VQ_SIZE as usize * core::mem::size_of::<VqDesc>();
        let avail_size = core::mem::size_of::<VqAvail>() + 2 /* event index padding */;
        let used_size  = core::mem::size_of::<VqUsed>() + 2 /* event index padding */;

        // 计算偏移: desc | avail (4 字节对齐) | used
        let desc_off = 0usize;
        let avail_off = desc_off + desc_size;
        let used_off = if legacy {
            PAGE_SIZE as usize // 旧版: used ring 必须页对齐 (QEMU 使用 VIRTIO_PCI_VRING_ALIGN)
        } else {
            align_up(avail_off + avail_size, 4)
        };
        let total_size = align_up(used_off + used_size, PAGE_SIZE as usize);

        let pages = total_size.div_ceil(PAGE_SIZE as usize);
        unsafe extern "C" {
            fn pmm_alloc_pages(count: u64) -> *mut u8;
        }
        // SAFETY: extern 函数的参数/返回值类型与 C ABI 声明一致; 调用方保证指针有效
        let mem = unsafe { pmm_alloc_pages(pages as u64) };
        if mem.is_null() {
            return None;
        }

        let mem_phys = mem as u64;
        let mem_virt = (mem_phys + KERNEL_BASE) as *mut u8;

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::write_bytes(mem_virt, 0, total_size);

            let desc_ptr = mem_virt as *mut VqDesc;
            let avail_ptr = mem_virt.add(avail_off as usize) as *mut VqAvail;
            let used_ptr = mem_virt.add(used_off as usize) as *mut VqUsed;

            for i in 0..VQ_SIZE {
                let desc = &mut *desc_ptr.add(i as usize);
                desc.flags = 0;
                desc.next = if i + 1 < VQ_SIZE { i + 1 } else { 0xFFFF };
            }

            // 清零 avail 与 used ring (关键: 设备将读取这些)
            core::ptr::write_bytes(avail_ptr as *mut u8, 0, avail_size);
            core::ptr::write_bytes(used_ptr as *mut u8, 0, used_size);

            Some(VirtQueue {
                desc: desc_ptr,
                avail: avail_ptr,
                used: used_ptr,
                queue_size: VQ_SIZE,
                free_head: 0,
                last_used_idx: 0,
                next_avail_idx: 0,
                desc_phys: mem_phys + desc_off as u64,
                avail_phys: mem_phys + avail_off as u64,
                used_phys: mem_phys + used_off as u64,
            })
        }
    }

    /// 获取描述符表的物理地址.
    pub fn desc_paddr(&self) -> u64 {
        self.desc_phys
    }
    /// 获取 available ring 的物理地址.
    pub fn avail_paddr(&self) -> u64 {
        self.avail_phys
    }
    /// 获取 used ring 的物理地址.
    pub fn used_paddr(&self) -> u64 {
        self.used_phys
    }

    /// 准备一个描述符链并返回头索引.
    /// 若无空闲描述符返回 `0xFFFF`.
    /// 对于简单读写操作, 这将创建单描述符链.
    pub fn prepare_desc(&mut self, buf_paddr: u64, buf_len: u32, write: bool) -> u16 {
        if self.free_head == 0xFFFF {
            return 0xFFFF;
        }
        let head = self.free_head;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let desc = &mut *self.desc.add(head as usize);
            let next_free = desc.next; // 覆盖前先保存
            desc.addr = buf_paddr;
            desc.len = buf_len;
            desc.flags = if write { VQ_DESC_F_WRITE } else { 0 };
            desc.next = 0;
            // free_head 推进到下一个空闲描述符
            self.free_head = next_free;
        }
        head
    }

    /// 提交描述符链到设备 (通知设备).
    /// 返回已提交的可用环索引.
    pub fn submit(&mut self, desc_head: u16) -> u16 {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
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

    /// 提交后通知设备 (调用方必须设置 avail->idx 并写 QueueNotify).
    pub fn commit_and_kick(&mut self) {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            // 全内存屏障: 确保描述符与 ring 写入全局可见
            crate::kernel::framework::sync::arch::fence();
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            core::ptr::write_volatile(&mut (*self.avail).idx, self.next_avail_idx);
        }
    }

    /// 检查是否有已用描述符可用, 有则返回.
    pub fn pop_used(&mut self) -> Option<(u16, u32)> {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let used_idx = core::ptr::read_volatile(&(*self.used).idx);
            if self.last_used_idx == used_idx {
                return None;
            }
            let elem = &(*self.used).ring[self.last_used_idx as usize % VQ_SIZE as usize];
            let id = elem.id as u16;
            let len = elem.len;
            self.last_used_idx = self.last_used_idx.wrapping_add(1);
            Some((id, len))
        }
    }

    /// 完成后将描述符归还到空闲链.
    pub fn reclaim_desc(&mut self, head: u16) {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let desc = &mut *self.desc.add(head as usize);
            desc.next = self.free_head;
            self.free_head = head;
        }
    }

    /// 将描述符 head 链接到 next 描述符 (设置 NEXT 标志和 next 指针).
    ///
    /// 用于构建多描述符链 (如 virtio-blk 的 header→data→status).
    pub fn link_desc(&mut self, head: u16, next: u16) {
        // SAFETY: head/next 均为有效的描述符索引 (由 prepare_desc 返回),
        // desc 指针指向 PMM 分配的 DMA 页, 生命周期由 VirtQueue 管理.
        unsafe {
            let desc = &mut *self.desc.add(head as usize);
            desc.flags |= VQ_DESC_F_NEXT;
            desc.next = next;
        }
    }
}

/// 将 `val` 向上对齐到 `align` 的下一个倍数.
fn align_up(val: usize, align: usize) -> usize {
    (val + align - 1) & !(align - 1)
}
