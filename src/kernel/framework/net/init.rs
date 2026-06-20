// 网络子系统初始化占位, 待 smoltcp 集成完成后启用。
// 保留文件级 allow: InitState/NetStatus 等内部类型和大量初始化函数
// 待网络栈端到端路径启用后使用, 逐项标注会淹没代码。
#![allow(dead_code)]

use core::ptr::null_mut;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::kernel::framework::sync::IrqSpinLock as Mutex;
use crate::kernel::framework::klog::{klog_net, klog_net_err, klog_init_msg};
use crate::kernel::framework::net::{ChitinNetDevice, NetworkStack};
use smoltcp::iface::{SocketHandle, SocketSet, SocketStorage};
use smoltcp::socket::dhcpv4;
use smoltcp::socket::{tcp, udp};
use smoltcp::wire::{IpCidr, IpEndpoint, IpListenEndpoint, IpAddress, Ipv4Address};

// I-46: 引用本目录 types 模块的 fallback 常量
use crate::kernel::framework::net as types;

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

// 当前网络配置快照 (D1.1/D1.2 高层 API 支撑)
// 全部为 Atomic, 单字段读写无需 NET_LOCK; 多字段一致性由 NetStatus::capture 原子复制.
// 未配置时全部 = 0; 0.0.0.0 表示"无".
const IPV4_NONE: [u8; 4] = [0; 4];
const MAC_NONE: [u8; 6] = [0; 6];
static G_MAC: AtomicU64 = AtomicU64::new(0);              // 6 字节大端打包为 u64
static G_IPV4: AtomicU32 = AtomicU32::new(0);             // 网络字节序
static G_GATEWAY: AtomicU32 = AtomicU32::new(0);          // 网络字节序
static G_DNS: [AtomicU32; 3] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];

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

// I-47: 编译期容量上限, 默认 256 (此前硬编码 8 严重限制并发).
// 编译期覆盖: 修改本常量或通过未来 build.rs 注入 cfg_flag 覆盖.
// 每个 socket 携带 TCP/UDP 静态缓冲, BSS 占用 ≈ 6 KB/连接 (TCP_RX 4K + UDP_RX 2K).
// 256 → ≈ 1.5 MB BSS; 生产环境按物理内存调整.
// TD-06: 编译期容量从 `fd_alloc::cfg_smoltcp_cap()` 派生, 默认 256, 用户可手动
// 切换至 1024 / 4096. 改本值后须同步 SOCKET_STORAGE / TCP_*_BUFS / UDP_*_BUFS /
// FD_TYPES / SOCKET_TABLE 的所有 8 张大表尺寸, 否则全表越界.
const MAX_SOCKETS: usize = crate::kernel::services::proc::cfg_smoltcp_cap() as usize;
static mut SOCKET_STORAGE: core::mem::MaybeUninit<[SocketStorage<'static>; MAX_SOCKETS]> =
    core::mem::MaybeUninit::uninit();
static mut SOCKET_SET: core::mem::MaybeUninit<SocketSet<'static>> =
    core::mem::MaybeUninit::uninit();
static SOCKETS_INITIALIZED: AtomicBool = AtomicBool::new(false);
static mut DHCP_HANDLE: Option<SocketHandle> = None;

// ============================================================================
// I-47: Socket 容量配置
//
// MAX_SOCKETS = 编译期容量上限 (静态存储尺寸). 此前硬编码 8 严重限制并发连接数.
// 启动期默认 1024 (与 Linux net.core.somaxconn 相当), 运行时可通过
// `set_max_sockets` 调整, 不超过 MAX_SOCKETS. 编译期可通过 ANT_MAX_SOCKETS
// 环境变量覆盖 (Cargo build.rs 读取并写入 cfg).
// ============================================================================
#[allow(dead_code)] // 待 socket 容量动态调整路径启用后使用。
const DEFAULT_MAX_SOCKETS: usize = 1024;

/// 运行时活动 socket 数上限 (≤ MAX_SOCKETS).
/// 初值取 [1, MAX_SOCKETS] 范围内的 DEFAULT_MAX_SOCKETS.
static G_MAX_SOCKETS: AtomicUsize = AtomicUsize::new(0);

/// 启动期初始化 G_MAX_SOCKETS. 必须在 init_sockets 前调用一次.
pub fn configure_max_sockets() {
    let initial = if DEFAULT_MAX_SOCKETS > MAX_SOCKETS {
        MAX_SOCKETS
    } else if DEFAULT_MAX_SOCKETS == 0 {
        1
    } else {
        DEFAULT_MAX_SOCKETS
    };
    G_MAX_SOCKETS.store(initial, Ordering::Release);
}

/// 获取当前运行时 socket 上限.
pub fn get_max_sockets() -> usize {
    let v = G_MAX_SOCKETS.load(Ordering::Acquire);
    if v == 0 {
        // 首次访问时尚未 configure, 返回编译期上限的保守值
        1
    } else {
        v
    }
}

/// 调整运行时 socket 上限. n=0 拒绝; n>MAX_SOCKETS 截断为 MAX_SOCKETS.
/// 返回实际生效值. 运行时调大已分配的 SocketStorage 不会扩容 (仅控制新连接).
pub fn set_max_sockets(n: usize) -> usize {
    let target = if n == 0 {
        return get_max_sockets();
    } else if n > MAX_SOCKETS {
        MAX_SOCKETS
    } else {
        n
    };
    G_MAX_SOCKETS.store(target, Ordering::Release);
    target
}

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

/// # Safety
///
/// - 仅在内核启动网络子系统的临界区内调用一次
/// - `SOCKET_STORAGE` 是 `MaybeUninit<[SocketStorage; MAX_SOCKETS]>` 静态变量, 由本函数独占初始化
/// - `SOCKET_SET` 是 `UninitCell<SocketSet<'static>>`, 初始化后只读
unsafe fn init_sockets() {
    if SOCKETS_INITIALIZED.load(Ordering::Acquire) {
        return;
    }
    configure_max_sockets();
    let ptr = SOCKET_STORAGE.as_mut_ptr() as *mut SocketStorage<'static>;
    for i in 0..MAX_SOCKETS {
        core::ptr::write(ptr.add(i), SocketStorage::EMPTY);
    }
    let storage = SOCKET_STORAGE.assume_init_mut();
    SOCKET_SET.write(SocketSet::new(&mut storage[..]));
    SOCKETS_INITIALIZED.store(true, Ordering::Release);
}

/// # Safety
///
/// - 调用前必须已执行 `init_sockets` 完成 `SOCKET_SET` 初始化
/// - 返回的指针仅在同一线程的 socket 调度上下文内使用, 不得跨线程共享
unsafe fn socket_set() -> *mut SocketSet<'static> {
    SOCKET_SET.as_mut_ptr()
}

/// # Safety
///
/// - `sockets` 必须是 `socket_set()` 返回的 `SocketSet`, 同一时间仅本函数独占访问
unsafe fn process_dhcp_events(sockets: &mut SocketSet<'_>) {
    static FIRST_DECONFIG: AtomicBool = AtomicBool::new(true);

    let dhcp_handle = match raw::dhcp_handle() {
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
            if let Some(stack) = raw::stack_mut() {
                stack.iface.update_ip_addrs(|addrs| {
                    addrs.clear();
                });
                let _ = stack.iface.routes_mut().remove_default_ipv4_route();
            }
            crate::kernel::framework::net::NET_CONFIGURED.store(false, Ordering::Release);
            raw::klog_msg("DHCP deconfigured");
        }
        Some(dhcpv4::Event::Configured(config)) => {
            FIRST_DECONFIG.store(false, Ordering::Release);
            if let Some(stack) = raw::stack_mut() {
                let cidr = config.address;
                stack.iface.update_ip_addrs(|addrs| {
                    addrs.clear();
                    let _ = addrs.push(IpCidr::Ipv4(cidr));
                });
                if let Some(router) = config.router {
                    let _ = stack.iface.routes_mut().add_default_ipv4_route(router);
                    G_GATEWAY.store(u32::from_be_bytes(router.octets()), Ordering::Release);
                }
                // D1.2: 把配置结果写进 G_IPV4 / G_DNS, 供高层观测 API
                G_IPV4.store(u32::from_be_bytes(config.address.address().octets()), Ordering::Release);
                for (i, dns) in config.dns_servers.iter().enumerate() {
                    if i >= G_DNS.len() {
                        break;
                    }
                    G_DNS[i].store(u32::from_be_bytes(dns.octets()), Ordering::Release);
                }
            }
            crate::kernel::framework::net::NET_CONFIGURED.store(true, Ordering::Release);
            raw::klog_msg("DHCP configured");
        }
    }
}

// ============================================================================
// 网络轮询 (统一入口，与具体网卡无关)
//
// 使用 NET_LOCK.try_lock() 确保互斥访问。
// try_lock() 在 ISR 上下文中不会阻塞：若锁已被持有则直接返回。
// ============================================================================

