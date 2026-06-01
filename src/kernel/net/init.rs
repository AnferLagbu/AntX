#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use spin::Mutex;

use crate::kernel::klog::{klog_net, klog_net_err, klog_init_msg};
use crate::kernel::net::types::*;

use crate::kernel::net::smoltcp_impl::{self, ChitinNetDevice, NetworkStack};
use smoltcp::iface::{SocketHandle, SocketSet, SocketStorage};
use smoltcp::socket::dhcpv4;
use smoltcp::socket::{tcp, udp};
use smoltcp::wire::{IpCidr, IpEndpoint, IpListenEndpoint, IpAddress, Ipv4Address};

// ============================================================================
// 初始化状态管理
// ============================================================================

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitState {
    Uninitialized = 0,
    HardwareProbed = 1,
    InterfaceReady = 2,
    FullyInitialized = 3,
    Failed = 255,
}

static G_INIT_STATE: AtomicU8 = AtomicU8::new(InitState::Uninitialized as u8);

// ============================================================================
// 全局网络状态
//
// 所有 static mut 变量必须在 NET_LOCK 保护下访问。
// NET_LOCK 是全局网络互斥锁，确保 SMP 环境下不会发生数据竞争。
// poll_network() 使用 try_lock() 避免在 ISR 上下文中阻塞；
// 其他函数使用 lock() 获取互斥访问。
// ============================================================================

static NET_LOCK: Mutex<()> = Mutex::new(());

static mut NET_DEVICE: Option<ChitinNetDevice> = None;
static mut NET_STACK: Option<NetworkStack> = None;

const MAX_SOCKETS: usize = 8;
static mut SOCKET_STORAGE: core::mem::MaybeUninit<[SocketStorage<'static>; MAX_SOCKETS]> =
    core::mem::MaybeUninit::uninit();
static mut SOCKET_SET: core::mem::MaybeUninit<SocketSet<'static>> =
    core::mem::MaybeUninit::uninit();
static SOCKETS_INITIALIZED: AtomicBool = AtomicBool::new(false);
static mut DHCP_HANDLE: Option<SocketHandle> = None;

// ============================================================================
// 辅助函数
// ============================================================================

fn transition_state(from: InitState, to: InitState) -> Result<(), ()> {
    match G_INIT_STATE.compare_exchange(from as u8, to as u8, Ordering::AcqRel, Ordering::Relaxed) {
        Ok(_) => Ok(()),
        Err(current) => {
            if current == InitState::Failed as u8 {
                Err(())
            } else if current >= to as u8 {
                Ok(())
            } else {
                Err(())
            }
        }
    }
}

fn set_failed() {
    G_INIT_STATE.store(InitState::Failed as u8, Ordering::Release);
}

unsafe fn init_sockets() {
    if SOCKETS_INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    let ptr = SOCKET_STORAGE.as_mut_ptr() as *mut SocketStorage<'static>;
    for i in 0..MAX_SOCKETS {
        core::ptr::write(ptr.add(i), SocketStorage::EMPTY);
    }
    let storage = SOCKET_STORAGE.assume_init_mut();
    SOCKET_SET.write(SocketSet::new(&mut storage[..]));
    SOCKETS_INITIALIZED.store(true, Ordering::Release);
}

unsafe fn socket_set() -> *mut SocketSet<'static> {
    SOCKET_SET.as_mut_ptr()
}

