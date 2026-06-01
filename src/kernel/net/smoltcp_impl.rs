//! smoltcp 网络协议栈集成模块
//!
//! 实现 smoltcp 的 `Device` trait，封装 E1000 / Virtio-Net 硬件，
//! 提供网络栈初始化与轮询功能。
//!
//! ## 架构
//!
//! ```text
//! smoltcp Interface
//! ├── phy::Device ── NetNic enum
//! │   ├── E1000(AntxE1000Device)
//! │   └── Virtio(AntxVirtioDevice)
//! ├── SocketSet ──── DHCP / TCP 等 socket (外部管理)
//! └── Config ─────── MAC / IP / MTU 等配置
//! ```
//!
//! ## 多网卡支持
//!
//! `NetNic` 枚举统一封装所有网卡类型，消除架构级 `#[cfg]` 分支。
//! 新增网卡只需：
//! 1. 实现 `smoltcp::phy::Device` 的包装器结构体
//! 2. 在 `NetNic` 枚举中添加变体
//! 3. 在 `nic_probe_all()` 中添加探测逻辑
//!
//! ## 安全注意事项
//! - TX buffer 的虚拟地址在 aarch64 下即为物理地址 (KERNEL_BASE=0)
//! - x86_64 下使用 virt_to_phys 转换为物理地址
//! - try_receive 内部完成 DMA buffer → CPU buffer 的拷贝

use smoltcp::iface::{Config, Interface, PollResult, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress};

use crate::kernel::mm::KERNEL_BASE;
use crate::kernel::driver::net::e1000::E1000Device;
use crate::kernel::timer::tick::get_uptime_ms;

#[cfg(target_arch = "aarch64")]
use crate::kernel::driver::virtio::net::VirtioNet;

const RX_BUF_SIZE: usize = 2048;
const TX_BUF_SIZE: usize = 2048;

#[allow(dead_code)]
fn virt_to_phys(virt: u64) -> u64 {
    if virt >= KERNEL_BASE {
        virt - KERNEL_BASE
    } else {
        virt
    }
}

fn smoltcp_now() -> Instant {
    Instant::from_millis(get_uptime_ms() as i64)
}

// ============================================================================
// x86_64: E1000 Device 包装器
// ============================================================================

#[cfg(target_arch = "x86_64")]
pub struct AntxE1000Device {
    pub inner: E1000Device,
    rx_buf: [u8; RX_BUF_SIZE],
    rx_len: usize,
    tx_buf: [u8; TX_BUF_SIZE],
}

#[cfg(target_arch = "x86_64")]
pub struct AntxE1000RxToken<'a> {
    buf: &'a [u8],
}

#[cfg(target_arch = "x86_64")]
pub struct AntxE1000TxToken<'a> {
    tx_buf: &'a mut [u8],
    inner: &'a mut E1000Device,
}

#[cfg(target_arch = "x86_64")]
impl AntxE1000Device {
    pub fn new(inner: E1000Device) -> Self {
        Self {
            inner,
            rx_buf: [0u8; RX_BUF_SIZE],
            rx_len: 0,
            tx_buf: [0u8; TX_BUF_SIZE],
        }
    }
}

#[cfg(target_arch = "x86_64")]
impl smoltcp::phy::Device for AntxE1000Device {
    type RxToken<'a> = AntxE1000RxToken<'a> where Self: 'a;
    type TxToken<'a> = AntxE1000TxToken<'a> where Self: 'a;

    fn receive(
        &mut self,
        _timestamp: Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let len = self.inner.try_receive(&mut self.rx_buf)?;
        self.rx_len = len;
        let rx = AntxE1000RxToken {
            buf: &self.rx_buf[..self.rx_len],
        };
        let tx = AntxE1000TxToken {
            tx_buf: &mut self.tx_buf[..],
            inner: &mut self.inner,
        };
        Some((rx, tx))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(AntxE1000TxToken {
            tx_buf: &mut self.tx_buf[..],
            inner: &mut self.inner,
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(64);
        caps.medium = Medium::Ethernet;
        caps
    }
}

#[cfg(target_arch = "x86_64")]
impl<'a> RxToken for AntxE1000RxToken<'a> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(self.buf)
    }
}

#[cfg(target_arch = "x86_64")]
impl<'a> TxToken for AntxE1000TxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let result = f(&mut self.tx_buf[..len]);
        self.inner.send_packet(&self.tx_buf[..len]).ok();
        result
    }
}

// ============================================================================
// aarch64: Virtio-Net Device 包装器
// ============================================================================

#[cfg(target_arch = "aarch64")]
pub struct AntxVirtioDevice {
    pub inner: VirtioNet,
    rx_buf: [u8; RX_BUF_SIZE],
    rx_len: usize,
    tx_buf: [u8; TX_BUF_SIZE],
}