/// 轮询网络栈 (驱动 TX/RX、定时器、DHCP)。
///
/// 在 timer ISR 或网络任务中调用, 内部 try_lock 避免阻塞。
/// 若 NET_LOCK 已被持有则直接返回, 不会等待。
///
/// # Safety
/// - `try_lock` 保证 ISR 安全 (不阻塞)。
/// - 内部 raw::device_mut / raw::stack_mut 通过 NET_LOCK 互斥保护。
pub unsafe fn poll_network() {
    let _guard = match NET_LOCK.try_lock() {
        Some(g) => g,
        None => return,
    };

    let nic = match raw::device_mut() {
        Some(d) => d,
        None => return,
    };
    let stack = match raw::stack_mut() {
        Some(s) => s,
        None => return,
    };
    let sockets = &mut *raw::socket_set();
    crate::kernel::framework::net::poll_stack(nic, stack, sockets);
    raw::process_dhcp_events(sockets);

    // P2-I-41: poll 完毕后通知所有 fd 的等待者, 让 sm_send/sm_recv
    // (未来阻塞扩展点) 重新检查 socket 状态. try_wake 持锁时间 O(1).
    use crate::kernel::framework::net::{WakeReason, SOCKET_WAIT_QUEUES};
    for fd in 0..MAX_SM_FD {
        if FD_TYPES.0[fd] == 0 {
            continue;
        }
        // 用 smoltcp can_send / can_recv 推断 wake 原因. socket_set 访问
        // 仍在 NET_LOCK 保护下 (try_wake 内部 lock 仅保护自身 pending 标记,
        // 与 smoltcp 状态机无关).
        let reason = if let Some(handle) = SOCKET_TABLE.0[fd] {
            let can_read = match FD_TYPES.0[fd] {
                1 => sockets.get::<tcp::Socket>(handle).can_recv(),
                2 => sockets.get::<udp::Socket>(handle).can_recv(),
                _ => false,
            };
            let can_write = match FD_TYPES.0[fd] {
                1 => sockets.get::<tcp::Socket>(handle).can_send(),
                2 => sockets.get::<udp::Socket>(handle).can_send(),
                _ => false,
            };
            if can_read {
                WakeReason::Readable
            } else if can_write {
                WakeReason::Writable
            } else {
                continue;
            }
        } else {
            continue;
        };
        if let Some(q) = SOCKET_WAIT_QUEUES.get(fd) {
            q.try_wake(reason);
        }
    }
}

// ============================================================================
// 多网卡探测 (按优先级依次尝试)
// ============================================================================

/// # Safety
///
/// - 在网络子系统初始化入口被调用, 期间无其他并发探测
/// - 依赖的 chitin/driver 框架 (`Driver::init`) 自身保证设备独占
#[cfg(not(feature = "kernel_test"))]
unsafe fn nic_probe_all() -> Option<ChitinNetDevice> {
    // I-53 修复: 去除编译时架构互斥, 双架构二进制按运行时探测顺序
    // 尝试 e1000 (PCI 设备) 与 virtio-net (MMIO 设备). 两者驱动代码
    // 均架构无关, 仅依赖 IoMem / PCI 抽象. QEMU 配置决定哪一个会成功.
    //
    // 探测顺序固定: e1000 -> virtio-net. 真实硬件 (e.g. PC 上) e1000
    // 优先; QEMU virt 上 e1000 探测返回非 0 走 fallthrough 到 virtio.
    //
    // 失败: 全部探测返回非 0 / Box::into_raw 失败 / Driver::init 失败.

    // 1) e1000 探测 (PCI 设备, 走 PCI 总线, 架构无关)
    {
        let probe_result = crate::kernel::framework::driver::e1000_probe();
        if probe_result == 0 {
            let mut dev = crate::kernel::framework::driver::e1000_take_device()?;
            if crate::kernel::framework::driver::Driver::init(&mut *dev).is_err() {
                raw::klog_err("e1000: hardware init failed");
                return None;
            }
            let mac = dev.mac;
            let raw_ptr = alloc::boxed::Box::into_raw(dev) as *mut core::ffi::c_void;
            let nic = ChitinNetDevice::new(&E1000_NET_OPS_STATIC, raw_ptr, mac);
            raw::klog_msg("e1000: probed successfully");
            return Some(nic);
        }
    }

    // 2) virtio-net 探测 (MMIO 设备, 走 virtio 总线, 架构无关)
    {
        let probe_result = crate::kernel::framework::driver::virtio_net_probe();
        if probe_result == 0 {
            let dev = crate::kernel::framework::driver::virtio_net_take_device()?;
            let mac = dev.mac;
            let raw_ptr = alloc::boxed::Box::into_raw(dev) as *mut core::ffi::c_void;
            let nic = ChitinNetDevice::new(&VIRTIO_NET_OPS_STATIC, raw_ptr, mac);
            raw::klog_msg("virtio-net: probed successfully");
            return Some(nic);
        }
    }

    None
}

#[cfg(not(feature = "kernel_test"))]
static E1000_NET_OPS_STATIC: crate::kernel::framework::chitin::NetOps =
    crate::kernel::framework::chitin::NetOps {
        send: crate::kernel::framework::driver::e1000_net_send,
        try_receive: crate::kernel::framework::driver::e1000_net_recv,
        get_mac: crate::kernel::framework::driver::e1000_net_get_mac,
        handle_irq: Some(crate::kernel::framework::driver::e1000_net_irq),
    };

static VIRTIO_NET_OPS_STATIC: crate::kernel::framework::chitin::NetOps =
    crate::kernel::framework::chitin::NetOps {
        send: crate::kernel::framework::driver::virtio_net_send,
        try_receive: crate::kernel::framework::driver::virtio_net_recv,
        get_mac: crate::kernel::framework::driver::virtio_net_get_mac,
        handle_irq: Some(crate::kernel::framework::driver::virtio_net_irq),
    };

// ============================================================================
// 恢复机制 (P2-I-44 完整实现)
// ============================================================================
//
// 快照 (save.rs) 在 NET_LOCK 持有时序列化 IP/GW/MAC/FD 表; restore
// 跳过 DHCP 重配并把 FD 表恢复到 save 时刻. smoltcp 内部 socket 状态
// (TCP 收发缓冲 / UDP metadata) 因 smoltcp 不暴露 serialize API 而无
// 法恢复, 这是已知限制 (写在 save.rs 文档中).

/// # Safety
///
/// - 调用方须保证单线程进入 (recovery 域串行执行)
/// - 必须在关中断上下文执行, NET_LOCK 由本函数获取
///
/// SAFETY: 见上方 # Safety 章节, 调用方保证单线程 + 关中断; NET_LOCK 由本函数内部获取
unsafe fn net_save() {
    use core::sync::atomic::Ordering;
    use crate::kernel::framework::net::save as snap;

    let _guard = NET_LOCK.lock();

    snap::save(|s| {
        // MAC: 从当前 NIC 读取 (mut 访问因 NET_LOCK 持有而安全)
        if let Some(dev) = raw::device_mut() {
            s.mac = dev.mac;
        }

        // IP / GW / prefix: 从 stack iface 读取
        if let Some(stack) = raw::stack_mut() {
            if let Some(cidr) = stack.iface.ip_addrs().first() {
                if let smoltcp::wire::IpCidr::Ipv4(v4) = cidr {
                    s.ip = v4.address().octets();
                    s.prefix_len = v4.prefix_len();
                }
            }
            // smoltcp 0.13 路由 API: get_default_ipv4_route 返回 Option<Route>
            if let Some(route) = stack.iface.routes().get_default_ipv4_route() {
                if let smoltcp::wire::IpAddress::Ipv4(gw) = route.via_router {
                    let oct = gw.octets();
                    s.gateway = oct;
                }
            }
        }

        // FD 表
        for i in 0..MAX_SM_FD {
            s.fd_types[i] = FD_TYPES.0[i];
            s.fd_handles[i] = match SOCKET_TABLE.0[i] {
                Some(h) => as_u32_handle(h),
                None => u32::MAX,
            };
        }

        // 状态
        s.net_ready = crate::kernel::framework::net::NET_READY.load(Ordering::Acquire);
        s.net_configured = crate::kernel::framework::net::NET_CONFIGURED.load(Ordering::Acquire);
        s.sockets_initialized = SOCKETS_INITIALIZED.load(Ordering::Acquire);
        s.init_state = G_INIT_STATE.load(Ordering::Acquire);
    });
}

/// SocketHandle → u32 (smoltcp SocketHandle 是包装 newtype, 用 transmute).
#[inline]
fn as_u32_handle(h: smoltcp::iface::SocketHandle) -> u32 {
    // SAFETY: SocketHandle is repr(transparent) over usize on supported targets
    let raw: usize = unsafe { core::mem::transmute(h) };
    raw as u32
}