unsafe fn process_dhcp_events(sockets: &mut SocketSet<'_>) {
    static FIRST_DECONFIG: AtomicBool = AtomicBool::new(true);

    let dhcp_handle = match unsafe { DHCP_HANDLE } {
        Some(h) => h,
        None => return,
    };

    let dhcp = sockets.get_mut::<dhcpv4::Socket>(dhcp_handle);
    let event = dhcp.poll();
    match event {
        None => {}
        Some(dhcpv4::Event::Deconfigured) => {
            if FIRST_DECONFIG.swap(false, Ordering::AcqRel) {
                return;
            }
            if let Some(stack) = unsafe { NET_STACK.as_mut() } {
                stack.iface.update_ip_addrs(|addrs| {
                    addrs.clear();
                });
                let _ = stack.iface.routes_mut().remove_default_ipv4_route();
            }
            crate::kernel::net::types::NET_CONFIGURED.store(false, Ordering::Release);
            unsafe {
                klog_net("DHCP deconfigured\0".as_ptr().cast());
            }
        }
        Some(dhcpv4::Event::Configured(config)) => {
            FIRST_DECONFIG.store(false, Ordering::Release);
            if let Some(stack) = unsafe { NET_STACK.as_mut() } {
                let cidr = config.address;
                stack.iface.update_ip_addrs(|addrs| {
                    addrs.clear();
                    let _ = addrs.push(IpCidr::Ipv4(cidr));
                });
                if let Some(router) = config.router {
                    let _ = stack.iface.routes_mut().add_default_ipv4_route(router);
                }
            }
            crate::kernel::net::types::NET_CONFIGURED.store(true, Ordering::Release);
            unsafe {
                klog_net("DHCP configured\0".as_ptr().cast());
            }
        }
    }
}

// ============================================================================
// 网络轮询 (统一入口，与具体网卡无关)
//
// 使用 NET_LOCK.try_lock() 确保互斥访问。
// try_lock() 在 ISR 上下文中不会阻塞：若锁已被持有则直接返回。
// ============================================================================

pub unsafe fn poll_network() {
    let _guard = match NET_LOCK.try_lock() {
        Some(g) => g,
        None => return,
    };

    let nic = match unsafe { NET_DEVICE.as_mut() } {
        Some(d) => d,
        None => return,
    };
    let stack = match unsafe { NET_STACK.as_mut() } {
        Some(s) => s,
        None => return,
    };
    let sockets = &mut *unsafe { socket_set() };
    smoltcp_impl::poll_stack(nic, stack, sockets);
    unsafe { process_dhcp_events(sockets) };
}

// ============================================================================
// 多网卡探测 (按优先级依次尝试)
// ============================================================================

unsafe fn nic_probe_all() -> Option<ChitinNetDevice> {
    #[cfg(target_arch = "x86_64")]
    {
        let probe_result = crate::kernel::driver::net::e1000::e1000_probe();
        if probe_result == 0 {
            let mut dev = crate::kernel::driver::net::e1000::take_device()?;
            if crate::kernel::driver::framework::Driver::init(&mut *dev).is_err() {
                klog_net_err("e1000: hardware init failed\0".as_ptr().cast());
                return None;
            }
            let mac = dev.mac;
            let raw_ptr = alloc::boxed::Box::into_raw(dev) as *mut core::ffi::c_void;
            let nic = ChitinNetDevice::new(&E1000_NET_OPS_STATIC, raw_ptr, mac);
            klog_net("e1000: probed successfully\0".as_ptr().cast());
            return Some(nic);
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        let probe_result = crate::kernel::driver::virtio::net::virtio_net_probe();
        if probe_result == 0 {
            let dev = crate::kernel::driver::virtio::net::take_device()?;
            let mac = dev.mac;
            let raw_ptr = alloc::boxed::Box::into_raw(dev) as *mut core::ffi::c_void;
            let nic = ChitinNetDevice::new(&VIRTIO_NET_OPS_STATIC, raw_ptr, mac);
            klog_net("virtio-net: probed successfully\0".as_ptr().cast());
            return Some(nic);
        }
    }

    None
}

static E1000_NET_OPS_STATIC: crate::kernel::chitin::proto_net::NetOps =
    crate::kernel::chitin::proto_net::NetOps {
        send: crate::kernel::driver::net::e1000::e1000_net_send,
        try_receive: crate::kernel::driver::net::e1000::e1000_net_recv,
        get_mac: crate::kernel::driver::net::e1000::e1000_net_get_mac,
        handle_irq: Some(crate::kernel::driver::net::e1000::e1000_net_irq),
    };

static VIRTIO_NET_OPS_STATIC: crate::kernel::chitin::proto_net::NetOps =
    crate::kernel::chitin::proto_net::NetOps {
        send: crate::kernel::driver::virtio::net::virtio_net_send,
        try_receive: crate::kernel::driver::virtio::net::virtio_net_recv,
        get_mac: crate::kernel::driver::virtio::net::virtio_net_get_mac,
        handle_irq: Some(crate::kernel::driver::virtio::net::virtio_net_irq),
    };

// ============================================================================
// 恢复机制
// ============================================================================

unsafe extern "C" fn net_save() {}

unsafe extern "C" fn net_restore() {
    let _guard = NET_LOCK.lock();

    crate::kernel::net::types::NET_READY.store(false, Ordering::Release);
    crate::kernel::net::types::NET_CONFIGURED.store(false, Ordering::Release);

    unsafe {
        NET_DEVICE = None;
        NET_STACK = None;
        DHCP_HANDLE = None;
    }
    SOCKETS_INITIALIZED.store(false, Ordering::Release);

    G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);

    drop(_guard);

    qx_net_init();

    crate::arch!(interrupt_enable());
    unsafe {
        klog_init_msg("--- Network Recovered ---\0".as_ptr().cast());
    }
}

