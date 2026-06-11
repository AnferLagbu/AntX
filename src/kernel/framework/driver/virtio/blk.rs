#![allow(dead_code)]
//! VirtIO Block Device Driver
//!
//! Implements the VirtIO Block Device specification (device ID 2).
//! Uses the split virtqueue for I/O submission and completion.
//!
//! Reads/writes a single 512-byte sector per request.
//! For performance, this can be extended to use multi-sector requests
//! and multiple outstanding I/Os.

use super::queue::VirtQueue;
use super::{VirtioMmioDevice, VIRTIO_ID_BLOCK};
use crate::kernel::framework::driver::block::BlockDevice;
use crate::kernel::framework::mm::KERNEL_BASE;
use crate::klog_info;
use crate::klog_warn;

#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::idt::types::InterruptFrame;

// I-42: virtio-blk 默认 IRQ 号 (QEMU virt 机器分配, 启动探测时由设备配置覆盖).
// 暴露为 pub const 让 boot/PCI 探测代码能改写.
pub const DEFAULT_VIRTIO_BLK_IRQ: u8 = 11;

// I-42: 全局 ISR → device 完成事件绑定指针.
// 单实例场景下, 一个静态指针足够; 多实例时应替换为 IRQ 索引到设备的查表.
#[cfg(target_arch = "x86_64")]
static mut VIRTIO_BLK_COMPLETION_PTR: *const IoCompletion = core::ptr::null();

// I-42: 轻量级 I/O 完成事件, 替代原 do_io 内的 `loop { pop_used(); spin_loop() }` 忙等.
// 由 ISR (`virtio_blk_irq_handler`) 在设备通知 used ring 有新条目时 signal,
// do_io 在等待时只 spin_loop 检查此 flag, 避免无限空转.
// 当前 single-owner 同步语义下, 每设备一个事件足够; 后续多 outstanding I/O 时
// 应改为按 request token 索引的 event 数组, 但本 fix 不阻塞 (do_io 仍串行).
use core::sync::atomic::{AtomicBool, Ordering};

/// 单次 I/O 完成事件
pub struct IoCompletion {
    done: AtomicBool,
}

impl IoCompletion {
    pub const fn new() -> Self {
        Self { done: AtomicBool::new(false) }
    }
    /// ISR 路径: 标记 I/O 完成
    pub fn signal(&self) {
        self.done.store(true, Ordering::Release);
    }
    /// 等待者: 是否有完成
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }
    /// 提交新一轮 I/O 前重置
    pub fn reset(&self) {
        self.done.store(false, Ordering::Release);
    }
}

/// VirtIO block request header (little-endian).
#[repr(C)]
#[derive(Debug)]
struct BlkRequest {
    req_type: u32, // 0=read, 1=write
    reserved: u32,
    sector: u64, // LBA (little-endian)
}

/// VirtIO block request status byte (written by device after completion).
const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_BLK_S_IOERR: u8 = 1;
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

// Request types
const VIRTIO_BLK_T_IN: u32 = 0; // read
const VIRTIO_BLK_T_OUT: u32 = 1; // write

// Config space offsets (relative to 0x100)
const BLK_CONFIG_CAPACITY_LO: usize = 0x00;
const BLK_CONFIG_CAPACITY_HI: usize = 0x04;

/// A virtio-blk device with its virtqueue.
pub struct VirtioBlk {
    /// MMIO device transport reference.
    pub device: VirtioMmioDevice,
    /// The single virtqueue used for I/O.
    pub vq: VirtQueue,
    /// Total capacity in 512-byte sectors (from config space).
    pub capacity_sectors: u64,
    /// DMA buffer for pending I/O requests (allocated from PMM).
    io_buffer: *mut u8,
    io_buffer_phys: u64,
    /// Status byte for the last completed request.
    status_byte: u8,
    /// I-42: I/O 完成事件. ISR 路径 signal, do_io 等待时检查.
    completion: IoCompletion,
    /// I-42: IRQ 是否已注册到 IDT. true = 走事件驱动, false = 退到原 spin-loop.
    irq_registered: bool,
}