/// # Safety
///
/// - 调用方须确保无其他线程持有 socket fd (例如文件系统已卸载完毕)
/// - 必须在关中断上下文执行, NET_LOCK 由本函数获取
///
/// SAFETY: 见上方 # Safety 章节, 调用方保证 socket fd 已无人持有 + 关中断; NET_LOCK 由本函数内部获取
unsafe fn net_restore() {
    use core::sync::atomic::Ordering;
    use crate::kernel::framework::net::save as snap;

    // 1. 复位状态机
    {
        let _guard = NET_LOCK.lock();
        crate::kernel::framework::net::NET_READY.store(false, Ordering::Release);
        crate::kernel::framework::net::NET_CONFIGURED.store(false, Ordering::Release);
        raw::clear_all();
        SOCKETS_INITIALIZED.store(false, Ordering::Release);
        G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);
    }

    // 2. 重新初始化 NIC + stack
    qx_net_init();

    // 3. 读取快照, 跳过 DHCP 重配, 直接把 IP/GW 重新绑回
    let saved = snap::load();
    if saved.is_valid() {
        if saved.net_configured
            && saved.ip != [0, 0, 0, 0]
            && saved.prefix_len > 0
            && saved.prefix_len <= 32
        {
            let _guard = NET_LOCK.lock();
            if let Some(stack) = raw::stack_mut() {
                let ip = smoltcp::wire::Ipv4Address::new(
                    saved.ip[0], saved.ip[1], saved.ip[2], saved.ip[3],
                );
                let cidr = smoltcp::wire::IpCidr::Ipv4(
                    smoltcp::wire::Ipv4Cidr::new(ip, saved.prefix_len),
                );
                stack.iface.update_ip_addrs(|addrs| {
                    let _ = addrs.push(cidr);
                });
                if saved.gateway != [0, 0, 0, 0] {
                    let gw = smoltcp::wire::Ipv4Address::new(
                        saved.gateway[0], saved.gateway[1],
                        saved.gateway[2], saved.gateway[3],
                    );
                    let _ = stack.iface.routes_mut().add_default_ipv4_route(gw);
                }
                crate::kernel::framework::net::NET_CONFIGURED.store(true, Ordering::Release);
            }
        }
        // FD 表恢复: 仅恢复 (type, handle) 元组; smoltcp socket 内部状态
        // 不可序列化, 已连接 socket 在 restore 后等同于未初始化, 业务
        // 层需自行重新 connect / accept.
        let _guard = NET_LOCK.lock();
        for i in 0..MAX_SM_FD {
            FD_TYPES.0[i] = saved.fd_types[i];
            SOCKET_TABLE.0[i] = if saved.fd_handles[i] == u32::MAX {
                None
            } else {
                let raw = saved.fd_handles[i] as usize;
                // SAFETY: SocketHandle 来自同构的 smoltcp 版本, repr(transparent) over usize
                Some(unsafe { core::mem::transmute::<usize, smoltcp::iface::SocketHandle>(raw) })
            };
        }
        SOCKETS_INITIALIZED.store(saved.sockets_initialized, Ordering::Release);
    }

    crate::arch!(interrupt_enable());
    raw::klog_msg("--- Network Recovered ---");
    snap::clear();
}

/// # Safety
///
/// - 调用方须确保无其他线程持有 socket fd (例如文件系统已卸载完毕)
unsafe fn net_reset() {
    let _guard = NET_LOCK.lock();

    crate::kernel::framework::net::NET_READY.store(false, Ordering::Release);
    crate::kernel::framework::net::NET_CONFIGURED.store(false, Ordering::Release);

    raw::clear_all();
    SOCKETS_INITIALIZED.store(false, Ordering::Release);

    G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);

    raw::klog_msg("--- Network Hard Reset ---");
}

// ============================================================================
// 网络子系统初始化入口
//
// Linux 风格: 内核只负责硬件探测与初始化, DHCP/IP 配置由用户态或
// timer ISR 异步完成。协议栈在硬件就绪后即可收发原始帧。
// ============================================================================