unsafe extern "C" fn net_reset() {
    let _guard = NET_LOCK.lock();

    crate::kernel::net::types::NET_READY.store(false, Ordering::Release);
    crate::kernel::net::types::NET_CONFIGURED.store(false, Ordering::Release);

    unsafe {
        NET_DEVICE = None;
        NET_STACK = None;
        DHCP_HANDLE = None;
    }
    SOCKETS_INITIALIZED.store(false, Ordering::Release);

    G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);

    unsafe {
        klog_init_msg("--- Network Hard Reset ---\0".as_ptr().cast());
    }
}

// ============================================================================
// 网络子系统初始化入口
//
// Linux 风格: 内核只负责硬件探测与初始化, DHCP/IP 配置由用户态或
// timer ISR 异步完成。协议栈在硬件就绪后即可收发原始帧。
// ============================================================================

#[no_mangle]
pub extern "C" fn qx_net_init() {
    unsafe {
        klog_init_msg("--- Network Subsystem Init ---\0".as_ptr().cast());

        if transition_state(InitState::Uninitialized, InitState::HardwareProbed).is_err() {
            let current = G_INIT_STATE.load(Ordering::Acquire);
            if current == InitState::FullyInitialized as u8 {
                klog_net("Network already initialized\0".as_ptr().cast());
                return;
            } else if current == InitState::Failed as u8 {
                klog_net_err(
                    "Previous initialization failed, retrying...\0".as_ptr().cast(),
                );
                G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);
            } else {
                klog_net_err("Invalid init state, aborting\0".as_ptr().cast());
                return;
            }
            if transition_state(InitState::Uninitialized, InitState::HardwareProbed).is_err() {
                return;
            }
        }

        klog_net("Step1: hardware probe\0".as_ptr().cast());

        let mut nic = match nic_probe_all() {
            Some(n) => n,
            None => {
                let _ = transition_state(InitState::HardwareProbed, InitState::FullyInitialized);
                klog_net(
                    "No NIC found, running without network\0".as_ptr().cast(),
                );
                klog_init_msg(
                    "--- Network Subsystem Ready (No Network) ---\0".as_ptr() as *const i8,
                );
                return;
            }
        };

        klog_net("Step2: init device hardware\0".as_ptr().cast());

        let mac = nic.mac;
        let stack = smoltcp_impl::init_stack(&mut nic, mac);

        {
            let _guard = NET_LOCK.lock();
            NET_DEVICE = Some(nic);
            NET_STACK = Some(stack);
        }

        if transition_state(InitState::HardwareProbed, InitState::InterfaceReady).is_err() {
            set_failed();
            klog_net_err("Failed to transition to InterfaceReady\0".as_ptr().cast());
            return;
        }

        klog_net("Step3: init network interface\0".as_ptr().cast());

        {
            let _guard = NET_LOCK.lock();
            init_sockets();
            let mut sockets = &mut *socket_set();
            let dhcp_socket = dhcpv4::Socket::new();
            let handle = sockets.add(dhcp_socket);
            DHCP_HANDLE = Some(handle);
        }

        crate::kernel::net::types::NET_READY.store(true, Ordering::Release);

        if transition_state(InitState::InterfaceReady, InitState::FullyInitialized).is_err() {
            set_failed();
            klog_net_err("Failed to transition to FullyInitialized\0".as_ptr().cast());
            return;
        }

        klog_net("DHCP: boot poll...\0".as_ptr().cast());
        for _attempt in 0u32..500 {
            poll_network();
            for _ in 0..50000 {
                core::hint::spin_loop();
            }
            if crate::kernel::net::types::NET_CONFIGURED.load(Ordering::Acquire) {
                klog_net("DHCP: lease acquired\0".as_ptr().cast());
                break;
            }
        }

        if !crate::kernel::net::types::NET_CONFIGURED.load(Ordering::Acquire) {
            let cidr = IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(
                smoltcp::wire::Ipv4Address::new(10, 0, 2, 15),
                24,
            ));
            let _guard = NET_LOCK.lock();
            if let Some(stack) = NET_STACK.as_mut() {
                stack.iface.update_ip_addrs(|addrs| {
                    let _ = addrs.push(cidr);
                });
                let gw = smoltcp::wire::Ipv4Address::new(10, 0, 2, 2);
                let _ = stack.iface.routes_mut().add_default_ipv4_route(gw);
                crate::kernel::net::types::NET_CONFIGURED.store(true, Ordering::Release);
                klog_net("Static IP 10.0.2.15/24 (fallback)\0".as_ptr().cast());
            }
        }

        crate::arch!(interrupt_enable());

        klog_init_msg("--- Network Subsystem Ready ---\0".as_ptr().cast());

        crate::kernel::barrier::recovery::recovery_domain_register(
            "net",
            5,
            &[],
            net_save,
            net_restore,
            net_reset,
        );
    }
}