// SAFETY: VirtioBlk uses DMA buffers from PMM; single-owner &mut self
// access ensures no concurrent I/O on the same device. MMIO writes
// use volatile + fence for cross-CPU visibility.
unsafe impl Send for VirtioBlk {}
unsafe impl Sync for VirtioBlk {}

impl VirtioBlk {
    /// Create and initialize a virtio-blk driver instance.
    ///
    /// # Safety
    /// `device` must have device_id == VIRTIO_ID_BLOCK.
    pub fn new(device: VirtioMmioDevice) -> Option<Self> {
        if device.device_id != VIRTIO_ID_BLOCK {
            return None;
        }

        // Initialize the transport layer
        if device.init().is_err() {
            klog_warn!(
                Driver,
                "virtio-blk: device init failed at {:#x}",
                device.iomem.phys().as_u64()
            );
            return None;
        }

        // Allocate the virtqueue
        let vq = VirtQueue::new(false)?; // x86_64 uses modern mode

        // Set up virtqueue 0 on the device
        if device.setup_vq(0, &vq).is_err() {
            return None;
        }

        // Set DRIVER_OK — device goes live after queue configuration
        device.set_driver_ok();

        // Read capacity from config space
        let capacity = device.read_config64(BLK_CONFIG_CAPACITY_LO);

        // Allocate IO buffer: 512 bytes for sector data + request header + status byte
        let buf_size = 512 + core::mem::size_of::<BlkRequest>() + 1;
        let buf_pages = buf_size.div_ceil(4096);
        extern "C" {
            fn pmm_alloc_pages(count: u64) -> *mut u8;
        }
        // SAFETY: extern 函数的参数/返回值类型与 C ABI 声明一致; 调用方保证指针有效
        let buf = unsafe { pmm_alloc_pages(buf_pages as u64) };
        if buf.is_null() {
            return None;
        }

        let buf_phys = buf as u64;
        let buf_virt = (buf_phys + KERNEL_BASE) as *mut u8;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::write_bytes(buf_virt, 0, buf_size);
        }

        klog_info!(
            Driver,
            "virtio-blk: initialized, capacity={} sectors ({:.1} MB)",
            capacity,
            (capacity * 512) as f64 / (1024.0 * 1024.0)
        );