#[cfg(target_arch = "aarch64")]
pub struct AntxVirtioRxToken<'a> {
    buf: &'a [u8],
}

#[cfg(target_arch = "aarch64")]
pub struct AntxVirtioTxToken<'a> {
    tx_buf: &'a mut [u8],
    inner: &'a mut VirtioNet,
}

#[cfg(target_arch = "aarch64")]
impl AntxVirtioDevice {
    pub fn new(inner: VirtioNet) -> Self {
        Self {
            inner,
            rx_buf: [0u8; RX_BUF_SIZE],
            rx_len: 0,
            tx_buf: [0u8; TX_BUF_SIZE],
        }
    }
}

#[cfg(target_arch = "aarch64")]
impl smoltcp::phy::Device for AntxVirtioDevice {
    type RxToken<'a> = AntxVirtioRxToken<'a> where Self: 'a;
    type TxToken<'a> = AntxVirtioTxToken<'a> where Self: 'a;

    fn receive(
        &mut self,
        _timestamp: Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let len = self.inner.try_receive(&mut self.rx_buf)?;
        self.rx_len = len;
        let rx = AntxVirtioRxToken {
            buf: &self.rx_buf[..self.rx_len],
        };
        let tx = AntxVirtioTxToken {
            tx_buf: &mut self.tx_buf[..],
            inner: &mut self.inner,
        };
        Some((rx, tx))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(AntxVirtioTxToken {
            tx_buf: &mut self.tx_buf[..],
            inner: &mut self.inner,
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(64);
        caps.medium = Medium::Ethernet;
        caps
    }
}

#[cfg(target_arch = "aarch64")]
impl<'a> RxToken for AntxVirtioRxToken<'a> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(self.buf)
    }
}

#[cfg(target_arch = "aarch64")]
impl<'a> TxToken for AntxVirtioTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let result = f(&mut self.tx_buf[..len]);
        let tx_phys = virt_to_phys(self.tx_buf.as_ptr() as u64);
        self.inner.send_packet(tx_phys, len as u32).ok();
        result
    }
}

// ============================================================================
// NetNic — 统一网卡枚举
// ============================================================================

pub enum NetNic {
    #[cfg(target_arch = "x86_64")]
    E1000(AntxE1000Device),
    #[cfg(target_arch = "aarch64")]
    Virtio(AntxVirtioDevice),
}

impl NetNic {
    pub fn mac(&self) -> [u8; 6] {
        match self {
            #[cfg(target_arch = "x86_64")]
            NetNic::E1000(d) => d.inner.mac,
            #[cfg(target_arch = "aarch64")]
            NetNic::Virtio(d) => d.inner.mac,
        }
    }
}

// ============================================================================
// smoltcp 网络栈管理（轻量包装器）
// ============================================================================

pub struct NetworkStack {
    pub iface: Interface,
    pub mac: [u8; 6],
    pub initialized: bool,
}

impl NetworkStack {
    pub fn poll<D: Device>(&mut self, device: &mut D, sockets: &mut SocketSet<'_>) -> PollResult {
        self.iface.poll(smoltcp_now(), device, sockets)
    }
}

// ============================================================================
// 公共 API：统一的初始化与轮询
// ============================================================================

pub fn init_stack(nic: &mut NetNic, mac: [u8; 6]) -> NetworkStack {
    let config = Config::new(HardwareAddress::Ethernet(EthernetAddress::from_bytes(
        &mac,
    )));
    let iface = match nic {
        #[cfg(target_arch = "x86_64")]
        NetNic::E1000(dev) => Interface::new(config, dev, smoltcp_now()),
        #[cfg(target_arch = "aarch64")]
        NetNic::Virtio(dev) => Interface::new(config, dev, smoltcp_now()),
    };
    NetworkStack {
        iface,
        mac,
        initialized: true,
    }
}

pub fn poll_stack(nic: &mut NetNic, stack: &mut NetworkStack, sockets: &mut SocketSet<'_>) {
    match nic {
        #[cfg(target_arch = "x86_64")]
        NetNic::E1000(dev) => {
            stack.poll(dev, sockets);
        }
        #[cfg(target_arch = "aarch64")]
        NetNic::Virtio(dev) => {
            stack.poll(dev, sockets);
        }
    }
}

// ============================================================================
// 多网卡探测
// ============================================================================

pub fn nic_name(nic: &NetNic) -> &'static str {
    match nic {
        #[cfg(target_arch = "x86_64")]
        NetNic::E1000(_) => "e1000",
        #[cfg(target_arch = "aarch64")]
        NetNic::Virtio(_) => "virtio-net",
    }
}