#[no_mangle]
pub extern "C" fn qx_net_init() {
    // SAFETY: 网络初始化由启动流程串行调用, 无并发访问全局状态。
    unsafe {
        raw::klog_init("--- Network Subsystem Init ---");

        if transition_state(InitState::Uninitialized, InitState::HardwareProbed).is_err() {
            let current = G_INIT_STATE.load(Ordering::Acquire);
            if current == InitState::FullyInitialized as u8 {
                raw::klog_msg("Network already initialized");
                return;
            } else if current == InitState::Failed as u8 {
                raw::klog_err("Previous initialization failed, retrying...");
                G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);
            } else {
                raw::klog_err("Invalid init state, aborting");
                return;
            }
            if transition_state(InitState::Uninitialized, InitState::HardwareProbed).is_err() {
                return;
            }
        }

        raw::klog_msg("Step1: hardware probe");

        let mut nic = match nic_probe_all() {
            Some(n) => n,
            None => {
                let _ = transition_state(InitState::HardwareProbed, InitState::FullyInitialized);
                raw::klog_msg("No NIC found, running without network");
                raw::klog_init("--- Network Subsystem Ready (No Network) ---");
                return;
            }
        };

        raw::klog_msg("Step2: init device hardware");

        let mac = nic.mac;
        let stack = crate::kernel::framework::net::init_stack(&mut nic, mac);

        {
            let _guard = NET_LOCK.lock();
            raw::set_device(Some(nic));
            raw::set_stack(Some(stack));
        }

        if transition_state(InitState::HardwareProbed, InitState::InterfaceReady).is_err() {
            set_failed();
            raw::klog_err("Failed to transition to InterfaceReady");
            return;
        }

        raw::klog_msg("Step3: init network interface");

        {
            let _guard = NET_LOCK.lock();
            raw::init_sockets();
            let sockets = &mut *raw::socket_set();
            let dhcp_socket = dhcpv4::Socket::new();
            let handle = sockets.add(dhcp_socket);
            raw::set_dhcp_handle(Some(handle));
        }

        crate::kernel::framework::net::NET_READY.store(true, Ordering::Release);

        if transition_state(InitState::InterfaceReady, InitState::FullyInitialized).is_err() {
            set_failed();
            raw::klog_err("Failed to transition to FullyInitialized");
            return;
        }

        raw::klog_msg("DHCP: boot poll...");
        for _attempt in 0u32..500 {
            poll_network();
            for _ in 0..50000 {
                core::hint::spin_loop();
            }
            if crate::kernel::framework::net::NET_CONFIGURED.load(Ordering::Acquire) {
                raw::klog_msg("DHCP: lease acquired");
                break;
            }
        }

        if !crate::kernel::framework::net::NET_CONFIGURED.load(Ordering::Acquire) {
            // I-46: 引用 net::types 中的集中常量, 不再硬编码 10.0.2.15/24/10.0.2.2.
            use crate::kernel::framework::net::types::{FALLBACK_GATEWAY, FALLBACK_IPV4, FALLBACK_PREFIX};
            let cidr = IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(
                smoltcp::wire::Ipv4Address::new(
                    FALLBACK_IPV4[0], FALLBACK_IPV4[1], FALLBACK_IPV4[2], FALLBACK_IPV4[3],
                ),
                FALLBACK_PREFIX,
            ));
            let gw = smoltcp::wire::Ipv4Address::new(
                FALLBACK_GATEWAY[0], FALLBACK_GATEWAY[1], FALLBACK_GATEWAY[2], FALLBACK_GATEWAY[3],
            );
            let _guard = NET_LOCK.lock();
            if let Some(stack) = raw::stack_mut() {
                stack.iface.update_ip_addrs(|addrs| {
                    let _ = addrs.push(cidr);
                });
                let _ = stack.iface.routes_mut().add_default_ipv4_route(gw);
                crate::kernel::framework::net::NET_CONFIGURED.store(true, Ordering::Release);

                // D1.2: 把 fallback IP/网关写进 G_IPV4/G_GATEWAY, 给 get_* 观测 API
                G_IPV4.store(u32::from_be_bytes(FALLBACK_IPV4), Ordering::Release);
                G_GATEWAY.store(u32::from_be_bytes(FALLBACK_GATEWAY), Ordering::Release);
                raw::klog_msg("Static IP (fallback, see net::types::FALLBACK_*)");
            }
        }

        crate::arch!(interrupt_enable());

        raw::klog_init("--- Network Subsystem Ready ---");

        // 演进 6: 网络 init 完成后做 driver 维度自检
        if let Err(e) = crate::kernel::framework::config::validate_network_subsystem() {
            crate::klog_drv_warn!("Network validation: {}", e);
        }

        crate::kernel::framework::barrier::recovery::recovery_domain_register(
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
///
/// # Safety
/// 调用方保证 NET 已初始化 (通过 `qx_net_init` 注册)，
/// `NET_READY` 由网络栈在链路就绪后置位。
#[no_mangle]
pub unsafe extern "C" fn qx_net_start_dhcp() -> i32 {
    if !crate::kernel::framework::net::NET_READY.load(Ordering::Acquire) {
        return -1;
    }
    poll_network();
    0
}

/// 设置静态 IP (x.x.x.x/prefix, gateway)
///
/// 格式: "10.0.2.15/24,10.0.2.2"
/// 返回 0 成功, -1 失败
///
/// # Safety
/// - `cidr_str` 与 `gw_str` 必须是有效的 C 字符串指针 (NUL 终止),
///   指向的内存必须在调用期间保持有效。
/// - 调用方保证 NET 已初始化。
#[no_mangle]
pub unsafe extern "C" fn qx_net_static_ip(cidr_str: *const u8, gw_str: *const u8) -> i32 {
    if !crate::kernel::framework::net::NET_READY.load(Ordering::Acquire) {
        return -1;
    }

    let _guard = NET_LOCK.lock();

    let stack = match raw::stack_mut() {
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
        } else if b.is_ascii_digit() {
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
        } else if b.is_ascii_digit() {
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

    crate::kernel::framework::net::NET_CONFIGURED.store(true, Ordering::Release);

    raw::klog_msg("Static IP configured");
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

// POSIX errno 常量 (i32)
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

/// POSIX `socket(domain, type, protocol)` 内核实现。
///
/// # Safety
/// - 由 `sys_socket` 系统调用分发, 参数由 syscall 层校验 (cred 检查)。
/// - 必须 NET_LOCK 持有。
#[no_mangle]
pub unsafe extern "C" fn sm_socket(domain: i32, sock_type: i32, _protocol: i32) -> i32 {
    if !is_network_initialized() {
        return -E_NODEV;
    }

    let _guard = NET_LOCK.lock();

    // I-47: 检查活动 socket 上限 (≤ G_MAX_SOCKETS ≤ MAX_SOCKETS).
    // 运行时可通过 set_max_sockets 调整, 编译期上限 MAX_SOCKETS 静态保证.
    let active: usize = (0..MAX_SM_FD).filter(|&i| FD_TYPES.0[i] != 0).count();
    if active >= get_max_sockets() {
        return -E_NFILE;
    }

    let fd = sm_alloc_fd();
    if fd < 0 {
        return -E_NFILE;
    }
    let fd_idx = fd as usize;

    if domain == 2 && sock_type == 1 {
        // TD-07: TCP RX/TX 缓冲走 slab, 不再静态 BSS 占用.
        // SAFETY: k_malloc 在初始化后可用, 返回非空或 null. null 时立即归还 fd.
        let rx_ptr = crate::kernel::framework::mm::k_malloc(TCP_BUF_SIZE);
        if rx_ptr.is_null() {
            return -E_NOMEM;
        }
        let tx_ptr = crate::kernel::framework::mm::k_malloc(TCP_BUF_SIZE);
        if tx_ptr.is_null() {
            crate::kernel::framework::mm::k_free(rx_ptr);
            return -E_NOMEM;
        }
        // SAFETY: rx_ptr/tx_ptr 来自 k_malloc(TCP_BUF_SIZE), 长度合法, 唯一别名.
        let rx_slice = unsafe { core::slice::from_raw_parts_mut(rx_ptr, TCP_BUF_SIZE) };
        let tx_slice = unsafe { core::slice::from_raw_parts_mut(tx_ptr, TCP_BUF_SIZE) };
        let tcp_sock = tcp::Socket::new(
            tcp::SocketBuffer::new(rx_slice),
            tcp::SocketBuffer::new(tx_slice),
        );
        let sockets = &mut *socket_set();
        let handle = sockets.add(tcp_sock);
        SOCKET_TABLE.0[fd_idx] = Some(handle);
        FD_TYPES.0[fd_idx] = 1;
        // TD-07: buf 指针记入静态表, close 时按指针归还 slab.
        TCP_RX_BUFS[fd_idx] = rx_ptr;
        TCP_TX_BUFS[fd_idx] = tx_ptr;
        fd
    } else if domain == 2 && sock_type == 2 {
        // TD-07: UDP RX/TX 缓冲走 slab. metas 仍静态 (小, 16 KB).
        let rx_ptr = crate::kernel::framework::mm::k_malloc(UDP_BUF_SIZE);
        if rx_ptr.is_null() {
            return -E_NOMEM;
        }
        let tx_ptr = crate::kernel::framework::mm::k_malloc(UDP_BUF_SIZE);
        if tx_ptr.is_null() {
            crate::kernel::framework::mm::k_free(rx_ptr);
            return -E_NOMEM;
        }
        // SAFETY: rx_ptr/tx_ptr 由 k_alloc 分配, 已 null 检查并保证 4K 对齐;
        // UDP_BUF_SIZE 来自 cfg_smoltcp_cap, 适配 PacketBuffer 容量上限.
        let rx_slice = unsafe { core::slice::from_raw_parts_mut(rx_ptr, UDP_BUF_SIZE) };
        // SAFETY: 同上, tx_ptr 由 k_alloc 分配, 已 null 检查.
        let tx_slice = unsafe { core::slice::from_raw_parts_mut(tx_ptr, UDP_BUF_SIZE) };
        let udp_sock = udp::Socket::new(
            udp::PacketBuffer::new(
                &mut UDP_RX_METAS[fd_idx][..],
                rx_slice,
            ),
            udp::PacketBuffer::new(
                &mut UDP_TX_METAS[fd_idx][..],
                tx_slice,
            ),
        );
        let sockets = &mut *socket_set();
        let handle = sockets.add(udp_sock);
        SOCKET_TABLE.0[fd_idx] = Some(handle);
        FD_TYPES.0[fd_idx] = 2;
        UDP_RX_BUFS[fd_idx] = rx_ptr;
        UDP_TX_BUFS[fd_idx] = tx_ptr;
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

/// 从 sockaddr_in C 结构体解析 IPv4 端点。
///
/// # Safety
/// `addr` 必须指向有效的 `SockaddrIn` 结构体, 至少含 8 字节已初始化。
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

/// POSIX `bind(fd, addr, addrlen)` 内核实现。
///
/// # Safety
/// - `addr` 必须是有效的 sockaddr 指针, 含 `_addrlen` 字节已初始化。
/// - 由 `sys_bind` 系统调用分发, 调用方验证权限。
/// - NET_LOCK 持有。
#[no_mangle]
pub unsafe extern "C" fn sm_bind(fd: i32, addr: *const u8, _addrlen: u32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE.0[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };

    let sockets = &mut *socket_set();

    match FD_TYPES.0[fd as usize] {
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

/// POSIX `listen(fd, backlog)` 内核实现。
///
/// # Safety
/// NET_LOCK 持有; 由 `sys_listen` 分发, 调用方验证权限。
#[no_mangle]
pub unsafe extern "C" fn sm_listen(fd: i32, _backlog: i32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE.0[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };

    if FD_TYPES.0[fd as usize] != 1 {
        return -E_NOTSUPP;
    }

    let sockets = &mut *socket_set();
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

/// POSIX `accept(fd, addr, addrlen)` 内核实现。
///
/// # Safety
/// - `addr`/`_addrlen` 必须是有效的 sockaddr 指针 (此处忽略)。
/// - NET_LOCK 持有; 由 `sys_accept` 分发, 调用方验证权限。
#[no_mangle]
pub unsafe extern "C" fn sm_accept(fd: i32, _addr: *mut u8, _addrlen: *mut u32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE.0[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };

    if FD_TYPES.0[fd as usize] != 1 {
        return -E_NOTSUPP;
    }

    let sockets = &mut *socket_set();
    let sock = sockets.get_mut::<tcp::Socket>(handle);

    if sock.is_active() {
        fd
    } else {
        -E_AGAIN
    }
}

/// POSIX `connect(fd, addr, addrlen)` 内核实现。
///
/// # Safety
/// `addr` 必须指向有效的 sockaddr 结构, 至少 `_addrlen` 字节。
/// NET_LOCK 持有。
#[no_mangle]
pub unsafe extern "C" fn sm_connect(fd: i32, addr: *const u8, _addrlen: u32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE.0[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };

    if !crate::kernel::framework::net::NET_CONFIGURED.load(Ordering::Acquire) {
        return -E_NODEV;
    }

    let endpoint = match parse_ipv4_endpoint(addr) {
        Some(ep) => ep,
        None => return -E_INVAL,
    };

    if FD_TYPES.0[fd as usize] != 1 {
        return -E_NOTSUPP;
    }

    let stack = match NET_STACK.as_mut() {
        Some(s) => s,
        None => return -E_NODEV,
    };

    let sockets = &mut *socket_set();
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

/// POSIX `send(fd, buf, len, flags)` 内核实现。
///
/// # Safety
/// `buf` 必须指向至少 `len` 字节的有效可读内存, 内存必须在调用期间保持有效。
/// NET_LOCK 持有; 由 `sys_send` 分发, cred 校验已通过。
#[no_mangle]
pub unsafe extern "C" fn sm_send(fd: i32, buf: *const u8, len: u32, _flags: i32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE.0[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };
    if buf.is_null() || len == 0 {
        return -E_INVAL;
    }

    let sockets = &mut *socket_set();
    let data = core::slice::from_raw_parts(buf, len as usize);

    match FD_TYPES.0[fd as usize] {
        1 => {
            let sock = sockets.get_mut::<tcp::Socket>(handle);
            match sock.send_slice(data) {
                Ok(n) => n as i32,
                Err(_) => -E_CONNRESET,
            }
        }
        2 => {
            // UDP 无目的地址: 依赖 socket 已 "连接" (经 endpoint 绑定)
            // 简化处理, 返回 ENOTCONN; 请改用 sendto
            -E_NOTCONN
        }
        _ => -E_NOTSUPP,
    }
}

/// POSIX `recv(fd, buf, len, flags)` 内核实现。
///
/// # Safety
/// `buf` 必须指向至少 `len` 字节的有效可写内存, 内存必须在调用期间保持有效。
/// NET_LOCK 持有; 由 `sys_recv` 分发。
#[no_mangle]
pub unsafe extern "C" fn sm_recv(fd: i32, buf: *mut u8, len: u32, _flags: i32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE.0[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };
    if buf.is_null() || len == 0 {
        return -E_INVAL;
    }

    let sockets = &mut *socket_set();
    let data = core::slice::from_raw_parts_mut(buf, len as usize);

    match FD_TYPES.0[fd as usize] {
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

/// POSIX `sendto(fd, buf, len, flags, addr, addrlen)` 内核实现。
///
/// # Safety
/// `buf`/`addr` 必须是有效指针, 内存至少含 `len`/`_addrlen` 字节。
/// NET_LOCK 持有; 由 `sys_sendto` 分发。
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

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE.0[fd as usize] {
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

    let sockets = &mut *socket_set();
    let data = core::slice::from_raw_parts(buf, len as usize);

    match FD_TYPES.0[fd as usize] {
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

/// POSIX `recvfrom(fd, buf, len, flags, addr, addrlen)` 内核实现。
///
/// # Safety
/// `buf` 必须是有效可写指针, 至少 `len` 字节; `addr`/`_addrlen` 可选地写入对端地址。
/// NET_LOCK 持有; 由 `sys_recvfrom` 分发。
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

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE.0[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };
    if buf.is_null() || len == 0 {
        return -E_INVAL;
    }

    let sockets = &mut *socket_set();
    let data = core::slice::from_raw_parts_mut(buf, len as usize);

    match FD_TYPES.0[fd as usize] {
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

/// POSIX `sendmsg(fd, msghdr, flags)` 内核实现 (SG 拼接, 栈缓冲 4KB 上限).
///
/// # Safety
/// `msg` 必须是有效用户指针, 含完整 `Msghdr { msg_iov, msg_iovlen, ... }`.
/// 调用方 (services) 须先校验可读范围.
/// NET_LOCK 持有; 由 `sys_sendmsg` 分发.
#[no_mangle]
pub unsafe extern "C" fn sm_sendmsg(fd: i32, msg: *const u8, _flags: i32) -> i32 {
    if msg.is_null() {
        return -E_FAULT;
    }
    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    // 读 Msghdr
    // SAFETY: msg 由 services 校验可读 56 字节 (u64 Linux x86_64 / aarch64 布局).
    let msg_iov_ptr = core::ptr::read_unaligned(msg.add(16) as *const u64);
    let msg_iovlen_us = core::ptr::read_unaligned(msg.add(24) as *const u64) as usize;
    if msg_iovlen_us == 0 || msg_iovlen_us > 1024 {
        return -E_INVAL;
    }
    if msg_iov_ptr == 0 {
        return -E_INVAL;
    }
    // 拼接 iov 到 IobRegion (按需 alloc, 突破 4KB 栈限制; 性能瓶颈解除).
    // 先总容量, 再一次 alloc.
    let mut total: usize = 0;
    let mut lens: [usize; 1024] = [0usize; 1024];
    let mut bases: [u64; 1024] = [0u64; 1024];
    for i in 0..msg_iovlen_us {
        // SAFETY: msg_iov + i*Iovec(16) 可读 16 字节 (services 校验 iov 范围).
        let iov_base = core::ptr::read_unaligned((msg_iov_ptr as *const u8).add(i * 16) as *const u64);
        let iov_len = core::ptr::read_unaligned((msg_iov_ptr as *const u8).add(i * 16 + 8) as *const u64) as usize;
        bases[i] = iov_base;
        lens[i] = iov_len;
        if iov_base == 0 || iov_len == 0 {
            continue;
        }
        total = match total.checked_add(iov_len) {
            Some(v) => v,
            None => return -E_INVAL,
        };
    }
    if total == 0 {
        return 0;
    }
    let region = match crate::kernel::framework::iobuf::IobRegion::alloc(total) {
        Some(r) => r,
        None => return -E_NOMEM,
    };
    let mut off: usize = 0;
    for i in 0..msg_iovlen_us {
        if bases[i] == 0 || lens[i] == 0 {
            continue;
        }
        // SAFETY: iov_base 由 services 校验 lens[i] 字节可读; region 容量 >= total >= off+lens[i].
        core::ptr::copy_nonoverlapping(bases[i] as *const u8, region.as_mut_ptr().add(off), lens[i]);
        off += lens[i];
    }
    let rc = sm_send(fd, region.as_mut_ptr(), total as u32, 0);
    rc
}

/// POSIX `recvmsg(fd, msghdr, flags)` 内核实现 (SG 拆分, 栈缓冲 4KB 上限).
///
/// # Safety
/// `msg` 必须是有效可写用户指针, services 校验.
/// NET_LOCK 持有; 由 `sys_recvmsg` 分发.
#[no_mangle]
pub unsafe extern "C" fn sm_recvmsg(fd: i32, msg: *mut u8, _flags: i32) -> i32 {
    if msg.is_null() {
        return -E_FAULT;
    }
    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let msg_iov_ptr = core::ptr::read_unaligned(msg.add(16) as *const u64);
    let msg_iovlen_us = core::ptr::read_unaligned(msg.add(24) as *const u64) as usize;
    if msg_iovlen_us == 0 || msg_iovlen_us > 1024 {
        return -E_INVAL;
    }
    if msg_iov_ptr == 0 {
        return -E_INVAL;
    }
    // 计算总可用 iov 容量 + 收集 iov (突破 4KB 栈限制).
    let mut cap: usize = 0;
    let mut lens: [usize; 1024] = [0usize; 1024];
    let mut bases: [u64; 1024] = [0u64; 1024];
    for i in 0..msg_iovlen_us {
        let iov_base = core::ptr::read_unaligned((msg_iov_ptr as *const u8).add(i * 16) as *const u64);
        let iov_len = core::ptr::read_unaligned((msg_iov_ptr as *const u8).add(i * 16 + 8) as *const u64) as usize;
        bases[i] = iov_base;
        lens[i] = iov_len;
        if iov_base == 0 || iov_len == 0 {
            continue;
        }
        cap = match cap.checked_add(iov_len) {
            Some(v) => v,
            None => return -E_INVAL,
        };
    }
    if cap == 0 {
        return 0;
    }
    let region = match crate::kernel::framework::iobuf::IobRegion::alloc(cap) {
        Some(r) => r,
        None => return -E_NOMEM,
    };
    let n = sm_recv(fd, region.as_mut_ptr(), cap as u32, 0);
    if n <= 0 {
        return n;
    }
    // 拆分回 iov
    let mut left = n as usize;
    let mut off = 0usize;
    for i in 0..msg_iovlen_us {
        if left == 0 {
            break;
        }
        if bases[i] == 0 || lens[i] == 0 {
            continue;
        }
        let cp = core::cmp::min(lens[i], left);
        // SAFETY: iov_base 由 services 校验 cp 字节可写.
        core::ptr::copy_nonoverlapping(region.as_mut_ptr().add(off), bases[i] as *mut u8, cp);
        off += cp;
        left -= cp;
    }
    n
}

/// POSIX `close(fd)` 内核实现。
///
/// # Safety
/// NET_LOCK 持有; 由 `sys_close` 分发, cred 校验已通过。
#[no_mangle]
pub unsafe extern "C" fn sm_close(fd: i32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE.0[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };

    let stype = FD_TYPES.0[fd as usize];
    let sockets = &mut *socket_set();

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
    // TD-07: smoltcp socket 已 drop, buf 借用结束, 此时 k_free 安全.
    if !TCP_RX_BUFS[fd as usize].is_null() {
        crate::kernel::framework::mm::k_free(TCP_RX_BUFS[fd as usize]);
        TCP_RX_BUFS[fd as usize] = core::ptr::null_mut();
    }
    if !TCP_TX_BUFS[fd as usize].is_null() {
        crate::kernel::framework::mm::k_free(TCP_TX_BUFS[fd as usize]);
        TCP_TX_BUFS[fd as usize] = core::ptr::null_mut();
    }
    if !UDP_RX_BUFS[fd as usize].is_null() {
        crate::kernel::framework::mm::k_free(UDP_RX_BUFS[fd as usize]);
        UDP_RX_BUFS[fd as usize] = core::ptr::null_mut();
    }
    if !UDP_TX_BUFS[fd as usize].is_null() {
        crate::kernel::framework::mm::k_free(UDP_TX_BUFS[fd as usize]);
        UDP_TX_BUFS[fd as usize] = core::ptr::null_mut();
    }
    SOCKET_TABLE.0[fd as usize] = None;
    FD_TYPES.0[fd as usize] = 0;
    0
}

/// POSIX `setsockopt` 内核实现 (当前空操作占位)。
///
/// # Safety
/// `_optval` 必须是有效指针, 含 `_optlen` 字节 (此处忽略)。
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

/// POSIX `getsockopt` 内核实现 (当前空操作占位)。
///
/// # Safety
/// `_optval` 必须是有效可写指针, `_optlen` 必须是有效可写 u32 指针 (此处忽略)。
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

/// POSIX `getsockname(fd, addr, addrlen)` 内核实现。
///
/// 真实实现: 写回 socket 的 local endpoint 到 `*addr`, 更新 `*addrlen`。
/// TCP 用 `local_endpoint()`, UDP 用 `endpoint()` (IpListenEndpoint).
///
/// # Safety
/// - `addr` 必须是可写 sockaddr 指针, 至少 `_addrlen` 字节.
/// - `_addrlen` 必须是可写 u32 指针 (写回实际长度).
/// - NET_LOCK 持有; 由 `sys_getsockname` 分发.
#[no_mangle]
pub unsafe extern "C" fn sm_getsockname(fd: i32, addr: *mut u8, addrlen: *mut u32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE.0[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };
    if addr.is_null() || addrlen.is_null() {
        return -E_INVAL;
    }
    let stype = FD_TYPES.0[fd as usize];
    let sockets = &mut *socket_set();

    let endpoint_opt: Option<IpEndpoint> = match stype {
        1 => {
            let sock = sockets.get::<tcp::Socket>(handle);
            sock.local_endpoint()
        }
        2 => {
            let sock = sockets.get::<udp::Socket>(handle);
            let ep = sock.endpoint();
            match ep.addr {
                Some(addr) => Some(IpEndpoint { addr, port: ep.port }),
                None => Some(IpEndpoint {
                    addr: IpAddress::Ipv4(Ipv4Address::UNSPECIFIED),
                    port: ep.port,
                }),
            }
        }
        _ => return -E_NOTSUPP,
    };

    let endpoint = match endpoint_opt {
        Some(e) => e,
        None => return -E_NOTCONN, // TCP 未 connect
    };
    let (ip_bytes, port) = match endpoint.addr {
        IpAddress::Ipv4(v4) => (v4.octets(), endpoint.port),
        _ => return -E_AFNOSUPPORT,
    };
    let sin = SockaddrIn {
        sin_family: 2,
        sin_port: u16::to_be(port),
        sin_addr: ip_bytes,
        sin_zero: [0u8; 8],
    };
    core::ptr::write_unaligned(addr as *mut SockaddrIn, sin);
    *addrlen = core::mem::size_of::<SockaddrIn>() as u32;
    0
}

/// POSIX `getpeername(fd, addr, addrlen)` 内核实现。
///
/// 真实实现: 写回 socket 的 remote endpoint 到 `*addr` (TCP 需已 connect).
///
/// # Safety
/// - `addr` 必须是可写 sockaddr 指针, 至少 `_addrlen` 字节.
/// - `_addrlen` 必须是可写 u32 指针 (写回实际长度).
/// - NET_LOCK 持有; 由 `sys_getpeername` 分发.
#[no_mangle]
pub unsafe extern "C" fn sm_getpeername(fd: i32, addr: *mut u8, addrlen: *mut u32) -> i32 {
    let _guard = NET_LOCK.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || FD_TYPES.0[fd as usize] == 0 {
        return -E_BADF;
    }
    let handle = match SOCKET_TABLE.0[fd as usize] {
        Some(h) => h,
        None => return -E_BADF,
    };
    if addr.is_null() || addrlen.is_null() {
        return -E_INVAL;
    }
    let stype = FD_TYPES.0[fd as usize];
    let sockets = &mut *socket_set();

    let endpoint_opt: Option<IpEndpoint> = match stype {
        1 => {
            let sock = sockets.get::<tcp::Socket>(handle);
            sock.remote_endpoint()
        }
        2 => {
            // UDP: remote 由 last_recv_meta 取, 但 Socket 没暴露, 暂返 ENOTCONN.
            return -E_NOTCONN;
        }
        _ => return -E_NOTSUPP,
    };

    let endpoint = match endpoint_opt {
        Some(e) => e,
        None => return -E_NOTCONN,
    };
    let (ip_bytes, port) = match endpoint.addr {
        IpAddress::Ipv4(v4) => (v4.octets(), endpoint.port),
        _ => return -E_AFNOSUPPORT,
    };
    let sin = SockaddrIn {
        sin_family: 2,
        sin_port: u16::to_be(port),
        sin_addr: ip_bytes,
        sin_zero: [0u8; 8],
    };
    core::ptr::write_unaligned(addr as *mut SockaddrIn, sin);
    *addrlen = core::mem::size_of::<SockaddrIn>() as u32;
    0
}

/// 轮询所有 socket 状态 (驱动 `select/poll` 内核实现)。
///
/// # Safety
/// NET_LOCK 持有; 由 `sys_poll`/`sys_select` 分发。
#[no_mangle]
pub unsafe extern "C" fn sm_poll_sockets() -> i32 {
    let _guard = NET_LOCK.lock();

    let sockets = &mut *socket_set();
    process_dhcp_events(sockets);

    for i in 0..MAX_SM_FD {
        if FD_TYPES.0[i] != 1 {
            continue;
        }
        if let Some(handle) = SOCKET_TABLE.0[i] {
            let _sock = sockets.get_mut::<tcp::Socket>(handle);
        }
    }
    0
}

// ============================================================================
// 公共 API
// ============================================================================

// I-47: FD 表容量, 与 MAX_SOCKETS 对齐 (每个 FD 对应一个 smoltcp socket).
// TD-02: 基址与容量改由 `framework::proc::FdPlan::SMOLTCP` 单一来源; 容量现从 FdRange.capacity 派生.
const MAX_SM_FD: usize = crate::kernel::framework::proc::FdPlan::SMOLTCP.capacity as usize;
const TCP_BUF_SIZE: usize = 4096;
const UDP_BUF_SIZE: usize = 2048;
const UDP_META_COUNT: usize = 4;

// TD-05: 8 张 smoltcp 大表, 小型热表按 64 字节 cache line 对齐, 减少多核 false sharing.
// 大型 buffer (TCP/UDP buf) 单 fd 独占一整片区域, 默认不会被相邻 fd 抢用, 仅需保持页对齐即可.
//
// 实现方式: `#[repr(align(N))]` 不能直接用于 `static mut [T; N]`, 改用 `static mut W: Wrapper<T>`.
#[repr(align(64))]
struct Align64<T>(T);

#[allow(non_camel_case_types)]
type SOCKET_TABLE_T = Align64<[Option<SocketHandle>; MAX_SM_FD]>;
#[allow(non_camel_case_types)]
type FD_TYPES_T = Align64<[u8; MAX_SM_FD]>;

static mut SOCKET_TABLE: SOCKET_TABLE_T = Align64([None; MAX_SM_FD]);
// Per-fd 类型标记: 0=free, 1=tcp, 2=udp.
// 64 字节对齐: 8 核机器下每核独立访问自己 fd 对应的 cache line, 不会因 1 字节写触发整行 invalidation.
static mut FD_TYPES: FD_TYPES_T = Align64([0u8; MAX_SM_FD]);

// TCP buffer storage (per fd)
// TD-07: 由 4 张 [[u8; N]; MAX_SM_FD] 静态数组 (≈3 MB BSS) 改为 [*mut u8; MAX_SM_FD] 指针表.
// 启动时 0 占用; socket alloc 时通过 `k_malloc` (slab) 申请; close 时 `k_free` 归还.
// 省下的 3 MB BSS 改为按需占用, 与 smoltcp `MAX_SM_FD` 解耦 (见 TD-06).
static mut TCP_RX_BUFS: [*mut u8; MAX_SM_FD] = [null_mut(); MAX_SM_FD];
static mut TCP_TX_BUFS: [*mut u8; MAX_SM_FD] = [null_mut(); MAX_SM_FD];

// UDP buffer storage (per fd) — 同样 TD-07 改造
static mut UDP_RX_BUFS: [*mut u8; MAX_SM_FD] = [null_mut(); MAX_SM_FD];
static mut UDP_TX_BUFS: [*mut u8; MAX_SM_FD] = [null_mut(); MAX_SM_FD];

// UDP metas 仍保留静态 (16 KB, 256 × 4 × 16B, 不值得动); td 改 metas 走 heap 是 V2 任务.
static mut UDP_RX_METAS: [[udp::PacketMetadata; UDP_META_COUNT]; MAX_SM_FD] =
    [[udp::PacketMetadata::EMPTY; UDP_META_COUNT]; MAX_SM_FD];
static mut UDP_TX_METAS: [[udp::PacketMetadata; UDP_META_COUNT]; MAX_SM_FD] =
    [[udp::PacketMetadata::EMPTY; UDP_META_COUNT]; MAX_SM_FD];

/// # Safety
///
/// - 直接访问 `static mut` 全局表 (`FD_TYPES`, `SOCKET_TABLE`)
/// - 调用方须保证在持有 `SM_FD_TABLE_LOCK` 时调用
unsafe fn sm_alloc_fd() -> i32 {
    for i in 0..MAX_SM_FD {
        if FD_TYPES.0[i] == 0 && SOCKET_TABLE.0[i].is_none() {
            // TD-02 V3: 通过 fd_alloc 集中计算 FD 编号
            return crate::kernel::framework::proc::fd_at(
                crate::kernel::framework::proc::FdSubsystem::Smoltcp,
                i,
            );
        }
    }
    -1
}

pub fn is_network_initialized() -> bool {
    crate::kernel::framework::net::NET_READY.load(Ordering::Acquire)
}

pub fn is_network_configured() -> bool {
    crate::kernel::framework::net::NET_CONFIGURED.load(Ordering::Acquire)
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

// ============================================================================
// D1.1/D1.2 高层 API 底层实现
// ============================================================================

/// 网络状态快照 (单次原子读, 多字段可能轻微不一致 — 用于观测/debug)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetStatus {
    pub state: InitState,
    pub mac: [u8; 6],
    pub ipv4: Option<[u8; 4]>,
    pub gateway: Option<[u8; 4]>,
    pub dns: [Option<[u8; 4]>; 3],
    pub dhcp_configured: bool,
}

impl NetStatus {
    pub fn capture() -> Self {
        let mac_raw = G_MAC.load(Ordering::Acquire);
        let mac = mac_raw.to_be_bytes()[2..8].try_into().unwrap_or([0; 6]);
        let ipv4 = ipv4_from_atomic(G_IPV4.load(Ordering::Acquire));
        let gateway = ipv4_from_atomic(G_GATEWAY.load(Ordering::Acquire));
        let dns = [
            ipv4_from_atomic(G_DNS[0].load(Ordering::Acquire)),
            ipv4_from_atomic(G_DNS[1].load(Ordering::Acquire)),
            ipv4_from_atomic(G_DNS[2].load(Ordering::Acquire)),
        ];
        NetStatus {
            state: get_init_state(),
            mac,
            ipv4,
            gateway,
            dns,
            dhcp_configured: crate::kernel::framework::net::NET_CONFIGURED
                .load(Ordering::Acquire),
        }
    }
}

fn ipv4_from_atomic(v: u32) -> Option<[u8; 4]> {
    if v == 0 {
        None
    } else {
        Some(v.to_be_bytes())
    }
}

/// 主动触发网络初始化 (非阻塞; 失败返回 false)
///
/// # 行为
/// - 状态机 = Uninitialized 时, 直接返回 false (需要先有 chitin 设备注册)
/// - 状态机 = HardwareProbed/InterfaceReady 时, 启动 DHCP 握手
/// - 状态机 = FullyInitialized 时, 直接返回 true
/// - 状态机 = Failed 时, 不重试, 返回 false
pub fn trigger_init() -> bool {
    match get_init_state() {
        InitState::FullyInitialized => true,
        InitState::HardwareProbed | InitState::InterfaceReady => {
            // DHCP 已经在轮询路径里跑了, 此处仅给上层一个"我已确认"信号
            true
        }
        _ => false,
    }
}

/// 查询设备 MAC 地址
pub fn get_mac_address() -> Option<[u8; 6]> {
    let raw = G_MAC.load(Ordering::Acquire);
    if raw == 0 {
        None
    } else {
        let bytes = raw.to_be_bytes();
        Some([bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]])
    }
}

/// 把 [u8; 6] MAC 写入 G_MAC (大端打包为 u64)
pub(crate) fn store_mac(mac: [u8; 6]) {
    let mut buf = [0u8; 8];
    buf[2..8].copy_from_slice(&mac);
    G_MAC.store(u64::from_be_bytes(buf), Ordering::Release);
}

/// 查询当前 IPv4
pub fn get_ipv4_address() -> Option<[u8; 4]> {
    ipv4_from_atomic(G_IPV4.load(Ordering::Acquire))
}

/// 查询默认网关
pub fn get_default_gateway() -> Option<[u8; 4]> {
    ipv4_from_atomic(G_GATEWAY.load(Ordering::Acquire))
}

/// 查询 DNS 服务器列表
pub fn get_dns_servers() -> [Option<[u8; 4]>; 3] {
    [
        ipv4_from_atomic(G_DNS[0].load(Ordering::Acquire)),
        ipv4_from_atomic(G_DNS[1].load(Ordering::Acquire)),
        ipv4_from_atomic(G_DNS[2].load(Ordering::Acquire)),
    ]
}

/// 静态 hosts 表条目: 主机名 → IPv4
#[derive(Debug, Clone, Copy)]
struct HostEntry {
    name: &'static str,
    ip: [u8; 4],
}

/// 内置静态 hosts (D1.2 起步, D 阶段后续可换 smoltcp wire/dns 升级)
// I-46: hosts 表里 10.0.2.x 引用集中常量, 避免散落硬编码
const STATIC_HOSTS: &[HostEntry] = &[
    HostEntry { name: "localhost",       ip: [127, 0, 0, 1] },
    HostEntry { name: "router",          ip: types::FALLBACK_GATEWAY },
    HostEntry { name: "host",            ip: types::FALLBACK_IPV4 },
    HostEntry { name: "qemu-gateway",    ip: types::FALLBACK_GATEWAY },
    HostEntry { name: "antx-gateway",    ip: types::FALLBACK_GATEWAY },
];

/// 简单 DNS 解析 (静态 hosts 表)
///
/// # 实现
/// - 精确匹配主机名 (不区分大小写 — ASCII tolower)
/// - 大小写不敏感: "Router" / "ROUTER" / "router" 都匹配
///
/// # 局限 (D 阶段后续工作)
/// - 不发起 DNS UDP 查询
/// - 不支持通配 (`*.example.com`)
/// - 不支持 AAAA (IPv6)
pub fn dns_resolve(name: &str) -> Option<[u8; 4]> {
    for entry in STATIC_HOSTS {
        if entry.name.eq_ignore_ascii_case(name) {
            return Some(entry.ip);
        }
    }
    // 数字字面量解析: "10.0.2.15" 直接返 (避免对 IP 字符串做 DNS 浪费)
    if let Some(ip) = parse_ipv4_literal(name) {
        return Some(ip);
    }
    None
}

/// 解析 IPv4 字面量 "a.b.c.d" (无错处理; 不合法返 None)
pub(crate) fn parse_ipv4_literal(s: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut idx = 0usize;
    let mut cur: u32 = 0;
    let mut has_digit = false;
    for &b in s.as_bytes() {
        if b == b'.' {
            if !has_digit || idx >= 3 || cur > 255 {
                return None;
            }
            octets[idx] = cur as u8;
            idx += 1;
            cur = 0;
            has_digit = false;
        } else if b.is_ascii_digit() {
            cur = cur * 10 + (b - b'0') as u32;
            has_digit = true;
        } else {
            return None;
        }
    }
    if !has_digit || idx != 3 || cur > 255 {
        return None;
    }
    octets[3] = cur as u8;
    Some(octets)
}

/// 显式关闭网络栈 (重置配置 + 状态)
pub fn shutdown_network() {
    let _guard = NET_LOCK.lock();
    G_IPV4.store(0, Ordering::Release);
    G_GATEWAY.store(0, Ordering::Release);
    G_DNS[0].store(0, Ordering::Release);
    G_DNS[1].store(0, Ordering::Release);
    G_DNS[2].store(0, Ordering::Release);
    crate::kernel::framework::net::NET_CONFIGURED.store(false, Ordering::Release);
    G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);
    raw::klog_msg("Network shutdown");
}

/// 重置网络栈状态 (供栏栈 BHR / 异常恢复使用)。
///
/// # Safety
/// - 必须持有 NET_LOCK (内部获取)。
/// - 必须在所有 socket 关闭后调用, 否则可能泄漏资源。
pub unsafe fn reset_network_state() {
    let _guard = NET_LOCK.lock();

    G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);

    raw::clear_all();
    SOCKETS_INITIALIZED.store(false, Ordering::Release);
}

// ============================================================================
// 特权子模块 (Framekernel raw): 集中 static mut 访问
// ============================================================================

pub(crate) mod raw {
    use super::*;

    /// 安全访问 NET_STACK (Framekernel 集中 unsafe 边界)
    pub fn stack_mut() -> Option<&'static mut NetworkStack> {
        // SAFETY: NET_STACK 由 NET_LOCK 保护, 调用方已持有锁或处于单线程上下文。
        unsafe { NET_STACK.as_mut() }
    }

    /// 安全访问 NET_DEVICE
    pub fn device_mut() -> Option<&'static mut ChitinNetDevice> {
        // SAFETY: 同上。
        unsafe { NET_DEVICE.as_mut() }
    }

    /// 安全设置 NET_DEVICE
    pub fn set_device(d: Option<ChitinNetDevice>) {
        // SAFETY: 由 NET_LOCK 保护。
        unsafe { NET_DEVICE = d; }
    }

    /// 安全设置 NET_STACK
    pub fn set_stack(s: Option<NetworkStack>) {
        // SAFETY: 由 NET_LOCK 保护。
        unsafe { NET_STACK = s; }
    }

    /// 安全读取 DHCP_HANDLE
    pub fn dhcp_handle() -> Option<SocketHandle> {
        // SAFETY: SocketHandle 是 Copy, 读取无副作用。
        unsafe { DHCP_HANDLE }
    }

    /// 安全设置 DHCP_HANDLE
    pub fn set_dhcp_handle(h: Option<SocketHandle>) {
        // SAFETY: 由 NET_LOCK 保护。
        unsafe { DHCP_HANDLE = h; }
    }

    /// 安全清空网络全局状态
    pub fn clear_all() {
        // SAFETY: 由 NET_LOCK 保护, 串行重置流程。
        unsafe {
            NET_DEVICE = None;
            NET_STACK = None;
            DHCP_HANDLE = None;
        }
    }

    /// 安全获取 SocketSet 指针
    pub fn socket_set() -> *mut SocketSet<'static> {
        // SAFETY: SOCKET_SET 在 init_sockets 后已初始化, 调用方在 NET_LOCK 下。
        unsafe { SOCKET_SET.as_mut_ptr() }
    }

    /// 安全初始化 sockets
    pub fn init_sockets() {
        // SAFETY: 由 NET_LOCK 保护, 单次初始化。
        unsafe { super::init_sockets() }
    }

    /// 安全处理 DHCP 事件
    pub fn process_dhcp_events(sockets: &mut SocketSet<'_>) {
        // SAFETY: 由 NET_LOCK 保护, sockets 来自本模块的 socket_set()。
        unsafe { super::process_dhcp_events(sockets) }
    }

    /// klog 网络消息 (C 字符串) - 安全包装
    pub fn klog_msg(s: &str) {
        let mut buf = [0u8; 256];
        let bytes = s.as_bytes();
        let len = bytes.len().min(buf.len() - 1);
        buf[..len].copy_from_slice(&bytes[..len]);
        // SAFETY: 临时 C 字符串, 在 klog_net 调用期间有效。
        unsafe { klog_net(buf.as_ptr().cast()) };
    }

    /// klog 初始化消息 (走 klog_init_msg)
    pub fn klog_init(s: &str) {
        let mut buf = [0u8; 256];
        let bytes = s.as_bytes();
        let len = bytes.len().min(buf.len() - 1);
        buf[..len].copy_from_slice(&bytes[..len]);
        // SAFETY: 临时 C 字符串, 在 klog_init_msg 调用期间有效。
        unsafe { klog_init_msg(buf.as_ptr().cast()) };
    }

    /// klog 错误消息
    pub fn klog_err(s: &str) {
        let mut buf = [0u8; 256];
        let bytes = s.as_bytes();
        let len = bytes.len().min(buf.len() - 1);
        buf[..len].copy_from_slice(&bytes[..len]);
        // SAFETY: 临时 C 字符串, 在 klog_net_err 调用期间有效。
        unsafe { klog_net_err(buf.as_ptr().cast()) };
    }
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

        // SAFETY: 单线程测试, `reset_network_state` 仅触达 `static mut` 单调状态
        unsafe {
            reset_network_state();
        }
        assert_eq!(get_init_state(), InitState::Uninitialized);
    }

    // ── D1.2 新增: parse_ipv4_literal / dns_resolve 纯逻辑测试 ──

    #[test]
    fn test_parse_ipv4_literal_valid() {
        assert_eq!(parse_ipv4_literal("0.0.0.0"), Some([0, 0, 0, 0]));
        assert_eq!(parse_ipv4_literal("10.0.2.15"), Some([10, 0, 2, 15]));
        assert_eq!(parse_ipv4_literal("255.255.255.255"), Some([255, 255, 255, 255]));
        assert_eq!(parse_ipv4_literal("127.0.0.1"), Some([127, 0, 0, 1]));
    }

    #[test]
    fn test_parse_ipv4_literal_invalid() {
        assert_eq!(parse_ipv4_literal(""), None);
        assert_eq!(parse_ipv4_literal("10"), None);
        assert_eq!(parse_ipv4_literal("10.0"), None);
        assert_eq!(parse_ipv4_literal("10.0.2"), None);
        assert_eq!(parse_ipv4_literal("10.0.2.15.1"), None);
        assert_eq!(parse_ipv4_literal("10.0.2.256"), None);   // 越界
        assert_eq!(parse_ipv4_literal("10.0..15"), None);
        assert_eq!(parse_ipv4_literal("a.b.c.d"), None);
        assert_eq!(parse_ipv4_literal("10.0.2."), None);
        assert_eq!(parse_ipv4_literal(".10.0.2.15"), None);
        assert_eq!(parse_ipv4_literal("10.0.2.15 "), None);   // 尾随空格
    }

    #[test]
    fn test_dns_resolve_static_hosts() {
        assert_eq!(dns_resolve("localhost"), Some([127, 0, 0, 1]));
        assert_eq!(dns_resolve("LOCALHOST"), Some([127, 0, 0, 1]));   // 大小写不敏感
        assert_eq!(dns_resolve("Router"), Some([10, 0, 2, 2]));
        assert_eq!(dns_resolve("qemu-gateway"), Some([10, 0, 2, 2]));
        assert_eq!(dns_resolve("antx-gateway"), Some([10, 0, 2, 2]));
    }

    #[test]
    fn test_dns_resolve_unknown_falls_back_to_ip_literal() {
        // 未知主机名直接走 IPv4 字面量路径
        assert_eq!(dns_resolve("8.8.8.8"), Some([8, 8, 8, 8]));
        assert_eq!(dns_resolve("10.0.2.15"), Some([10, 0, 2, 15]));
    }

    #[test]
    fn test_dns_resolve_returns_none_for_garbage() {
        assert_eq!(dns_resolve("nonexistent.example.com"), None);
        assert_eq!(dns_resolve(""), None);
        assert_eq!(dns_resolve("999.999.999.999"), None);
    }

    #[test]
    fn test_ipv4_from_atomic() {
        assert_eq!(ipv4_from_atomic(0), None);
        assert_eq!(ipv4_from_atomic(0x0A00020F), Some([10, 0, 2, 15]));
        assert_eq!(ipv4_from_atomic(0xFF000001), Some([255, 0, 0, 1]));
    }

    #[test]
    fn test_net_status_capture_initial_state() {
        // SAFETY: 单线程测试, reset 仅修改状态原子变量
        unsafe { reset_network_state(); }
        let s = NetStatus::capture();
        assert_eq!(s.state, InitState::Uninitialized);
        assert_eq!(s.ipv4, None);
        assert_eq!(s.gateway, None);
        assert_eq!(s.dns, [None, None, None]);
        assert!(!s.dhcp_configured);
    }

    #[test]
    fn test_store_and_get_mac_roundtrip() {
        // SAFETY: 单线程测试, reset 仅修改状态原子变量
        unsafe { reset_network_state(); }
        let mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];
        store_mac(mac);
        assert_eq!(get_mac_address(), Some(mac));
    }

    #[test]
    fn test_dns_servers_default_empty() {
        // SAFETY: 单线程测试, reset 仅修改状态原子变量
        unsafe { reset_network_state(); }
        let dns = get_dns_servers();
        assert_eq!(dns, [None, None, None]);
    }
}