        Some(VirtioBlk {
            device,
            vq,
            capacity_sectors: capacity,
            io_buffer: buf_virt,
            io_buffer_phys: buf_phys,
            status_byte: VIRTIO_BLK_S_OK,
            completion: IoCompletion::new(),
            irq_registered: false,
        })
    }

    /// Read a single sector (512 bytes) into `buf`.
    pub fn read_sector(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), ()> {
        if buf.len() < 512 {
            return Err(());
        }
        self.do_io(lba, VIRTIO_BLK_T_IN, buf)
    }

    /// Write a single sector (512 bytes) from `buf`.
    pub fn write_sector(&mut self, lba: u64, buf: &[u8]) -> Result<(), ()> {
        if buf.len() < 512 {
            return Err(());
        }
        self.do_io(lba, VIRTIO_BLK_T_OUT, buf)
    }

    /// I-42: 注册 virtio-blk IRQ 到 IDT.
    ///
    /// 调用后, do_io 走事件驱动路径: 提交后等待 `completion` 标志,
    /// 由 `virtio_blk_irq_handler` ISR signal. 失败 (例如 IDT 已满, IRQ 已被占用)
    /// 保留 irq_registered = false, do_io 自动退到原 spin-loop 退路.
    #[cfg(target_arch = "x86_64")]
    pub fn enable_irq(&mut self) -> Result<(), &'static str> {
        use crate::kernel::framework::idt::IdtManager;
        if self.irq_registered {
            return Ok(());
        }
        // 先绑定 ISR 目标 (此设备的 completion 引用), 再注册 handler,
        // 保证 ISR 触发时指针已非空.
        bind_virtio_blk_completion(&self.completion);
        let manager = IdtManager::instance();
        manager.register_irq(
            DEFAULT_VIRTIO_BLK_IRQ,
            virtio_blk_irq_handler,
            "virtio-blk",
            0, // flags
        )?;
        manager.enable_irq(DEFAULT_VIRTIO_BLK_IRQ);
        self.irq_registered = true;
        klog_info!(Driver, "virtio-blk IRQ {} registered", DEFAULT_VIRTIO_BLK_IRQ);
        Ok(())
    }

    /// I-42: aarch64 平台 virtio-blk 暂未实现 IRQ 路径, 直接报错.
    #[cfg(target_arch = "aarch64")]
    pub fn enable_irq(&mut self) -> Result<(), &'static str> {
        Err("virtio-blk IRQ not implemented for aarch64")
    }

    /// Execute a single-sector I/O request via the virtqueue.
    ///
    /// Uses chained descriptors:
    ///   desc[0] = BlkRequest header (device-read)
    ///   desc[1] = data buffer (device-read for IN, device-write for OUT)
    ///   desc[2] = status byte (device-write)
    fn do_io(&mut self, lba: u64, req_type: u32, buf: &[u8]) -> Result<(), ()> {
        // ── Build the request in the DMA buffer ──
        let req_size = core::mem::size_of::<BlkRequest>();
        let data_offset = req_size;
        let status_offset = data_offset + 512;

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            // Fill request header
            let req = &mut *(self.io_buffer as *mut BlkRequest);
            req.req_type = req_type.to_le();
            req.reserved = 0;
            req.sector = lba.to_le();

            // For writes, copy data into DMA buffer
            if req_type == VIRTIO_BLK_T_OUT {
                let dst = self.io_buffer.add(data_offset);
                core::ptr::copy_nonoverlapping(buf.as_ptr(), dst, buf.len().min(512));
            }
        }

        // ── Prepare descriptor chain ──
        let desc_req = self
            .vq
            .prepare_desc(self.io_buffer_phys, req_size as u32, false); // device reads header
        let desc_data = self.vq.prepare_desc(
            self.io_buffer_phys + data_offset as u64,
            512,
            req_type == VIRTIO_BLK_T_IN,
        ); // IN=device writes
        let desc_status = self
            .vq
            .prepare_desc(self.io_buffer_phys + status_offset as u64, 1, true); // device writes status

        // Link the chain: req → data → status
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            (*self.vq.desc.add(desc_req as usize)).flags |= super::queue::VQ_DESC_F_NEXT;
            (*self.vq.desc.add(desc_req as usize)).next = desc_data;
            (*self.vq.desc.add(desc_data as usize)).flags |= super::queue::VQ_DESC_F_NEXT;
            (*self.vq.desc.add(desc_data as usize)).next = desc_status;
        }

        // ── Submit and kick ──
        // I-42: 重置完成事件, 提交后才不会误判上一次完成的残留信号.
        self.completion.reset();
        self.vq.submit(desc_req);
        self.vq.commit_and_kick();

        // Ensure writes are visible before notifying the device
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.device.notify(0);

        // ── Wait for completion (I-42: 事件驱动) ──
        //
        // 原实现: 紧接 `pop_used()` 自旋, 长时间空转浪费 CPU, 单核可能活锁.
        // 新实现: 优先等 `completion.done`, 由 `virtio_blk_irq_handler` ISR signal;
        //        若 irq_registered=false (测试/未注册) 或等不到 (timeout 退路),
        //        才进入原 pop_used spin-loop.
        //
        // I-42 timeout: bound ~10ms 等待 ISR, 之后降级为直接 pop_used.
        // 10ms 是 HDD 平均寻道时间, 在此期间 CPU 几乎零开销.
        if self.irq_registered {
            const EVENT_WAIT_BOUND: u32 = 10_000_000; // ~10ms @ 1 GHz spin_loop()
            let mut spins: u32 = 0;
            while !self.completion.is_done() && spins < EVENT_WAIT_BOUND {
                core::hint::spin_loop();
                spins = spins.saturating_add(1);
            }
            if !self.completion.is_done() {
                // 退路: IRQ 未触发 (设备异常), 转 spin-loop 直接 drain used ring.
                klog_warn!(
                    Driver,
                    "virtio-blk completion timeout after {} spins, falling back to poll",
                    spins
                );
            }
        }

        // ── Drain used ring ──
        // IRQ 路径下 `completion` 已 set, 这里一次 pop_used 就拿到结果;
        // poll 退路下走原 spin loop.
        loop {
            if let Some((_id, _len)) = self.vq.pop_used() {
                // Check status byte
                // SAFETY: `self` 由调用方保证为有效指针; 只读访问
                let status = unsafe { *self.io_buffer.add(status_offset) };
                self.vq.reclaim_desc(desc_status);
                self.vq.reclaim_desc(desc_data);
                self.vq.reclaim_desc(desc_req);

                if status != VIRTIO_BLK_S_OK {
                    return Err(());
                }

                // For reads, copy data from DMA buffer to user buffer
                if req_type == VIRTIO_BLK_T_IN {
                    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                    let src = unsafe { self.io_buffer.add(data_offset) };
                    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                    unsafe {
                        core::ptr::copy_nonoverlapping(src, buf.as_ptr() as *mut u8, 512);
                    }
                }

                return Ok(());
            }
            // spin-wait with a hint (仅在 IRQ 模式下不会到这里, 因 completion.is_done
            // 为 true 时 pop_used 必成功; poll 退路或未注册 IRQ 才走此分支).
            core::hint::spin_loop();
        }
    }
}