// ============================================================================
// 网络配置入口 (用户态或驱动层调用)
// ============================================================================

/// 启动 DHCP (异步, 由 timer ISR 驱动 poll 完成)
///
/// 调用后 DHCP Discover 会在下一个 timer tick 发出。
/// 用户态通过 poll/select 或轮询 NET_CONFIGURED 等待完成。
#[no_mangle]
pub unsafe extern "C" fn qx_net_start_dhcp() -> i32 {
    if !crate::kernel::net::types::NET_READY.load(Ordering::Acquire) {
        return -1;
    }
    poll_network();
    0
}

/// 设置静态 IP (x.x.x.x/prefix, gateway)
///
/// 格式: "10.0.2.15/24,10.0.2.2"
/// 返回 0 成功, -1 失败
#[no_mangle]
pub unsafe extern "C" fn qx_net_static_ip(cidr_str: *const u8, gw_str: *const u8) -> i32 {
    if !crate::kernel::net::types::NET_READY.load(Ordering::Acquire) {
        return -1;
    }

    let _guard = NET_LOCK.lock();

    let stack = match unsafe { NET_STACK.as_mut() } {
        Some(s) => s,
        None => return -1,
    };

    // 解析 CIDR 字符串 "a.b.c.d/prefix"
    let mut octets = [0u8; 4];
    let mut prefix = 24u8;
    let mut parsing_prefix = false;
    let mut octet_idx = 0usize;
    let mut current = 0u32;

    let mut ptr = cidr_str;
    loop {
        let b = *ptr;
        if b == 0 {
            if !parsing_prefix {
                octets[octet_idx] = current as u8;
            } else {
                prefix = current as u8;
            }
            break;
        }
        if b == b'/' {
            octets[octet_idx] = current as u8;
            parsing_prefix = true;
            current = 0;
        } else if b == b'.' && !parsing_prefix {
            octets[octet_idx] = current as u8;
            octet_idx += 1;
            if octet_idx >= 4 { return -1; }
            current = 0;
        } else if b >= b'0' && b <= b'9' {
            current = current * 10 + (b - b'0') as u32;
        } else {
            return -1;
        }
        ptr = ptr.add(1);
    }

    let ip = smoltcp::wire::Ipv4Address::new(octets[0], octets[1], octets[2], octets[3]);

    // 解析网关
    let mut gw_octets = [0u8; 4];
    let mut gw_idx = 0usize;
    let mut gw_current = 0u32;
    let mut gw_ptr = gw_str;
    loop {
        let b = *gw_ptr;
        if b == 0 {
            gw_octets[gw_idx] = gw_current as u8;
            break;
        }
        if b == b'.' {
            gw_octets[gw_idx] = gw_current as u8;
            gw_idx += 1;
            if gw_idx >= 4 { return -1; }
            gw_current = 0;
        } else if b >= b'0' && b <= b'9' {
            gw_current = gw_current * 10 + (b - b'0') as u32;
        } else {
            return -1;
        }
        gw_ptr = gw_ptr.add(1);
    }
    let gw = smoltcp::wire::Ipv4Address::new(gw_octets[0], gw_octets[1], gw_octets[2], gw_octets[3]);

    let cidr = IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(ip, prefix));
    stack.iface.update_ip_addrs(|addrs| {
        addrs.clear();
        let _ = addrs.push(cidr);
    });
    let _ = stack.iface.routes_mut().add_default_ipv4_route(gw);

    crate::kernel::net::types::NET_CONFIGURED.store(true, Ordering::Release);

    unsafe {
        klog_net("Static IP configured\0".as_ptr().cast());
    }
    0
}

