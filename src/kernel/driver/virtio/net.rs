//! VirtIO Network Device Driver (设备 ID 1)
//!
//! Implements the VirtIO Network Device specification.
//! Uses 2 split virtqueues:
//!   - Queue 0: Receive (RX) — device writes received packets
//!   - Queue 1: Transmit (TX) — driver writes packets to send
//!
//! Config space (offset 0x100):
//!   - 0x00: mac[6] (MAC address, if VIRTIO_NET_F_MAC is set)
//!   - 0x06: status (u16, if VIRTIO_NET_F_STATUS is set)

use super::queue::{VirtQueue, VQ_SIZE};
use super::{VirtioMmioDevice, VIRTIO_ID_NET};
use crate::kernel::mm::KERNEL_BASE;
use crate::klog_info;
use crate::klog_warn;
use crate::klog_err;

// ── Feature bits ──

const VIRTIO_NET_F_MAC:    u64 = 1 << 5;
const VIRTIO_NET_F_STATUS: u64 = 1 << 16;

// ── VirtIO Net Header (always 12 bytes) ──
// Both legacy and v1 headers are identical in Linux/QEMU:
// flags(1) + gso_type(1) + hdr_len(2) + gso_size(2) + csum_start(2) + csum_offset(2) + num_buffers(2) = 12

/// virtio_net_hdr — header before every TX/RX packet.
/// Both legacy and VERSION_1 use the same 12-byte layout.
/// `num_buffers` is only meaningful when VIRTIO_NET_F_MRG_RXBUF is negotiated.
#[repr(C)]
struct VirtioNetHdr {
    flags:         u8,
    gso_type:      u8,
    hdr_len:       u16,
    gso_size:      u16,
    csum_start:    u16,
    csum_offset:   u16,
    num_buffers:   u16,
}

/// Size of virtio_net_hdr (12 bytes).
const NET_HDR_SIZE: usize = core::mem::size_of::<VirtioNetHdr>();

// ── Config space offsets (relative to 0x100) ──

const NET_CONFIG_MAC:    usize = 0x00; // 6 bytes
const NET_CONFIG_STATUS: usize = 0x06; // 2 bytes

// ── RX buffer constants ──

const RX_BUFFER_SIZE: usize = 2048;

// ── QEMU virt: virtio-net MMIO base ──

/// QEMU virt aarch64 places virtio-net at 0x0a000000 (first device).
/// The virtio-net device ID is 1, so probe_all() will find it.
const VIRTIO_NET_MMIO_BASE_HINT: u64 = 0x0a00_0000;

/// A virtio-net device with its virtqueues.
pub struct VirtioNet {
    /// MMIO device transport reference.
    pub device: VirtioMmioDevice,
    /// RX virtqueue (queue 0).
    pub rx_vq: VirtQueue,
    /// TX virtqueue (queue 1).
    pub tx_vq: VirtQueue,
    /// MAC address from config space.
    pub mac: [u8; 6],
    /// Link status from config space (1 = up).
    pub link_up: bool,
    /// RX buffer memory (allocated from PMM).
    rx_buffers: [*mut u8; VQ_SIZE as usize],
    /// RX buffer physical addresses.
    rx_buffers_phys: [u64; VQ_SIZE as usize],
    /// Net header size: 10 (modern VERSION_1) or 12 (legacy).
    pub hdr_size: usize,
    /// Statistics.
    pub tx_count: u64,
    pub rx_count: u64,
}

// SAFETY: Single-core, DMA buffers from PMM.
unsafe impl Send for VirtioNet {}
unsafe impl Sync for VirtioNet {}