// I-42: virtio-blk ISR — 设备 used ring 写入后触发, signal 完成事件.
//
// ISR 不做 pop_used, 因为设备可能在事件产生后才把 status byte 写入 DMA,
// 而 do_io 接下来会自己 pop. 这里只把 "有完成" 这件事广播给等待者.
// 多 outstanding I/O 场景下, 此处应根据 used ring entry 索引派发到
// 不同的 request token, 当前每设备串行 I/O, 一个 flag 足够.
//
// 当前全局仅支持一个 virtio-blk 实例; 后续多实例时应改为设备注册表索引
// (类似 chitin_dev IRQ dispatch), 本 fix 不阻塞 (单实例场景).
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub extern "C" fn virtio_blk_irq_handler(_frame: *mut InterruptFrame) {
    // Acknowledge interrupt at device level by writing ISR status.
    // 简化处理: 仅 signal 全局事件; 真正的设备 ISR acknowledge 由 IDT 层 EOI 完成.
    // 设备上下文寄存器写入留作后续优化 (避免重复触发).
    // SAFETY: 静态指针由 bind_virtio_blk_completion 在 enable_irq 前设置一次,
    //         ISR 内仅原子读 done 字段, 不会与 do_io 写形成数据竞争.
    unsafe {
        if !VIRTIO_BLK_COMPLETION_PTR.is_null() {
            (*VIRTIO_BLK_COMPLETION_PTR).signal();
        }
    }
}

// I-42: 暴露给 enable_irq() 绑定 ISR 目标 completion 指针.
// 注册到 IDT 前必须调用, ISR 才能找到正确的完成事件.
#[cfg(target_arch = "x86_64")]
pub fn bind_virtio_blk_completion(c: &IoCompletion) {
    // SAFETY: c 的生命周期 >= 设备生命周期, ISR 仅读 done 字段 (atomic).
    unsafe {
        VIRTIO_BLK_COMPLETION_PTR = c as *const IoCompletion;
    }
}

/// Check if a device ID indicates a block device.
#[inline]
pub fn is_block_device(device_id: u32) -> bool {
    device_id == VIRTIO_ID_BLOCK
}

// ── BlockDevice trait implementation ──

impl BlockDevice for VirtioBlk {
    fn blk_read(&mut self, sector: u64, buf: &mut [u8]) -> i32 {
        match self.read_sector(sector, buf) {
            Ok(()) => 0,
            Err(()) => -1,
        }
    }

    fn blk_write(&mut self, sector: u64, buf: &[u8]) -> i32 {
        match self.write_sector(sector, buf) {
            Ok(()) => 0,
            Err(()) => -1,
        }
    }

    fn blk_is_present(&self) -> bool {
        true
    }

    fn blk_total_sectors(&self) -> u64 {
        self.capacity_sectors
    }
}