// ============================================================================
// Socket 系统调用注册
// ============================================================================

#[no_mangle]
pub extern "C" fn qx_socket_register_syscalls() -> i32 {
    0
}

// ============================================================================
// Socket 公共 API
// ============================================================================

// POSIX errno constants (i32)
const E_PERM: i32 = 1;
const E_NOENT: i32 = 2;
const E_INTR: i32 = 4;
const E_IO: i32 = 5;
const E_BADF: i32 = 9;
const E_AGAIN: i32 = 11;
const E_NOMEM: i32 = 12;
const E_FAULT: i32 = 14;
const E_INVAL: i32 = 22;
const E_NFILE: i32 = 23;
const E_NOTSUPP: i32 = 95;
const E_AFNOSUPPORT: i32 = 97;
const E_ADDRINUSE: i32 = 98;
const E_ADDRNOTAVAIL: i32 = 99;
const E_CONNRESET: i32 = 104;
const E_NOTCONN: i32 = 107;
const E_CONNREFUSED: i32 = 111;
const E_NODEV: i32 = 19;

#[no_mangle]
pub unsafe extern "C" fn sm_socket(domain: i32, sock_type: i32, _protocol: i32) -> i32 {
    if !is_network_initialized() {
        return -E_NODEV;
    }

    let _guard = NET_LOCK.lock();

    let fd = sm_alloc_fd();
    if fd < 0 {
        return -E_NFILE;
    }
    let fd_idx = fd as usize;

    if domain == 2 && sock_type == 1 {
        let tcp_sock = tcp::Socket::new(
            tcp::SocketBuffer::new(&mut TCP_RX_BUFS[fd_idx][..]),
            tcp::SocketBuffer::new(&mut TCP_TX_BUFS[fd_idx][..]),
        );
        let mut sockets = &mut *socket_set();
        let handle = sockets.add(tcp_sock);
        SOCKET_TABLE[fd_idx] = Some(handle);
        FD_TYPES[fd_idx] = 1;
        fd
    } else if domain == 2 && sock_type == 2 {
        let udp_sock = udp::Socket::new(
            udp::PacketBuffer::new(
                &mut UDP_RX_METAS[fd_idx][..],
                &mut UDP_RX_BUFS[fd_idx][..],
            ),
            udp::PacketBuffer::new(
                &mut UDP_TX_METAS[fd_idx][..],
                &mut UDP_TX_BUFS[fd_idx][..],
            ),
        );
        let mut sockets = &mut *socket_set();
        let handle = sockets.add(udp_sock);
        SOCKET_TABLE[fd_idx] = Some(handle);
        FD_TYPES[fd_idx] = 2;
        fd
    } else {
        -E_AFNOSUPPORT
    }
}