impl VirtioNet {
    /// Create and initialize a virtio-net driver instance.
    ///
    /// Caller must ensure `device` has device_id == VIRTIO_ID_NET.
    pub fn new(device: VirtioMmioDevice) -> Option<Self> {
        if device.device_id != VIRTIO_ID_NET {
            return None;
        }

        klog_info!(Driver, "virtio-net: initializing at {:#x}", device.mmio_base);

        // Read device version to determine legacy vs modern
        let version = unsafe { device.read32(super::VERSION) };
        let is_legacy = version == 1; // 1=transitional, 2=modern-only
        klog_info!(Driver, "virtio-net: version={}, {} mode",
            version, if is_legacy { "legacy" } else { "modern" });

        // Feature negotiation
        let negotiated_v1: bool;
        let hdr_size: usize;
        unsafe {
            use super::STATUS;
            use super::STATUS_ACKNOWLEDGE;
            use super::STATUS_DRIVER;
            use super::STATUS_FEATURES_OK;

            // Reset + ACKNOWLEDGE + DRIVER
            device.write32(STATUS, 0);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            device.write32(STATUS, STATUS_ACKNOWLEDGE);
            device.write32(STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);

            // Read device features
            device.write32(super::DEVICE_FEATURES_SEL, 1);
            let dev_feat_hi = device.read32(super::DEVICE_FEATURES);
            device.write32(super::DEVICE_FEATURES_SEL, 0);
            let dev_feat_lo = device.read32(super::DEVICE_FEATURES);
            let dev_features = (dev_feat_hi as u64) << 32 | dev_feat_lo as u64;
            klog_info!(Driver, "virtio-net: dev_features={:#018x}", dev_features);

            let has_v1 = (dev_features & super::VIRTIO_F_VERSION_1) != 0;

            if !is_legacy || has_v1 {
                // Modern mode: negotiate VIRTIO_F_VERSION_1 + VIRTIO_NET_F_MAC
                negotiated_v1 = true;
                hdr_size = NET_HDR_SIZE;
                let feat = super::VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC;
                device.write32(super::DRIVER_FEATURES_SEL, 1);
                device.write32(super::DRIVER_FEATURES, (feat >> 32) as u32);
                device.write32(super::DRIVER_FEATURES_SEL, 0);
                device.write32(super::DRIVER_FEATURES, (feat & 0xFFFF_FFFF) as u32);
                klog_info!(Driver, "virtio-net: negotiating modern features (VERSION_1)");
            } else {
                // Legacy mode: only VIRTIO_NET_F_MAC
                negotiated_v1 = false;
                hdr_size = NET_HDR_SIZE;
                device.write32(super::DRIVER_FEATURES_SEL, 1);
                device.write32(super::DRIVER_FEATURES, 0);
                device.write32(super::DRIVER_FEATURES_SEL, 0);
                device.write32(super::DRIVER_FEATURES, VIRTIO_NET_F_MAC as u32);
                klog_info!(Driver, "virtio-net: negotiating legacy features");
            }

            // FEATURES_OK
            device.write32(STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);
            let status = device.read32(STATUS);
            if status & STATUS_FEATURES_OK == 0 {
                klog_warn!(Driver, "virtio-net: FEATURES_OK rejected");
                return None;
            }
        }

        // Read MAC address from config space
        let mut mac: [u8; 6] = [0; 6];
        for i in 0..6 {
            mac[i] = (device.read_config32(NET_CONFIG_MAC + (i & !3)) >> ((i & 3) * 8)) as u8;
        }

        klog_info!(Driver, "virtio-net: MAC={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);

        // Read link status
        let link_status = device.read_config32(NET_CONFIG_STATUS) as u16;
        let link_up = (link_status & 1) != 0;
        klog_info!(Driver, "virtio-net: link {}", if link_up { "up" } else { "down" });

        // Queue layout: modern=sequential, legacy=page-aligned used ring
        let vq_legacy = !negotiated_v1;

        klog_info!(Driver, "virtio-net: allocating RX vq (legacy={})", vq_legacy);

        // Allocate RX virtqueue (queue 0)
        let rx_vq = VirtQueue::new(vq_legacy)?;
        klog_info!(Driver, "virtio-net: RX vq allocated, setting up...");
        if vq_legacy {
            if device.setup_vq_legacy(0, &rx_vq).is_err() {
                klog_err!(Driver, "virtio-net: failed to setup RX vq (legacy)");
                return None;
            }
        } else {
            if device.setup_vq(0, &rx_vq).is_err() {
                klog_err!(Driver, "virtio-net: failed to setup RX vq (modern)");
                return None;
            }
        }
        klog_info!(Driver, "virtio-net: RX vq ready");

        // Allocate TX virtqueue (queue 1)
        klog_info!(Driver, "virtio-net: allocating TX vq");
        let tx_vq = VirtQueue::new(vq_legacy)?;
        klog_info!(Driver, "virtio-net: TX vq allocated, setting up...");
        if vq_legacy {
            if device.setup_vq_legacy(1, &tx_vq).is_err() {
                klog_err!(Driver, "virtio-net: failed to setup TX vq (legacy)");
                return None;
            }
        } else {
            if device.setup_vq(1, &tx_vq).is_err() {
                klog_err!(Driver, "virtio-net: failed to setup TX vq (modern)");
                return None;
            }
        }

        // Set DRIVER_OK — device goes live after queue configuration
        device.set_driver_ok();

        let mut net = VirtioNet {
            device,
            rx_vq,
            tx_vq,
            mac,
            link_up,
            rx_buffers: [core::ptr::null_mut(); VQ_SIZE as usize],
            rx_buffers_phys: [0; VQ_SIZE as usize],
            hdr_size,
            tx_count: 0,
            rx_count: 0,
        };

        // Pre-fill RX queue with empty buffers
        net.refill_rx();

        Some(net)
    }

    /// Fill the RX virtqueue with empty buffers for the device to write into.
    fn refill_rx(&mut self) {
        for i in 0..VQ_SIZE as usize {
            if !self.rx_buffers[i].is_null() {
                continue; // Already filled
            }

            // Allocate a buffer for this RX slot
            let pages = (RX_BUFFER_SIZE + 4095) / 4096;
            extern "C" { fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void; }
            let buf = unsafe { pmm_alloc_pages(pages as u64) };
            if buf.is_null() {
                klog_warn!(Driver, "virtio-net: failed to alloc RX buffer {}", i);
                return;
            }

            let buf_phys = buf as u64;
            let buf_virt = (buf_phys + KERNEL_BASE) as *mut u8;
            self.rx_buffers[i] = buf_virt;
            self.rx_buffers_phys[i] = buf_phys;

            // Prepare descriptor: device writes to this buffer
            let desc_idx = self.rx_vq.prepare_desc(buf_phys, RX_BUFFER_SIZE as u32, true);
            self.rx_vq.submit(desc_idx);
        }

        // Kick the device to let it start using these buffers
        self.rx_vq.commit_and_kick();
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.device.notify(0);
    }

    /// Send a packet through the TX virtqueue.
    ///
    /// `data` points to the packet buffer (physically contiguous).
    /// `len` is the packet length.
    pub fn send_packet(&mut self, data_phys: u64, len: u32) -> Result<(), ()> {
        if len == 0 || len > 65535 {
            return Err(());
        }

        // Prepare a single TX descriptor (device reads from this buffer)
        let desc_idx = self.tx_vq.prepare_desc(data_phys, len, false);

        // Submit and kick
        self.tx_vq.submit(desc_idx);
        self.tx_vq.commit_and_kick();

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.device.notify(1);

        // Poll for TX completion with MMIO reads to force VM exits
        for _ in 0..100000 {
            if let Some((_id, _len)) = self.tx_vq.pop_used() {
                self.tx_vq.reclaim_desc(desc_idx);
                self.tx_count += 1;
                return Ok(());
            }
            unsafe { self.device.read32(super::INTERRUPT_STATUS); }
            for _ in 0..100 { core::hint::spin_loop(); }
        }

        klog_warn!(Driver, "virtio-net: TX timeout (desc_idx={}, data={:#x}, len={})",
            desc_idx, data_phys, len);
        Err(())
    }

    /// Poll the RX queue for received packets.
    /// Returns number of packets processed.
    pub fn poll_rx(&mut self) -> usize {
        let mut processed = 0;

        loop {
            let result = self.rx_vq.pop_used();
            if result.is_none() {
                break;
            }

            let (desc_idx, len) = result.unwrap();
            let buf_idx = desc_idx as usize;

            if buf_idx < VQ_SIZE as usize && !self.rx_buffers[buf_idx].is_null() {
                // Process the received packet
                // Device writes virtio_net_hdr before the ethernet frame
                if len > self.hdr_size as u32 && len <= RX_BUFFER_SIZE as u32 {
                    let data_len = (len - self.hdr_size as u32) as u16;
                    extern "C" {
                        fn ethernet_input_from_virtio(
                            data: *mut core::ffi::c_void,
                            len: u16,
                        ) -> i32;
                    }
                    unsafe {
                        ethernet_input_from_virtio(
                            self.rx_buffers[buf_idx].add(self.hdr_size) as *mut core::ffi::c_void,
                            data_len,
                        );
                    }
                    self.rx_count += 1;
                    processed += 1;
                }

                // Recycle: re-prepare the buffer for device writing
                self.rx_vq.reclaim_desc(desc_idx);
                let new_desc = self.rx_vq.prepare_desc(
                    self.rx_buffers_phys[buf_idx],
                    RX_BUFFER_SIZE as u32,
                    true,
                );
                self.rx_vq.submit(new_desc);
            }
        }

        // Kick if we recycled any buffers
        if processed > 0 {
            self.rx_vq.commit_and_kick();
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            self.device.notify(0);
        }

        processed
    }

    /// Process any used descriptors for RX reclamation without processing data.
    /// Used for interrupt-driven polling.
    pub fn handle_interrupt(&self) {
        // Read and clear interrupt status
        unsafe {
            self.device.write32(0x64, self.device.read32(0x60)); // ACK = STATUS
        }
    }
}

// ============================================================================
// Global instance + FFI (for lwIP integration)
// ============================================================================

static mut VIRTIO_NET_INSTANCE: Option<VirtioNet> = None;

/// C FFI: probe for virtio-net device. Returns 0 on success, -1 on failure.
#[no_mangle]
pub unsafe extern "C" fn virtio_net_probe() -> i32 {
    probe()
}

/// Probe for a virtio-net device and create a global instance.
///
/// Returns 0 on success, -1 on failure.
pub fn probe() -> i32 {
    let devices = super::probe_all();
    for dev in devices {
        if dev.device_id == VIRTIO_ID_NET {
            match VirtioNet::new(dev) {
                Some(net) => unsafe {
                    VIRTIO_NET_INSTANCE = Some(net);
                    klog_info!(Driver, "virtio-net: probed successfully");
                    return 0;
                },
                None => {
                    klog_warn!(Driver, "virtio-net: found device but init failed");
                }
            }
        }
    }
    klog_info!(Driver, "virtio-net: no device found");
    -1
}

/// Initialize virtio-net for lwIP. Called by netif_add's init callback.
///
/// The device must already be probed via `probe()`.
#[no_mangle]
pub unsafe extern "C" fn virtio_net_init(netif: *mut core::ffi::c_void) -> i32 {
    extern "C" {
        fn qx_netif_init_virtio(netif: *mut core::ffi::c_void, mac: *const u8);
    }

    match &mut VIRTIO_NET_INSTANCE {
        Some(ref dev) => {
            qx_netif_init_virtio(netif, dev.mac.as_ptr());
            klog_info!(Net, "virtio-net: lwIP netif initialized");
            0
        }
        None => -5,
    }
}

/// Send a packet via virtio-net. Called by lwIP's linkoutput callback.
#[no_mangle]
pub unsafe extern "C" fn virtio_net_send(_netif: *mut core::ffi::c_void, p: *mut core::ffi::c_void) -> i32 {
    extern "C" {
        fn qx_pbuf_copyout(p: *mut core::ffi::c_void, buf: *mut u8, out_len: *mut u16);
    }

    if p.is_null() {
        return -1;
    }

    match &mut VIRTIO_NET_INSTANCE {
        Some(ref mut dev) => {
            let pbuf_base = p as *mut u8;
            let total = *(pbuf_base.add(0x10) as *const u16) as usize;

            // Use static TX buffer with space for virtio_net_hdr
            static mut TX_BUF: [u8; 2000] = [0u8; 2000];
            let hdr_off = dev.hdr_size;

            // Zero the virtio header before each send (no offloads)
            for i in 0..hdr_off {
                TX_BUF[i] = 0;
            }

            let max_payload = TX_BUF.len() - hdr_off;
            let mut out_len: u16 = total.min(max_payload) as u16;
            qx_pbuf_copyout(p, TX_BUF[hdr_off..].as_mut_ptr(), &mut out_len);

            // Convert to physical address (identity-mapped on aarch64)
            let phys = TX_BUF.as_ptr() as u64;

            match dev.send_packet(phys, (hdr_off + out_len as usize) as u32) {
                Ok(()) => 0,
                Err(()) => {
                    klog_err!(Net, "virtio-net: send failed");
                    -1
                }
            }
        }
        None => -1,
    }
}

/// Poll for received packets.
#[no_mangle]
pub unsafe extern "C" fn virtio_net_poll_rx() {
    match &mut VIRTIO_NET_INSTANCE {
        Some(ref mut dev) => {
            // Force VM exit to let QEMU process pending network bottom-half handlers
            let _ = dev.device.read32(super::INTERRUPT_STATUS);
            let n = dev.poll_rx();
            if n > 0 {
                klog_info!(Driver, "virtio-net: poll_rx processed {} packets", n);
            }
        }
        None => {}
    }
}

/// Forward received packet to lwIP.
#[no_mangle]
pub unsafe extern "C" fn virtio_net_input(
    data: *mut core::ffi::c_void,
    len: u16,
) -> i32 {
    // External declaration (defined in netif.rs)
    extern "C" {
        fn ethernet_input_from_virtio(data: *mut core::ffi::c_void, len: u16) -> i32;
    }
    ethernet_input_from_virtio(data, len)
}