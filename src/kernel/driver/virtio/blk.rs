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
use crate::kernel::driver::block::BlockDevice;
use crate::kernel::mm::KERNEL_BASE;
use crate::klog_info;
use crate::klog_warn;

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
                device.mmio_base
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
        let buf = unsafe { pmm_alloc_pages(buf_pages as u64) };
        if buf.is_null() {
            return None;
        }

        let buf_phys = buf as u64;
        let buf_virt = (buf_phys + KERNEL_BASE) as *mut u8;
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
        unsafe {
            (*self.vq.desc.add(desc_req as usize)).flags |= super::queue::VQ_DESC_F_NEXT;
            (*self.vq.desc.add(desc_req as usize)).next = desc_data;
            (*self.vq.desc.add(desc_data as usize)).flags |= super::queue::VQ_DESC_F_NEXT;
            (*self.vq.desc.add(desc_data as usize)).next = desc_status;
        }

        // ── Submit and kick ──
        self.vq.submit(desc_req);
        self.vq.commit_and_kick();

        // Ensure writes are visible before notifying the device
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.device.notify(0);

        // ── Poll for completion ──
        // Simple polling loop (TODO: use interrupt-driven completion)
        loop {
            if let Some((_id, _len)) = self.vq.pop_used() {
                // Check status byte
                let status = unsafe { *self.io_buffer.add(status_offset) };
                self.vq.reclaim_desc(desc_status);
                self.vq.reclaim_desc(desc_data);
                self.vq.reclaim_desc(desc_req);

                if status != VIRTIO_BLK_S_OK {
                    return Err(());
                }

                // For reads, copy data from DMA buffer to user buffer
                if req_type == VIRTIO_BLK_T_IN {
                    let src = unsafe { self.io_buffer.add(data_offset) };
                    unsafe {
                        core::ptr::copy_nonoverlapping(src, buf.as_ptr() as *mut u8, 512);
                    }
                }

                return Ok(());
            }
            // Spin-wait with a hint
            core::hint::spin_loop();
        }
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