#[repr(C)]
struct SockaddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

unsafe fn parse_ipv4_endpoint(addr: *const u8) -> Option<IpEndpoint> {
    if addr.is_null() {
        return None;
    }
    let sin = &*(addr as *const SockaddrIn);
    if sin.sin_family != 2 {
        return None;
    }
    let ip = Ipv4Address::new(
        sin.sin_addr[0],
        sin.sin_addr[1],
        sin.sin_addr[2],
        sin.sin_addr[3],
    );
    let port = u16::from_be(sin.sin_port);
    Some(IpEndpoint {
        addr: IpAddress::Ipv4(ip),
        port,
    })
}

#[no_mangle]
pub unsafe extern "C" fn sm_bind(fd: i32, addr: *const u8, _addrlen: u32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };

    let mut sockets = &mut *socket_set();

    match FD_TYPES[fd as usize] {
        2 => {
            let sock = sockets.get_mut::<udp::Socket>(handle);
            let endpoint = match parse_ipv4_endpoint(addr) {
                Some(ep) => IpListenEndpoint {
                    addr: Some(ep.addr),
                    port: ep.port,
                },
                None => return -E_INVAL,
            };
            match sock.bind(endpoint) {
                Ok(()) => 0,
                Err(_) => -E_ADDRINUSE,
            }
        }
        _ => -E_NOTSUPP,
    }
}

#[no_mangle]
pub unsafe extern "C" fn sm_listen(fd: i32, _backlog: i32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };

    if FD_TYPES[fd as usize] != 1 {
        return -E_NOTSUPP;
    }

    let mut sockets = &mut *socket_set();
    let sock = sockets.get_mut::<tcp::Socket>(handle);

    let local = IpListenEndpoint {
        addr: None,
        port: 0,
    };
    match sock.listen(local) {
        Ok(()) => 0,
        Err(_) => -E_ADDRINUSE,
    }
}

#[no_mangle]
pub unsafe extern "C" fn sm_accept(fd: i32, _addr: *mut u8, _addrlen: *mut u32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };

    if FD_TYPES[fd as usize] != 1 {
        return -E_NOTSUPP;
    }

    let mut sockets = &mut *socket_set();
    let sock = sockets.get_mut::<tcp::Socket>(handle);

    if sock.is_active() {
        fd
    } else {
        -E_AGAIN
    }
}

#[no_mangle]
pub unsafe extern "C" fn sm_connect(fd: i32, addr: *const u8, _addrlen: u32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };

    if !crate::kernel::net::types::NET_CONFIGURED.load(Ordering::Acquire) {
        return -E_NODEV;
    }

    let endpoint = match parse_ipv4_endpoint(addr) {
        Some(ep) => ep,
        None => return -E_INVAL,
    };

    if FD_TYPES[fd as usize] != 1 {
        return -E_NOTSUPP;
    }

    let stack = match NET_STACK.as_mut() {
        Some(s) => s,
        None => return -E_NODEV,
    };

    let mut sockets = &mut *socket_set();
    let sock = sockets.get_mut::<tcp::Socket>(handle);

    let local = IpListenEndpoint {
        addr: None,
        port: 0,
    };
    match sock.connect(stack.iface.context(), endpoint, local) {
        Ok(()) => 0,
        Err(_) => -E_CONNREFUSED,
    }
}

#[no_mangle]
pub unsafe extern "C" fn sm_send(fd: i32, buf: *const u8, len: u32, _flags: i32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };
    if buf.is_null() || len == 0 {
        return -E_INVAL;
    }

    let mut sockets = &mut *socket_set();
    let data = core::slice::from_raw_parts(buf, len as usize);

    match FD_TYPES[fd as usize] {
        1 => {
            let sock = sockets.get_mut::<tcp::Socket>(handle);
            match sock.send_slice(data) {
                Ok(n) => n as i32,
                Err(_) => -E_CONNRESET,
            }
        }
        2 => {
            // UDP without destination: depends on socket being "connected" (bound via endpoint)
            // For simplicity, return ENOTCONN; use sendto instead
            -E_NOTCONN
        }
        _ => -E_NOTSUPP,
    }
}

#[no_mangle]
pub unsafe extern "C" fn sm_recv(fd: i32, buf: *mut u8, len: u32, _flags: i32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };
    if buf.is_null() || len == 0 {
        return -E_INVAL;
    }

    let mut sockets = &mut *socket_set();
    let data = core::slice::from_raw_parts_mut(buf, len as usize);

    match FD_TYPES[fd as usize] {
        1 => {
            let sock = sockets.get_mut::<tcp::Socket>(handle);
            match sock.recv_slice(data) {
                Ok(n) => n as i32,
                Err(_) => {
                    if sock.is_open() {
                        0
                    } else {
                        -E_CONNRESET
                    }
                }
            }
        }
        2 => {
            let sock = sockets.get_mut::<udp::Socket>(handle);
            match sock.recv_slice(data) {
                Ok((n, _meta)) => n as i32,
                Err(_) => -E_AGAIN,
            }
        }
        _ => -E_NOTSUPP,
    }
}

#[no_mangle]
pub unsafe extern "C" fn sm_sendto(
    fd: i32,
    buf: *const u8,
    len: u32,
    _flags: i32,
    addr: *const u8,
    _addrlen: u32,
) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };
    if buf.is_null() || len == 0 {
        return -E_INVAL;
    }

    let endpoint = match parse_ipv4_endpoint(addr) {
        Some(ep) => ep,
        None => return -E_INVAL,
    };

    let mut sockets = &mut *socket_set();
    let data = core::slice::from_raw_parts(buf, len as usize);

    match FD_TYPES[fd as usize] {
        2 => {
            let sock = sockets.get_mut::<udp::Socket>(handle);
            match sock.send_slice(data, endpoint) {
                Ok(()) => len as i32,
                Err(_) => -E_CONNRESET,
            }
        }
        1 => {
            let sock = sockets.get_mut::<tcp::Socket>(handle);
            match sock.send_slice(data) {
                Ok(n) => n as i32,
                Err(_) => -E_CONNRESET,
            }
        }
        _ => -E_NOTSUPP,
    }
}

#[no_mangle]
pub unsafe extern "C" fn sm_recvfrom(
    fd: i32,
    buf: *mut u8,
    len: u32,
    _flags: i32,
    _addr: *mut u8,
    _addrlen: *mut u32,
) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };
    if buf.is_null() || len == 0 {
        return -E_INVAL;
    }

    let mut sockets = &mut *socket_set();
    let data = core::slice::from_raw_parts_mut(buf, len as usize);

    match FD_TYPES[fd as usize] {
        2 => {
            let sock = sockets.get_mut::<udp::Socket>(handle);
            match sock.recv_slice(data) {
                Ok((n, _meta)) => n as i32,
                Err(_) => -E_AGAIN,
            }
        }
        1 => {
            let sock = sockets.get_mut::<tcp::Socket>(handle);
            match sock.recv_slice(data) {
                Ok(n) => n as i32,
                Err(_) => {
                    if sock.is_open() {
                        0
                    } else {
                        -E_CONNRESET
                    }
                }
            }
        }
        _ => -E_NOTSUPP,
    }
}

#[no_mangle]
pub unsafe extern "C" fn sm_close(fd: i32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };

    let stype = FD_TYPES[fd as usize];
    let mut sockets = &mut *socket_set();

    match stype {
        1 => {
            let sock = sockets.get_mut::<tcp::Socket>(handle);
            sock.close();
        }
        2 => {
            let sock = sockets.get_mut::<udp::Socket>(handle);
            sock.close();
        }
        _ => {}
    }

    sockets.remove(handle);
    SOCKET_TABLE[fd as usize] = None;
    FD_TYPES[fd as usize] = 0;
    0
}

#[no_mangle]
pub unsafe extern "C" fn sm_setsockopt(
    _fd: i32,
    _level: i32,
    _optname: i32,
    _optval: *const u8,
    _optlen: u32,
) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn sm_getsockopt(
    _fd: i32,
    _level: i32,
    _optname: i32,
    _optval: *mut u8,
    _optlen: *mut u32,
) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn sm_poll_sockets() -> i32 {
    let _guard = NET_LOCK.lock();

    let mut sockets = &mut *socket_set();
    process_dhcp_events(&mut sockets);

    for i in 0..MAX_SM_FD {
        if FD_TYPES[i] != 1 {
            continue;
        }
        if let Some(handle) = SOCKET_TABLE[i] {
            let _sock = sockets.get_mut::<tcp::Socket>(handle);
        }
    }
    0
}

// ============================================================================
// 公共 API
// ============================================================================

const MAX_SM_FD: usize = 16;
const TCP_BUF_SIZE: usize = 4096;
const UDP_BUF_SIZE: usize = 2048;
const UDP_META_COUNT: usize = 4;

static mut SOCKET_TABLE: [Option<SocketHandle>; MAX_SM_FD] = [None; MAX_SM_FD];

// Per-fd 类型标记: 0=free, 1=tcp, 2=udp
static mut FD_TYPES: [u8; MAX_SM_FD] = [0u8; MAX_SM_FD];

// TCP buffer storage (per fd)
static mut TCP_RX_BUFS: [[u8; TCP_BUF_SIZE]; MAX_SM_FD] = [[0u8; TCP_BUF_SIZE]; MAX_SM_FD];
static mut TCP_TX_BUFS: [[u8; TCP_BUF_SIZE]; MAX_SM_FD] = [[0u8; TCP_BUF_SIZE]; MAX_SM_FD];

// UDP buffer storage (per fd)
static mut UDP_RX_METAS: [[udp::PacketMetadata; UDP_META_COUNT]; MAX_SM_FD] =
    [[udp::PacketMetadata::EMPTY; UDP_META_COUNT]; MAX_SM_FD];
static mut UDP_RX_BUFS: [[u8; UDP_BUF_SIZE]; MAX_SM_FD] = [[0u8; UDP_BUF_SIZE]; MAX_SM_FD];
static mut UDP_TX_METAS: [[udp::PacketMetadata; UDP_META_COUNT]; MAX_SM_FD] =
    [[udp::PacketMetadata::EMPTY; UDP_META_COUNT]; MAX_SM_FD];
static mut UDP_TX_BUFS: [[u8; UDP_BUF_SIZE]; MAX_SM_FD] = [[0u8; UDP_BUF_SIZE]; MAX_SM_FD];

unsafe fn sm_alloc_fd() -> i32 {
    for i in 0..MAX_SM_FD {
        if FD_TYPES[i] == 0 && SOCKET_TABLE[i].is_none() {
            return i as i32;
        }
    }
    -1
}

pub fn is_network_initialized() -> bool {
    crate::kernel::net::types::NET_READY.load(Ordering::Acquire)
}

pub fn is_network_configured() -> bool {
    crate::kernel::net::types::NET_CONFIGURED.load(Ordering::Acquire)
}

pub fn get_init_state() -> InitState {
    match G_INIT_STATE.load(Ordering::Acquire) {
        0 => InitState::Uninitialized,
        1 => InitState::HardwareProbed,
        2 => InitState::InterfaceReady,
        3 => InitState::FullyInitialized,
        _ => InitState::Failed,
    }
}

pub unsafe fn reset_network_state() {
    let _guard = NET_LOCK.lock();

    G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);

    unsafe {
        NET_DEVICE = None;
        NET_STACK = None;
        DHCP_HANDLE = None;
    }
    SOCKETS_INITIALIZED.store(false, Ordering::Release);
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialization_state_machine() {
        assert_eq!(get_init_state(), InitState::Uninitialized);
        assert!(!is_network_initialized());

        unsafe {
            reset_network_state();
        }
        assert_eq!(get_init_state(), InitState::Uninitialized);
    }
}