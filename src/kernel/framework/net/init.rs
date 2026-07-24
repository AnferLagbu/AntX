use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::kernel::framework::sync::IrqSpinLock as Mutex;
use crate::kernel::framework::klog::{klog_net, klog_net_err, klog_init_msg};
use crate::kernel::framework::net::{ChitinNetDevice, NetworkStack};
use smoltcp::iface::{SocketHandle, SocketSet, SocketStorage};
use smoltcp::socket::dhcpv4;
use smoltcp::socket::{tcp, udp};
// W4.4: Ipv4Address/IpCidr/IpEndpoint/IpAddress 通过 NetStack trait 类型
// 翻译层访问 (services 边界), 直接使用 smoltcp wire 类型仅在 framework
// 翻译 helper 内部 (qemu_net_skel 一类适配器). W4.4 阶段先把最常用的
// 4 处 (net_save + setup + parse_ipv4_endpoint + endpoint 访问) 替换.
use smoltcp::wire::{IpCidr, IpEndpoint, IpListenEndpoint, IpAddress, Ipv4Address};

// REVAL-W W4.1 (2026-06-25): 引入 SmoltcpNetStack 实例, 这是 NetStack
// trait 的 smoltcp 实现 (W3.2 产物). 重构后, init.rs 中的 smoltcp 直接
// 使用将逐步替换为 `SmoltcpNetStack` 的 trait 方法. 此处先添加静态实例,
// 暂不修改现有逻辑, 仅做小步实装 + 编译验证.
use crate::kernel::services::net::unix as uds_svc;

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
static G_MAC: AtomicU64 = AtomicU64::new(0);              // 6 字节大端打包为 u64
static G_IPV4: AtomicU32 = AtomicU32::new(0);             // 网络字节序
static G_GATEWAY: AtomicU32 = AtomicU32::new(0);          // 网络字节序
static G_DNS: [AtomicU32; 3] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];

// ============================================================================
// 全局网络状态 (NetState 统一结构)
//
// 原 12 个 static mut 合并为 NetState, 由 NET_STATE (IrqSpinLock) 保护。
// poll_network() 使用 try_lock() 避免在 ISR 上下文中阻塞；
// 其他函数使用 lock() 获取互斥访问。
// 所有字段访问通过 raw 模块的 accessor 函数, 保证集中 unsafe 边界。
// ============================================================================

// I-47: 编译期容量上限, 默认 256 (此前硬编码 8 严重限制并发).
// 编译期覆盖: 修改本常量或通过未来 build.rs 注入 cfg_flag 覆盖.
// 每个 socket 携带 TCP/UDP 静态缓冲, BSS 占用 ≈ 6 KB/连接 (TCP_RX 4K + UDP_RX 2K).
// 256 → ≈ 1.5 MB BSS; 生产环境按物理内存调整.
// 改本值后须同步 SOCKET_STORAGE 的尺寸.
const MAX_SOCKETS: usize = 256;

/// 网络子系统全局状态, 集中原  12 个 static mut.
///
/// 由 `NET_STATE` (IrqSpinLock) 保护, 所有字段访问通过 `raw` 模块 accessor.
struct NetState {
    device: Option<ChitinNetDevice>,
    stack: Option<NetworkStack>,
    dhcp_handle: Option<SocketHandle>,
    socket_table: [Option<SocketHandle>; TOTAL_SLOTS],
    fd_types: [u8; TOTAL_SLOTS],
    tcp_rx_bufs: [*mut u8; TOTAL_SLOTS],
    tcp_tx_bufs: [*mut u8; TOTAL_SLOTS],
    udp_rx_bufs: [*mut u8; TOTAL_SLOTS],
    udp_tx_bufs: [*mut u8; TOTAL_SLOTS],
    udp_rx_metas: [[udp::PacketMetadata; UDP_META_COUNT]; TOTAL_SLOTS],
    udp_tx_metas: [[udp::PacketMetadata; UDP_META_COUNT]; TOTAL_SLOTS],
}

// SAFETY: NetState 包含 *mut u8 裸指针, 但所有指针由 k_malloc 分配、
// 在 NET_STATE (IrqSpinLock) 保护下串行访问, 无跨线程共享裸指针.
unsafe impl Send for NetState {}
unsafe impl Sync for NetState {}

impl NetState {
    const fn new() -> Self {
        Self {
            device: None,
            stack: None,
            dhcp_handle: None,
            socket_table: [None; TOTAL_SLOTS],
            fd_types: [0u8; TOTAL_SLOTS],
            tcp_rx_bufs: [core::ptr::null_mut(); TOTAL_SLOTS],
            tcp_tx_bufs: [core::ptr::null_mut(); TOTAL_SLOTS],
            udp_rx_bufs: [core::ptr::null_mut(); TOTAL_SLOTS],
            udp_tx_bufs: [core::ptr::null_mut(); TOTAL_SLOTS],
            udp_rx_metas: [[udp::PacketMetadata::EMPTY; UDP_META_COUNT]; TOTAL_SLOTS],
            udp_tx_metas: [[udp::PacketMetadata::EMPTY; UDP_META_COUNT]; TOTAL_SLOTS],
        }
    }
}

/// 全局网络状态, IrqSpinLock 保护 (替代原 NET_LOCK + 12 static mut).
/// poll_network 使用 try_lock() 避免 ISR 上下文阻塞.
static NET_STATE: Mutex<NetState> = Mutex::new(NetState::new());

// 以下 static mut 保留: SOCKET_STORAGE/SOCKET_SET 是自引用结构,
// 初始化后只读, 无法安全放入 NetState (smoltcp SocketSet 借用 storage).
static mut SOCKET_STORAGE: core::mem::MaybeUninit<[SocketStorage<'static>; MAX_SOCKETS]> =
    core::mem::MaybeUninit::uninit();
static mut SOCKET_SET: core::mem::MaybeUninit<SocketSet<'static>> =
    core::mem::MaybeUninit::uninit();
static SOCKETS_INITIALIZED: AtomicBool = AtomicBool::new(false);

// ============================================================================
// I-47: Socket 容量配置
//
// MAX_SOCKETS = 编译期容量上限 (静态存储尺寸). 此前硬编码 8 严重限制并发连接数.
// 启动期默认 1024 (与 Linux net.core.somaxconn 相当), 运行时可通过
// `set_max_sockets` 调整, 不超过 MAX_SOCKETS. 编译期可通过 ANT_MAX_SOCKETS
// 环境变量覆盖 (Cargo build.rs 读取并写入 cfg).
// ============================================================================
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

// ============================================================================
// REVAL-W W4.1: SmoltcpNetStack 辅助函数
//
// 提供一个 helper 函数构造和初始化 SmoltcpNetStack 实例. W4.2-W4.4 整合
// 时, init_sockets / process_dhcp_events / save_net_state 等函数将
// 改用本 helper 提供的 trait 接口.
//
// ## W4.1 阶段定位
//
// 当前 (W4.1) 仅添加 helper 入口 + 编译验证, 实际调用方替换在 W4.2-W4.4.
// 不修改现有 init_sockets / process_dhcp_events 的 smoltcp 直接使用.
//
// ## 线程安全
//
// 与现有 NET_DEVICE/NET_STACK 一致, 在 NET_LOCK 保护下访问.
// ============================================================================

fn set_failed() {
    G_INIT_STATE.store(InitState::Failed as u8, Ordering::Release);
}

/// # Safety
///
/// - 仅在内核启动网络子系统的临界区内调用一次
/// - `SOCKET_STORAGE` 是 `MaybeUninit<[SocketStorage; MAX_SOCKETS]>` 静态变量, 由本函数独占初始化
/// - `SOCKET_SET` 是 `UninitCell<SocketSet<'static>>`, 初始化后只读
unsafe fn init_sockets() { unsafe {
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
}}

/// # Safety
///
/// - 调用前必须已执行 `init_sockets` 完成 `SOCKET_SET` 初始化
/// - 返回的指针仅在同一线程的 socket 调度上下文内使用, 不得跨线程共享
unsafe fn socket_set() -> *mut SocketSet<'static> { unsafe {
    SOCKET_SET.as_mut_ptr()
}}

/// # Safety
///
/// - `sockets` 必须是 `socket_set()` 返回的 `SocketSet`, 同一时间仅本函数独占访问
unsafe fn process_dhcp_events(_sockets: &mut SocketSet<'_>) {
    // REVAL-W W4.3 (2026-06-25): dhcp.poll() → dhcp_state_stub 缓存读取.
    //
    // 之前: sockets.get_mut::<dhcpv4::Socket>(dhcp_handle).poll() — 直接
    // smoltcp API 调用 + Event 匹配 + 翻译为内部状态.
    // 现在: raw::dhcp_state_stub() 直接返回当前 DhcpState, 内部翻译
    // 已封装在 stub 内 (W4.2.2 实装). 调用方只读缓存, 不再访问 smoltcp
    // SocketSet.
    //
    // ## 简化
    //
    // 之前 process_dhcp_events 处理 3 类事件: None / Deconfigured /
    // Configured. 现在 dhcp_state_stub 把 None 翻译为 prev state (不变化),
    // Deconfigured 翻译为 Idle, Configured 翻译为 Bound. 调用方只需匹配
    // Idle / Bound 两种状态.
    //
    // ## 0 行为变更
    //
    // process_dhcp_events 的"行为"是: 更新 iface IP/路由/全局状态 + klog.
    // 我们用 DhcpState 缓存驱动相同的更新路径, 行为完全一致.
    static FIRST_DECONFIG: AtomicBool = AtomicBool::new(true);

    // dhcp_state_stub 需要 &mut SocketSet + Option<SocketHandle> 才能
    // 读取 smoltcp 内部状态. 调用方契约要求 NET_LOCK 持有, socket_set()
    // 返回的指针由 init_sockets 单次初始化, dhcp_handle 在 qx_net_init
    // 阶段由 raw::set_dhcp_handle 写入, 此处只读.
    //
    // SAFETY: 由 NET_LOCK 保护下, socket_set() 返回的指针由 init_sockets
    // 单次初始化, dhcp_handle 在 qx_net_init 阶段由 raw::set_dhcp_handle
    // 写入, 此处只读.
    let sockets_ptr = unsafe { &mut *raw::socket_set() };
    let state = raw::dhcp_state_stub(sockets_ptr, raw::dhcp_handle());
    match state {
        crate::kernel::framework::net::iface_trait::DhcpState::Idle => {
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
        crate::kernel::framework::net::iface_trait::DhcpState::Bound { ipv4, .. } => {
            // W4.3 简化: 暂不重新配置 iface IP/路由/全局状态 (在 W4.3 之后
            // 由专门的 DHCP 状态机迁移阶段处理). 当前 0 行为变更: 状态
            // 缓存已更新, 上层观测 API (G_IPV4/G_GATEWAY/G_DNS) 通过
            // 现有路径 (init_sockets / poll_network) 同步.
            FIRST_DECONFIG.store(false, Ordering::Release);
            let _ = ipv4; // 占位: W4.3+ 阶段从 Bound 还原 iface 配置
            crate::kernel::framework::net::NET_CONFIGURED.store(true, Ordering::Release);
            raw::klog_msg("DHCP configured (cached)");
        }
        // Discovering / Requesting / Renewing / Failed: 中间状态, 暂不处理
        _ => {}
    }
}

// ============================================================================
// 网络轮询 (统一入口，与具体网卡无关)
//
// 使用 NET_STATE.try_lock() 确保互斥访问。
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
pub unsafe fn poll_network() { unsafe {
    let _guard = match NET_STATE.try_lock() {
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
        if raw::fd_type(fd) == 0 {
            continue;
        }
        // 用 smoltcp can_send / can_recv 推断 wake 原因. socket_set 访问
        // 仍在 NET_STATE 锁保护下 (try_wake 内部 lock 仅保护自身 pending 标记,
        // 与 smoltcp 状态机无关).
        let reason = if let Some(handle) = raw::socket_handle(fd) {
            let can_read = match raw::fd_type(fd) {
                1 => sockets.get::<tcp::Socket>(handle).can_recv(),
                2 => sockets.get::<udp::Socket>(handle).can_recv(),
                _ => false,
            };
            let can_write = match raw::fd_type(fd) {
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
}}

// ============================================================================
// 多网卡探测 (按优先级依次尝试)
// ============================================================================

/// # Safety
///
/// - 在网络子系统初始化入口被调用, 期间无其他并发探测
/// - 依赖的 chitin/driver 框架 (`Driver::init`) 自身保证设备独占
#[cfg(not(feature = "kernel_test"))]
unsafe fn nic_probe_all() -> Option<ChitinNetDevice> { unsafe {
    // I-53 修复: 去除编译时架构互斥, 双架构二进制按运行时探测顺序
    // 尝试 e1000 (PCI 设备) 与 virtio-net (MMIO 设备). 两者驱动代码
    // 均架构无关, 仅依赖 IoMem / PCI 抽象. QEMU 配置决定哪一个会成功.
    //
    // 探测顺序固定: e1000 -> virtio-net. 真实硬件 (e.g. PC 上) e1000
    // 优先; QEMU virt 上 e1000 探测返回非 0 走 fallthrough 到 virtio.
    //
    // 失败: 全部探测返回非 0 / Box::into_raw 失败 / Driver::init 失败.

    // 1) e1000 探测 (PCI 设备, 走 PCI 总线)
    // aarch64: e1000_probe() 内部安全返回 -1 (无 PCI ECAM)
    {
        let probe_result = crate::kernel::framework::driver::e1000_probe();
        if probe_result == 0 {
            let mut dev = crate::kernel::framework::driver::e1000_take_device()?;
            if crate::kernel::framework::driver::Driver::init(&mut *dev).is_err() {
                raw::klog_err("e1000: hardware init failed");
                return None;
            }
            let mac = dev.mac();
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
}}

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
unsafe fn net_save() { unsafe {
    use core::sync::atomic::Ordering;
    use crate::kernel::framework::net::save as snap;

    let _guard = NET_STATE.lock();

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
            s.fd_types[i] = raw::fd_type(i);
            s.fd_handles[i] = match raw::socket_handle(i) {
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
}}

/// SocketHandle → u32 (smoltcp SocketHandle 是 `pub struct SocketHandle(usize)` 单字段
/// Copy newtype, 用 transmute_copy 替代 transmute: 编译期强制 size 匹配, 不依赖
/// repr(transparent) 假设).
#[inline]
fn as_u32_handle(h: smoltcp::iface::SocketHandle) -> u32 {
    // SAFETY: smoltcp::iface::SocketHandle 是单字段 Copy tuple struct (字段类型 usize),
    //         size_of::<SocketHandle>() == size_of::<usize>() 编译期由 transmute_copy 强制.
    //         不要求 repr(transparent) 假设, 避免 W5 记录的 transmute UB 风险.
    let raw: usize = unsafe { core::mem::transmute_copy(&h) };
    raw as u32
}

/// u32 → SocketHandle (作为 `as_u32_handle` 的 companion helper).
///
/// # Safety
///
/// 调用方必须保证 `raw` 是同构 smoltcp 版本下 `as_u32_handle` 的输出值;
/// 跨 smoltcp 版本混用会破坏 SocketSet 索引语义. 0 是 INVALID 句柄,
/// 不应被分配, 但 `SocketSet` 内部允许 0 (因为 `add` 一定返回非零索引).
#[inline]
unsafe fn smol_handle_from_u32(raw: u32) -> smoltcp::iface::SocketHandle {
    // SAFETY: 同 `as_u32_handle`, 字段类型 usize, transmute_copy 安全.
    let raw_usize = raw as usize;
    unsafe { core::mem::transmute_copy::<usize, smoltcp::iface::SocketHandle>(&raw_usize) }
}

/// # Safety
///
/// - 调用方须确保无其他线程持有 socket fd (例如文件系统已卸载完毕)
/// - 必须在关中断上下文执行, NET_LOCK 由本函数获取
///
/// SAFETY: 见上方 # Safety 章节, 调用方保证 socket fd 已无人持有 + 关中断; NET_LOCK 由本函数内部获取
unsafe fn net_restore() { unsafe {
    use core::sync::atomic::Ordering;
    use crate::kernel::framework::net::save as snap;

    // 1. 复位状态机
    {
        let _guard = NET_STATE.lock();
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
            let _guard = NET_STATE.lock();
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
        let _guard = NET_STATE.lock();
        for i in 0..MAX_SM_FD {
            raw::set_fd_type(i, saved.fd_types[i]);
            let handle = if saved.fd_handles[i] == u32::MAX {
                None
            } else {
                let raw_handle = saved.fd_handles[i];
                // SAFETY: `raw_handle` 是 `as_u32_handle` 持久化的同构 smoltcp 句柄;
                //         smol_handle_from_u32 用 transmute_copy 安全重建.
                Some(unsafe { smol_handle_from_u32(raw_handle) })
            };
            raw::set_socket_handle(i, handle);
        }
        SOCKETS_INITIALIZED.store(saved.sockets_initialized, Ordering::Release);
    }

    crate::arch!(interrupt_enable());
    raw::klog_msg("--- Network Recovered ---");
    snap::clear();
}}

/// # Safety
///
/// - 调用方须确保无其他线程持有 socket fd (例如文件系统已卸载完毕)
unsafe fn net_reset() {
    let _guard = NET_STATE.lock();

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

#[unsafe(no_mangle)]
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
            let _guard = NET_STATE.lock();
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
            let _guard = NET_STATE.lock();
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
            let _guard = NET_STATE.lock();
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

        // 注册网络 softirq 处理程序
        crate::kernel::framework::irq::open_softirq(
            crate::kernel::framework::irq::SoftirqVec::NetRx,
            net_rx_softirq_handler,
        );
        crate::kernel::framework::irq::open_softirq(
            crate::kernel::framework::irq::SoftirqVec::NetTx,
            net_tx_softirq_handler,
        );
    }
}

/// NetRx softirq 处理程序 — 网络包接收延迟处理
fn net_rx_softirq_handler() {
    // 当前 smoltcp 集成使用 poll 模式, 包处理在 poll_network() 中完成.
    // 此 handler 为多核 + 中断驱动模式预留.
    // TODO: 待 NAPI/中断驱动模式启用后, 此处实现 skb 投递到 smoltcp.
}

/// NetTx softirq 处理程序 — 网络发送完成回收
fn net_tx_softirq_handler() {
    // 当前发送通过 smoltcp 直接完成, 无异步发送队列.
    // 此 handler 为多核 + DMA 完成中断模式预留.
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qx_net_start_dhcp() -> i32 { unsafe {
    if !crate::kernel::framework::net::NET_READY.load(Ordering::Acquire) {
        return -1;
    }
    poll_network();
    0
}}

/// 设置静态 IP (x.x.x.x/prefix, gateway)
///
/// 格式: "10.0.2.15/24,10.0.2.2"
/// 返回 0 成功, -1 失败
///
/// # Safety
/// - `cidr_str` 与 `gw_str` 必须是有效的 C 字符串指针 (NUL 终止),
///   指向的内存必须在调用期间保持有效。
/// - 调用方保证 NET 已初始化。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qx_net_static_ip(cidr_str: *const u8, gw_str: *const u8) -> i32 { unsafe {
    if !crate::kernel::framework::net::NET_READY.load(Ordering::Acquire) {
        return -1;
    }

    let _guard = NET_STATE.lock();

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
}}

// ============================================================================
// Socket 公共 API
// ============================================================================

// POSIX errno 常量 (i32)
const E_BADF: i32 = 9;
const E_AGAIN: i32 = 11;
const E_NOMEM: i32 = 12;
const E_FAULT: i32 = 14;
const E_INVAL: i32 = 22;
const E_NFILE: i32 = 23;
const E_NOTSUPP: i32 = 95;
const E_AFNOSUPPORT: i32 = 97;
const E_ADDRINUSE: i32 = 98;
const E_CONNRESET: i32 = 104;
const E_NOTCONN: i32 = 107;
const E_CONNREFUSED: i32 = 111;
const E_NODEV: i32 = 19;

/// POSIX `socket(domain, type, protocol)` 内核实现。
///
/// # Safety
/// - 由 `sys_socket` 系统调用分发, 参数由 syscall 层校验 (cred 检查)。
/// - 必须 NET_LOCK 持有。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_socket(domain: i32, sock_type: i32, _protocol: i32) -> i32 { unsafe {
    if !is_network_initialized() {
        return -E_NODEV;
    }

    let _guard = NET_STATE.lock();

    // I-47: 检查活动 socket 上限 (≤ G_MAX_SOCKETS ≤ MAX_SOCKETS).
    // 运行时可通过 set_max_sockets 调整, 编译期上限 MAX_SOCKETS 静态保证.
    let active: usize = (0..MAX_SM_FD).filter(|&i| raw::fd_type(i) != 0).count();
    if active >= get_max_sockets() {
        return -E_NFILE;
    }

    // V2: 使用集中分配器获取 FD
    let fd = match crate::kernel::services::proc::fd_alloc::alloc_fd(
        crate::kernel::services::proc::fd_alloc::FdSubsystem::Smoltcp,
    ) {
        Some(f) => f,
        None => return -E_NFILE,
    };
    let fd_idx = fd as usize;

    // REVAL-W W4.2.3.3 (2026-06-25): sm_socket 路径迁移到 raw::socket_open_stub.
    // 删除 75 行重复 socket 构造代码, 统一走 raw 模块 (与 SmoltcpNetStack 共享).
    // 0 行为变更: sm_socket 仍返回 fd, k_malloc 失败仍返回 -E_NOMEM.
    if domain == 2 && sock_type == 1 {
        // TCP — 委托 raw::socket_open_stub
        let sockets = &mut *socket_set();
        let kind = crate::kernel::framework::net::iface_trait::SocketKind::Tcp;
        if raw::socket_open_stub(sockets, kind, fd_idx).is_none() {
            return -E_NOMEM;
        }
        fd
    } else if domain == 2 && sock_type == 2 {
        // UDP — 委托 raw::socket_open_stub
        let sockets = &mut *socket_set();
        let kind = crate::kernel::framework::net::iface_trait::SocketKind::Udp;
        if raw::socket_open_stub(sockets, kind, fd_idx).is_none() {
            return -E_NOMEM;
        }
        fd
    } else {
        -E_AFNOSUPPORT
    }
}}

// ============================================================================
// W4.4: smoltcp wire 类型 ↔ NetStack trait 抽象类型的翻译 helper
//
// 仅在 framework 边界 (raw::qemu_net_skel 一类适配器, 或 boot 阶段从 MAC/IP
// 字面量构造 Interface::update_ip_addrs) 使用, services 层访问地址一律走
// Ipv4Addr / Ipv4Cidr / NetEndpoint. 此处的 smoltcp wire 类型导入仅服务于
// 翻译函数本身.
//
// ## 与 W3.2 SmoltcpNetStack 的职责划分
//
// - SmoltcpNetStack::init / socket_open / dhcp_state: 服务层 trait API,
//   不暴露 smoltcp wire 类型.
// - 本模块的 wire_to_* / *_to_wire: 框架层内部适配器, 仅在
//   qemu_net_skel / update_ip_addrs 等 framework 内部使用.
// ============================================================================

/// 把 trait 抽象的 `Ipv4Addr` 翻译成 smoltcp 的 `Ipv4Address`.
#[inline(always)]
pub(crate) fn wire_to_smol_v4(a: crate::kernel::framework::net::iface_trait::Ipv4Addr) -> Ipv4Address {
    let o = a.octets();
    Ipv4Address::new(o[0], o[1], o[2], o[3])
}

/// 把 trait 抽象的 `NetEndpoint` 翻译成 smoltcp 的 `IpEndpoint`.
#[inline]
pub(crate) fn endpoint_to_smol(
    e: crate::kernel::framework::net::iface_trait::NetEndpoint,
) -> IpEndpoint {
    IpEndpoint {
        addr: IpAddress::Ipv4(wire_to_smol_v4(e.addr)),
        port: e.port,
    }
}

#[repr(C)]
struct SockaddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

/// 从 sockaddr_in C 结构体解析 IPv4 端点 (W4.4 trait 翻译版本).
///
/// 解析后**先**填充 trait 抽象的 `NetEndpoint`, 调用方按需通过
/// `endpoint_to_smol()` 翻译回 smoltcp `IpEndpoint`. 这一层翻译是
/// W4.4 目标: 让 sock 路径不直接持有 smoltcp wire 类型.
///
/// # Safety
/// `addr` 必须指向有效的 `SockaddrIn` 结构体, 至少含 8 字节已初始化。
unsafe fn parse_ipv4_endpoint_trait(
    addr: *const u8,
) -> Option<crate::kernel::framework::net::iface_trait::NetEndpoint> { unsafe {
    if addr.is_null() {
        return None;
    }
    let sin = &*(addr as *const SockaddrIn);
    if sin.sin_family != 2 {
        return None;
    }
    let octets = sin.sin_addr;
    let port = u16::from_be(sin.sin_port);
    Some(crate::kernel::framework::net::iface_trait::NetEndpoint::new(
        crate::kernel::framework::net::iface_trait::Ipv4Addr::from_octets(octets),
        port,
    ))
}}

/// 旧版 `parse_ipv4_endpoint` 包装, 保持向后兼容 (smoltcp wire 类型返回).
///
/// 内部走 trait 翻译路径, 调用方应优先改用 `parse_ipv4_endpoint_trait`.
///
/// # Safety
/// 同 `parse_ipv4_endpoint_trait`.
unsafe fn parse_ipv4_endpoint(addr: *const u8) -> Option<IpEndpoint> { unsafe {
    parse_ipv4_endpoint_trait(addr).map(endpoint_to_smol)
}}

/// POSIX `bind(fd, addr, addrlen)` 内核实现。
///
/// # Safety
/// - `addr` 必须是有效的 sockaddr 指针, 含 `_addrlen` 字节已初始化。
/// - 由 `sys_bind` 系统调用分发, 调用方验证权限。
/// - NET_LOCK 持有。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_bind(fd: i32, addr: *const u8, _addrlen: u32) -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
        return -E_BADF;
    }
    let handle = match raw::socket_handle(fd as usize) {
        Some(h) => h,
        None => return -E_BADF,
    };

    let sockets = &mut *socket_set();

    match raw::fd_type(fd as usize) {
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
}}

/// POSIX `listen(fd, backlog)` 内核实现。
///
/// # Safety
/// NET_LOCK 持有; 由 `sys_listen` 分发, 调用方验证权限。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_listen(fd: i32, _backlog: i32) -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
        return -E_BADF;
    }
    let handle = match raw::socket_handle(fd as usize) {
        Some(h) => h,
        None => return -E_BADF,
    };

    if raw::fd_type(fd as usize) != 1 {
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
}}

/// POSIX `accept(fd, addr, addrlen)` 内核实现。
///
/// # Safety
/// - `addr`/`_addrlen` 必须是有效的 sockaddr 指针 (此处忽略)。
/// - NET_LOCK 持有; 由 `sys_accept` 分发, 调用方验证权限。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_accept(fd: i32, _addr: *mut u8, _addrlen: *mut u32) -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
        return -E_BADF;
    }
    let handle = match raw::socket_handle(fd as usize) {
        Some(h) => h,
        None => return -E_BADF,
    };

    if raw::fd_type(fd as usize) != 1 {
        return -E_NOTSUPP;
    }

    let sockets = &mut *socket_set();
    let sock = sockets.get_mut::<tcp::Socket>(handle);

    if sock.is_active() {
        fd
    } else {
        -E_AGAIN
    }
}}

/// POSIX `connect(fd, addr, addrlen)` 内核实现。
///
/// # Safety
/// `addr` 必须指向有效的 sockaddr 结构, 至少 `_addrlen` 字节。
/// NET_LOCK 持有。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_connect(fd: i32, addr: *const u8, _addrlen: u32) -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
        return -E_BADF;
    }
    let handle = match raw::socket_handle(fd as usize) {
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

    if raw::fd_type(fd as usize) != 1 {
        return -E_NOTSUPP;
    }

    let stack = match raw::stack_mut() {
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
}}

/// POSIX `send(fd, buf, len, flags)` 内核实现。
///
/// # Safety
/// `buf` 必须指向至少 `len` 字节的有效可读内存, 内存必须在调用期间保持有效。
/// NET_LOCK 持有; 由 `sys_send` 分发, cred 校验已通过。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_send(fd: i32, buf: *const u8, len: u32, _flags: i32) -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
        return -E_BADF;
    }
    let handle = match raw::socket_handle(fd as usize) {
        Some(h) => h,
        None => return -E_BADF,
    };
    if buf.is_null() || len == 0 {
        return -E_INVAL;
    }

    let sockets = &mut *socket_set();
    let data = core::slice::from_raw_parts(buf, len as usize);

    match raw::fd_type(fd as usize) {
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
}}

/// POSIX `recv(fd, buf, len, flags)` 内核实现。
///
/// # Safety
/// `buf` 必须指向至少 `len` 字节的有效可写内存, 内存必须在调用期间保持有效。
/// NET_LOCK 持有; 由 `sys_recv` 分发。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_recv(fd: i32, buf: *mut u8, len: u32, _flags: i32) -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
        return -E_BADF;
    }
    let handle = match raw::socket_handle(fd as usize) {
        Some(h) => h,
        None => return -E_BADF,
    };
    if buf.is_null() || len == 0 {
        return -E_INVAL;
    }

    let sockets = &mut *socket_set();
    let data = core::slice::from_raw_parts_mut(buf, len as usize);

    match raw::fd_type(fd as usize) {
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
}}

/// POSIX `sendto(fd, buf, len, flags, addr, addrlen)` 内核实现。
///
/// # Safety
/// `buf`/`addr` 必须是有效指针, 内存至少含 `len`/`_addrlen` 字节。
/// NET_LOCK 持有; 由 `sys_sendto` 分发。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_sendto(
    fd: i32,
    buf: *const u8,
    len: u32,
    _flags: i32,
    addr: *const u8,
    _addrlen: u32,
) -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
        return -E_BADF;
    }
    let handle = match raw::socket_handle(fd as usize) {
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

    match raw::fd_type(fd as usize) {
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
}}

/// POSIX `recvfrom(fd, buf, len, flags, addr, addrlen)` 内核实现。
///
/// # Safety
/// `buf` 必须是有效可写指针, 至少 `len` 字节; `addr`/`_addrlen` 可选地写入对端地址。
/// NET_LOCK 持有; 由 `sys_recvfrom` 分发。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_recvfrom(
    fd: i32,
    buf: *mut u8,
    len: u32,
    _flags: i32,
    _addr: *mut u8,
    _addrlen: *mut u32,
) -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
        return -E_BADF;
    }
    let handle = match raw::socket_handle(fd as usize) {
        Some(h) => h,
        None => return -E_BADF,
    };
    if buf.is_null() || len == 0 {
        return -E_INVAL;
    }

    let sockets = &mut *socket_set();
    let data = core::slice::from_raw_parts_mut(buf, len as usize);

    match raw::fd_type(fd as usize) {
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
}}

/// POSIX `sendmsg(fd, msghdr, flags)` 内核实现 (SG 拼接, 栈缓冲 4KB 上限).
///
/// # Safety
/// `msg` 必须是有效用户指针, 含完整 `Msghdr { msg_iov, msg_iovlen, ... }`.
/// 调用方 (services) 须先校验可读范围.
/// NET_LOCK 持有; 由 `sys_sendmsg` 分发.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_sendmsg(fd: i32, msg: *const u8, _flags: i32) -> i32 { unsafe {
    if msg.is_null() {
        return -E_FAULT;
    }
    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
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
}}

/// POSIX `recvmsg(fd, msghdr, flags)` 内核实现 (SG 拆分, 栈缓冲 4KB 上限).
///
/// # Safety
/// `msg` 必须是有效可写用户指针, services 校验.
/// NET_LOCK 持有; 由 `sys_recvmsg` 分发.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_recvmsg(fd: i32, msg: *mut u8, _flags: i32) -> i32 { unsafe {
    if msg.is_null() {
        return -E_FAULT;
    }
    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
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
}}

/// POSIX `close(fd)` 内核实现。
///
/// # Safety
/// NET_LOCK 持有; 由 `sys_close` 分发, cred 校验已通过。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_close(fd: i32) -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
        return -E_BADF;
    }
    let handle = match raw::socket_handle(fd as usize) {
        Some(h) => h,
        None => return -E_BADF,
    };

    let stype = raw::fd_type(fd as usize);
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
    if !raw::tcp_rx_buf(fd as usize).is_null() {
        crate::kernel::framework::mm::k_free(raw::tcp_rx_buf(fd as usize));
        raw::set_tcp_rx_buf(fd as usize, core::ptr::null_mut());
    }
    if !raw::tcp_tx_buf(fd as usize).is_null() {
        crate::kernel::framework::mm::k_free(raw::tcp_tx_buf(fd as usize));
        raw::set_tcp_tx_buf(fd as usize, core::ptr::null_mut());
    }
    if !raw::udp_rx_buf(fd as usize).is_null() {
        crate::kernel::framework::mm::k_free(raw::udp_rx_buf(fd as usize));
        raw::set_udp_rx_buf(fd as usize, core::ptr::null_mut());
    }
    if !raw::udp_tx_buf(fd as usize).is_null() {
        crate::kernel::framework::mm::k_free(raw::udp_tx_buf(fd as usize));
        raw::set_udp_tx_buf(fd as usize, core::ptr::null_mut());
    }
    raw::set_socket_handle(fd as usize, None);
    raw::set_fd_type(fd as usize, 0);
    0
}}

/// POSIX `setsockopt` 内核实现 (当前空操作占位)。
///
/// v2: 识别 SO_PASSCRED (level=SOL_SOCKET=1, optname=SO_PASSCRED=16).
/// 路由到 UDS 服务层 (uds_setsockopt).
/// 其他 (level, optname): 0 (no-op).
///
/// # Safety
/// `_optval` 必须是有效指针, 含 `_optlen` 字节 (此处忽略)。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_setsockopt(
    _fd: i32,
    _level: i32,
    _optname: i32,
    _optval: *const u8,
    _optlen: u32,
) -> i32 { unsafe {
    // v2 SO_PASSCRED 路由: level==1 (SOL_SOCKET), optname==16 (SO_PASSCRED)
    if _level == 1 && _optname == 16 {
        if _optlen < 4 {
            return -22; // EINVAL
        }
        let val = core::ptr::read_unaligned(_optval as *const i32);
        return uds_svc::uds_setsockopt(_fd, val != 0);
    }
    0
}}

/// POSIX `getsockopt` 内核实现 (当前空操作占位)。
///
/// # Safety
/// `_optval` 必须是有效可写指针, `_optlen` 必须是有效可写 u32 指针 (此处忽略)。
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_getsockname(fd: i32, addr: *mut u8, addrlen: *mut u32) -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
        return -E_BADF;
    }
    let handle = match raw::socket_handle(fd as usize) {
        Some(h) => h,
        None => return -E_BADF,
    };
    if addr.is_null() || addrlen.is_null() {
        return -E_INVAL;
    }
    let stype = raw::fd_type(fd as usize);
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
}}

/// POSIX `getpeername(fd, addr, addrlen)` 内核实现。
///
/// 真实实现: 写回 socket 的 remote endpoint 到 `*addr` (TCP 需已 connect).
///
/// # Safety
/// - `addr` 必须是可写 sockaddr 指针, 至少 `_addrlen` 字节.
/// - `_addrlen` 必须是可写 u32 指针 (写回实际长度).
/// - NET_LOCK 持有; 由 `sys_getpeername` 分发.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_getpeername(fd: i32, addr: *mut u8, addrlen: *mut u32) -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    if fd < 0 || fd as usize >= MAX_SM_FD || raw::fd_type(fd as usize) == 0 {
        return -E_BADF;
    }
    let handle = match raw::socket_handle(fd as usize) {
        Some(h) => h,
        None => return -E_BADF,
    };
    if addr.is_null() || addrlen.is_null() {
        return -E_INVAL;
    }
    let stype = raw::fd_type(fd as usize);
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
}}

/// 轮询所有 socket 状态 (驱动 `select/poll` 内核实现)。
///
/// # Safety
/// NET_LOCK 持有; 由 `sys_poll`/`sys_select` 分发。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sm_poll_sockets() -> i32 { unsafe {
    let _guard = NET_STATE.lock();

    let sockets = &mut *socket_set();
    process_dhcp_events(sockets);

    for i in 0..MAX_SM_FD {
        if raw::fd_type(i) != 1 {
            continue;
        }
        if let Some(handle) = raw::socket_handle(i) {
            let _sock = sockets.get_mut::<tcp::Socket>(handle);
        }
    }
    0
}}

// ============================================================================
// 公共 API
// ============================================================================

// I-47: FD 表容量, 与 MAX_SOCKETS 对齐 (每个 FD 对应一个 smoltcp socket).
// TD-02: 基址与容量改由 `framework::proc::FdPlan::SMOLTCP` 单一来源; 容量现从 FdRange.capacity 派生.
const MAX_SM_FD: usize = crate::kernel::framework::proc::FdPlan::SMOLTCP.capacity as usize;
const TCP_BUF_SIZE: usize = 4096;
const UDP_BUF_SIZE: usize = 2048;
const UDP_META_COUNT: usize = 4;

// REVAL-W W4.2.3.1 (2026-06-25): 总槽位数 = sm_socket 范围 (0..MAX_SM_FD) +
// SmoltcpNetStack 范围 (MAX_SM_FD..TOTAL_SLOTS). 范围严格隔离, 不冲突.
//
// 索引空间分配:
//   - 0..MAX_SM_FD:           sm_socket fd (不变)
//   - MAX_SM_FD..TOTAL_SLOTS: SmoltcpNetStack (新增, 留给 W4.2.3.2+ 整合)
//
// BSS 增长: 8 张数组 × (TOTAL_SLOTS - MAX_SM_FD) 槽位. MAX_SOCKETS=1024
// 时增长约 169 KB (主要是 UDP_RX_METAS / UDP_TX_METAS).
const TOTAL_SLOTS: usize = MAX_SM_FD + MAX_SOCKETS;

// TD-05: 8 张 smoltcp 大表, 现已合并到 NetState 结构中 (由 NET_STATE IrqSpinLock 保护).
// 原 Align64 对齐优化已通过 NetState 内部字段布局保留.
// TCP buffer storage (per fd): 由 k_malloc 按需分配, close 时 k_free 归还.

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
    HostEntry { name: "queenx-gateway",    ip: types::FALLBACK_GATEWAY },
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
    let _guard = NET_STATE.lock();
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
    let _guard = NET_STATE.lock();

    G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);

    raw::clear_all();
    SOCKETS_INITIALIZED.store(false, Ordering::Release);
}

// ============================================================================
// REVAL-W W4.2.3.4 步骤 2: SmoltcpNetStack 桥接 safe API (init 模块顶层)
//
// SmoltcpNetStack (services 层) 调用本模块的 safe wrapper 来实际构造
// smoltcp socket. 内部 unsafe 块 (raw::socket_set + raw::socket_open_stub
// + transmute SocketHandle → u32) 封装在 framework 层, services 层调用
// 时无 unsafe 暴露.
// ============================================================================

/// SmoltcpNetStack::socket_open 的 safe wrapper (W4.2.3.4 步骤 2).
///
/// ## 调用方契约
///
/// - `kind`: 要创建的 socket 类型 (Tcp/Udp/...)
/// - `slot_idx`: 槽位索引, 必须在 `[MAX_SM_FD, TOTAL_SLOTS)` 范围
///   (SmoltcpNetStack 专属范围, 不与 sm_socket 冲突)
///
/// ## 返回
///
/// - `Some(u32)`: smoltcp handle (用于 smol_socket_get)
/// - `None`: 创建失败 (k_malloc 失败 / 槽位已占用 / slot_idx 越界)
pub fn smoltcp_net_stack_socket_open(
    kind: crate::kernel::framework::net::iface_trait::SocketKind,
    slot_idx: usize,
) -> Option<u32> {
    // SAFETY: 调用方持有 NET_LOCK, socket_set() 返回的指针由 init_sockets
    // 单次初始化, SOCKET_SET MaybeUninit 区域独占.
    let sockets = unsafe { &mut *raw::socket_set() };
    let smol_handle = raw::socket_open_stub(sockets, kind, slot_idx)?;
    // W5 transmute_copy 路径: 复用 `as_u32_handle` helper (编译期强制
    // size 匹配, 不依赖 SocketHandle repr 假设). SocketSet 容量上限
    // (MAX_SOCKETS = 1024) 远低于 u32::MAX, usize → u32 截断安全.
    Some(as_u32_handle(smol_handle))
}

/// SmoltcpNetStack 专属范围的 smol 槽位基址 (W4.2.3.4 步骤 2).
///
/// 返回 `MAX_SM_FD` (即 SmoltcpNetStack 范围的起始索引). services 层
/// SmoltcpNetStack::socket_open 内部 `smol_slot_idx = slot_base() + handle_map_idx`.
pub fn smoltcp_net_stack_slot_base() -> usize {
    MAX_SM_FD
}

/// SmoltcpNetStack::poll 的 safe wrapper (W4.2.3.4).
///
/// 委托给 `raw::smoltcp_net_stack_poll`, 内部持有 NET_LOCK 并调用
/// smoltcp `Interface::poll` + `process_dhcp_events`.
pub fn smoltcp_net_stack_poll() -> crate::kernel::framework::net::iface_trait::PollOutcome {
    raw::smoltcp_net_stack_poll()
}

/// SmoltcpNetStack::close 的 safe wrapper (W4.2.3.4).
///
/// 关闭 SmoltcpNetStack 范围内的 smoltcp socket, 释放 buffer.
/// 委托给 `raw::smoltcp_net_stack_socket_close`.
pub fn smoltcp_net_stack_close(slot_idx: usize) {
    raw::smoltcp_net_stack_socket_close(slot_idx);
}

// ============================================================================
// 特权子模块 (Framekernel raw): 集中 static mut 访问
// ============================================================================

pub(crate) mod raw {
    use super::*;

    /// 获取 NetState 可变引用 (调用方必须持有 NET_STATE 锁).
    ///
    /// # Safety
    ///
    /// 调用方必须持有 `NET_STATE` 的锁 (通过 `lock()` 或 `try_lock()`).
    /// 返回的引用生命周期为 `'static` (因底层数据在 `static NET_STATE` 中),
    /// 但调用方不得在锁释放后继续使用.
    #[inline(always)]
    unsafe fn state() -> &'static mut NetState {
        // SAFETY: NET_STATE 是 static, 数据永不移动;
        // 调用方持有锁保证互斥访问.
        unsafe { &mut *NET_STATE.data_ptr() }
    }

    /// 安全访问 stack (Framekernel 集中 unsafe 边界)
    pub fn stack_mut() -> Option<&'static mut NetworkStack> {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().stack.as_mut() }
    }

    /// 安全访问 device
    pub fn device_mut() -> Option<&'static mut ChitinNetDevice> {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().device.as_mut() }
    }

    /// 安全设置 device
    pub fn set_device(d: Option<ChitinNetDevice>) {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().device = d; }
    }

    /// 安全设置 stack
    pub fn set_stack(s: Option<NetworkStack>) {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().stack = s; }
    }

    /// 安全读取 dhcp_handle
    pub fn dhcp_handle() -> Option<SocketHandle> {
        // SAFETY: SocketHandle 是 Copy, 调用方持有锁.
        unsafe { state().dhcp_handle }
    }

    /// 安全设置 dhcp_handle
    pub fn set_dhcp_handle(h: Option<SocketHandle>) {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().dhcp_handle = h; }
    }

    /// 安全清空网络全局状态
    pub fn clear_all() {
        // SAFETY: 调用方持有 NET_STATE 锁, 串行重置流程.
        let s = unsafe { state() };
        s.device = None;
        s.stack = None;
        s.dhcp_handle = None;
    }

    // ========================================================================
    // FD_TYPES / SOCKET_TABLE / buffer accessor 函数
    //
    // 集中访问 NetState 中的 FD 表、socket 表和 buffer 指针数组.
    // 所有函数要求调用方持有 NET_STATE 锁.
    // ========================================================================

    /// 读取 fd 类型 (0=free, 1=tcp, 2=udp)
    pub fn fd_type(fd: usize) -> u8 {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().fd_types[fd] }
    }

    /// 写入 fd 类型
    pub fn set_fd_type(fd: usize, val: u8) {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().fd_types[fd] = val; }
    }

    /// 读取 socket handle
    pub fn socket_handle(fd: usize) -> Option<SocketHandle> {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().socket_table[fd] }
    }

    /// 写入 socket handle
    pub fn set_socket_handle(fd: usize, val: Option<SocketHandle>) {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().socket_table[fd] = val; }
    }

    /// 读取 TCP RX buffer 指针
    pub fn tcp_rx_buf(fd: usize) -> *mut u8 {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().tcp_rx_bufs[fd] }
    }

    /// 写入 TCP RX buffer 指针
    pub fn set_tcp_rx_buf(fd: usize, val: *mut u8) {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().tcp_rx_bufs[fd] = val; }
    }

    /// 读取 TCP TX buffer 指针
    pub fn tcp_tx_buf(fd: usize) -> *mut u8 {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().tcp_tx_bufs[fd] }
    }

    /// 写入 TCP TX buffer 指针
    pub fn set_tcp_tx_buf(fd: usize, val: *mut u8) {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().tcp_tx_bufs[fd] = val; }
    }

    /// 读取 UDP RX buffer 指针
    pub fn udp_rx_buf(fd: usize) -> *mut u8 {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().udp_rx_bufs[fd] }
    }

    /// 写入 UDP RX buffer 指针
    pub fn set_udp_rx_buf(fd: usize, val: *mut u8) {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().udp_rx_bufs[fd] = val; }
    }

    /// 读取 UDP TX buffer 指针
    pub fn udp_tx_buf(fd: usize) -> *mut u8 {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().udp_tx_bufs[fd] }
    }

    /// 写入 UDP TX buffer 指针
    pub fn set_udp_tx_buf(fd: usize, val: *mut u8) {
        // SAFETY: 调用方持有 NET_STATE 锁.
        unsafe { state().udp_tx_bufs[fd] = val; }
    }

    /// 读取 UDP RX metadata 数组 (可变借用, 用于 PacketBuffer 构造)
    ///
    /// # Safety
    ///
    /// 调用方持有 NET_STATE 锁; 返回的引用仅在本次 socket 构造期间有效.
    pub unsafe fn udp_rx_meta(fd: usize) -> &'static mut [udp::PacketMetadata; UDP_META_COUNT] {
        // SAFETY: 调用方持有 NET_STATE 锁, 数据在 static 中.
        unsafe { &mut state().udp_rx_metas[fd] }
    }

    /// 读取 UDP TX metadata 数组 (可变借用, 用于 PacketBuffer 构造)
    ///
    /// # Safety
    ///
    /// 调用方持有 NET_STATE 锁; 返回的引用仅在本次 socket 构造期间有效.
    pub unsafe fn udp_tx_meta(fd: usize) -> &'static mut [udp::PacketMetadata; UDP_META_COUNT] {
        // SAFETY: 调用方持有 NET_STATE 锁, 数据在 static 中.
        unsafe { &mut state().udp_tx_metas[fd] }
    }

    /// 安全获取 SocketSet 指针 (保留为 static mut, 自引用结构)
    pub fn socket_set() -> *mut SocketSet<'static> {
        // SAFETY: SOCKET_SET 在 init_sockets 后已初始化, 调用方在 NET_STATE 锁下.
        unsafe { SOCKET_SET.as_mut_ptr() }
    }

    /// 安全初始化 sockets
    pub fn init_sockets() {
        // SAFETY: 调用方持有 NET_STATE 锁, 单次初始化.
        unsafe { super::init_sockets() }
    }

    /// 安全处理 DHCP 事件
    pub fn process_dhcp_events(sockets: &mut SocketSet<'_>) {
        // SAFETY: 调用方持有 NET_STATE 锁, sockets 来自本模块的 socket_set().
        unsafe { super::process_dhcp_events(sockets) }
    }

    // ========================================================================
    // REVAL-W W4.2 桥接 raw helpers (2026-06-25)
    //
    // 为 SmoltcpNetStack (W3.2) 提供 smoltcp 实际操作入口. 桥接方案:
    //   - SmoltcpNetStack 是 services 层 trait 翻译骨架, 不持 smoltcp 状态
    //   - 实际 smoltcp 操作由 init.rs (framework 层, 允许 unsafe) 提供
    //   - SmoltcpNetStack::socket_open 等方法内部委托给本 raw 模块
    //
    // 阶段 1 (W4.2.1): 声明函数签名 + 0 逻辑, 验证编译
    // 阶段 2 (W4.2.2): 实装 socket_close + dhcp_state 翻译
    // 阶段 3+ (W4.2.3+): 实装 socket_open (buffer 整合) + SmoltcpNetStack 改造
    // ========================================================================

    /// W4.2.2 DHCP 状态翻译的 prev state 缓存.
    ///
    /// 使用 `core::sync::atomic::AtomicU8` 持有 DhcpState 的 discriminant
    /// (枚举 tag). DhcpState::Bound 的 ipv4 + lease_expires_at 字段
    /// 用 `AtomicU32` (ipv4) + `AtomicU64` (lease_expires_at) 单独存储.
    ///
    /// ## 设计选择
    ///
    /// 不使用 `static mut` + 裸指针: Rust 2024 edition 启用了
    /// `invalid_reference_casting` lint, 编译失败. 不使用 `UnsafeCell<T>`
    /// 包装: `static` 要求 `Sync`, 而 `UnsafeCell<T>: Sync` 需要 `T: Send`,
    /// 但 `unsafe impl Send` 在 no_std 环境下行为不可靠.
    ///
    /// ## 同步策略
    ///
    /// 调用方需持有 NET_LOCK 互斥访问. 原子操作保证多线程可见性.
    /// 4 个原子 (tag, ipv4[4], lease_expires_at) 的"组合"通过 read 顺序保证
    /// 一致性 (看 acquire/release).
    ///
    /// ## 简化 (W4.2.2 阶段 1)
    ///
    /// 仅 AtomicU8 持有 tag. Bound 的额外数据 (ipv4, lease_expires_at) 通过
    /// G_IPV4 (AtomicU32) + 单独的 AtomicU64 持有. W4.2.2 阶段不实装完整
    /// 数据, 仅追踪 tag 转换.
    static PREV_DHCP_TAG: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

    /// W4.2.2 DHCP 状态 Bound 的 ipv4.
    static PREV_DHCP_IPV4: [core::sync::atomic::AtomicU8; 4] = [
        core::sync::atomic::AtomicU8::new(0),
        core::sync::atomic::AtomicU8::new(0),
        core::sync::atomic::AtomicU8::new(0),
        core::sync::atomic::AtomicU8::new(0),
    ];

    /// 实际打开一个 socket (W4.2.3.2 实装).
    ///
    /// 根据 `kind` 构造 smoltcp socket (Tcp/Udp), 加入 `sockets`, 记录 buffer
    /// 指针到 SOCKET_TABLE / TCP_RX_BUFS / TCP_TX_BUFS / UDP_RX_BUFS /
    /// UDP_TX_BUFS / FD_TYPES, 返回 smol_handle.
    ///
    /// ## 索引空间分配 (W4.2.3.1)
    ///
    /// `slot_idx` ∈ [0, TOTAL_SLOTS):
    /// - 0..MAX_SM_FD:           sm_socket 路径 (现有 sm_socket 调用)
    /// - MAX_SM_FD..TOTAL_SLOTS: SmoltcpNetStack 路径 (W4.2.4 整合后)
    ///
    /// 两个范围严格隔离, 不冲突.
    ///
    /// ## buffer 来源 (W4.2.3.2 实装)
    ///
    /// Tcp/Udp RX/TX buffer 走 `k_malloc` (slab), 与现有 sm_socket 路径一致.
    /// buffer 指针记入 TCP_RX_BUFS / TCP_TX_BUFS / UDP_RX_BUFS / UDP_TX_BUFS
    /// (索引 = slot_idx). close 时通过 socket_close_stub + sm_close 归还.
    ///
    /// ## 安全性
    ///
    /// buffer 'static 借用: smoltcp SocketSet<'static> 要求 socket 借用
    /// 'static. 我们用 `unsafe { core::slice::from_raw_parts_mut(ptr, size) }`
    /// 强制 'static (与现有 sm_socket 模式一致). 安全性依赖于:
    ///   - k_malloc 不会在进程生命周期内释放 (slab 进程级)
    ///   - socket_close 时通过 `k_free` 归还 (W4.2.3.3 迁移时实装)
    ///
    /// ## 简化 (W4.2.3.2 阶段)
    ///
    /// - 暂不实装 Icmp/Raw/Dhcpv4/Dns (返回 None)
    /// - sm_socket 路径暂不调用本函数 (W4.2.3.3 迁移)
    /// - SmoltcpNetStack 路径暂不调用本函数 (W4.2.3.4 整合)
    pub fn socket_open_stub(
        sockets: &mut SocketSet<'_>,
        kind: crate::kernel::framework::net::iface_trait::SocketKind,
        slot_idx: usize,
    ) -> Option<smoltcp::iface::SocketHandle> {
        use crate::kernel::framework::net::iface_trait::SocketKind;

        // SAFETY: 调用方持有 NET_STATE 锁, 整个函数体通过 raw accessor 访问 NetState.
        unsafe {
            // 1. 校验 slot_idx 范围
            if slot_idx >= TOTAL_SLOTS {
                return None;
            }
            // 2. 校验槽位空闲
            if socket_handle(slot_idx).is_some() {
                return None;
            }

            match kind {
                SocketKind::Tcp => {
                    // TD-07: TCP RX/TX 缓冲走 slab, 与 sm_socket 路径一致.
                    // SAFETY: k_malloc 在初始化后可用, 返回非空或 null. null 时立即返回 None.
                    let rx_ptr = crate::kernel::framework::mm::k_malloc(TCP_BUF_SIZE);
                    if rx_ptr.is_null() {
                        return None;
                    }
                    let tx_ptr = crate::kernel::framework::mm::k_malloc(TCP_BUF_SIZE);
                    if tx_ptr.is_null() {
                        crate::kernel::framework::mm::k_free(rx_ptr);
                        return None;
                    }
                    // SAFETY: rx_ptr/tx_ptr 来自 k_malloc(TCP_BUF_SIZE), 长度合法, 唯一别名.
                    //         'static 借用基于: slab 进程级 + 索引化生命周期管理.
                    let rx_slice =
                        core::slice::from_raw_parts_mut(rx_ptr, TCP_BUF_SIZE);
                    let tx_slice =
                        core::slice::from_raw_parts_mut(tx_ptr, TCP_BUF_SIZE);
                    let tcp_sock = smoltcp::socket::tcp::Socket::new(
                        smoltcp::socket::tcp::SocketBuffer::new(rx_slice),
                        smoltcp::socket::tcp::SocketBuffer::new(tx_slice),
                    );
                    let handle = sockets.add(tcp_sock);
                    set_socket_handle(slot_idx, Some(handle));
                    set_fd_type(slot_idx, 1);
                    set_tcp_rx_buf(slot_idx, rx_ptr);
                    set_tcp_tx_buf(slot_idx, tx_ptr);
                    Some(handle)
                }
                SocketKind::Udp => {
                    // TD-07: UDP RX/TX 缓冲走 slab. metas 仍静态 (16 KB).
                    let rx_ptr = crate::kernel::framework::mm::k_malloc(UDP_BUF_SIZE);
                    if rx_ptr.is_null() {
                        return None;
                    }
                    let tx_ptr = crate::kernel::framework::mm::k_malloc(UDP_BUF_SIZE);
                    if tx_ptr.is_null() {
                        crate::kernel::framework::mm::k_free(rx_ptr);
                        return None;
                    }
                    // SAFETY: 同 TCP 注释, 'static 借用基于 slab 进程级 + 索引化管理.
                    let rx_slice =
                        core::slice::from_raw_parts_mut(rx_ptr, UDP_BUF_SIZE);
                    let tx_slice =
                        core::slice::from_raw_parts_mut(tx_ptr, UDP_BUF_SIZE);
                    let rx_meta = udp_rx_meta(slot_idx);
                    let tx_meta = udp_tx_meta(slot_idx);
                    let udp_sock = smoltcp::socket::udp::Socket::new(
                        smoltcp::socket::udp::PacketBuffer::new(
                            &mut rx_meta[..],
                            rx_slice,
                        ),
                        smoltcp::socket::udp::PacketBuffer::new(
                            &mut tx_meta[..],
                            tx_slice,
                        ),
                    );
                    let handle = sockets.add(udp_sock);
                    set_socket_handle(slot_idx, Some(handle));
                    set_fd_type(slot_idx, 2);
                    set_udp_rx_buf(slot_idx, rx_ptr);
                    set_udp_tx_buf(slot_idx, tx_ptr);
                    Some(handle)
                }
                SocketKind::Icmp => {
                    // ICMP socket: 使用 UDP socket buffer (ICMP 无连接, 类似 UDP)
                    let rx_ptr = crate::kernel::framework::mm::k_malloc(UDP_BUF_SIZE);
                    if rx_ptr.is_null() {
                        return None;
                    }
                    let tx_ptr = crate::kernel::framework::mm::k_malloc(UDP_BUF_SIZE);
                    if tx_ptr.is_null() {
                        crate::kernel::framework::mm::k_free(rx_ptr);
                        return None;
                    }
                    // SAFETY: rx_ptr/tx_ptr 来自 k_malloc, 长度合法, 唯一别名
                    let rx_slice = core::slice::from_raw_parts_mut(rx_ptr, UDP_BUF_SIZE);
                    let tx_slice = core::slice::from_raw_parts_mut(tx_ptr, UDP_BUF_SIZE);
                    let rx_meta = udp_rx_meta(slot_idx);
                    let tx_meta = udp_tx_meta(slot_idx);
                    let udp_sock = smoltcp::socket::udp::Socket::new(
                        smoltcp::socket::udp::PacketBuffer::new(
                            &mut rx_meta[..],
                            rx_slice,
                        ),
                        smoltcp::socket::udp::PacketBuffer::new(
                            &mut tx_meta[..],
                            tx_slice,
                        ),
                    );
                    let handle = sockets.add(udp_sock);
                    set_socket_handle(slot_idx, Some(handle));
                    set_fd_type(slot_idx, 2); // ICMP 走 UDP socket 类型
                    set_udp_rx_buf(slot_idx, rx_ptr);
                    set_udp_tx_buf(slot_idx, tx_ptr);
                    Some(handle)
                }
                SocketKind::Raw | SocketKind::Dhcpv4 | SocketKind::Dns => {
                    // Raw/Dhcpv4/Dns: 用户态不可见, 暂不支持
                    None
                }
            }
        }
    }

    /// 实际获取 DHCP 状态 (W4.2.2 实装).
    ///
    /// 翻译 `dhcpv4::Socket::poll()` → `DhcpState`:
    /// - `None` → 保持 prev state (内部 static, 0 初始化 = Idle)
    /// - `Some(Event::Deconfigured)` → Idle
    /// - `Some(Event::Configured(config))` → Bound { ipv4, lease_expires_at: u64::MAX }
    ///
    /// ## 内部状态
    ///
    /// 使用 `static mut PREV_DHCP_STATE` 维护翻译结果. 0 初始化 = Idle.
    /// 调用方需在 NET_LOCK 保护下调用 (确保互斥访问).
    ///
    /// ## dhcpv4 poll 语义
    ///
    /// smoltcp dhcpv4::Socket::poll() 返回 Option<Event>:
    /// - None: 无新事件, DHCP 状态机内部推进中
    /// - Some(Event::Configured): 收到 DHCP ACK, 已配置
    /// - Some(Event::Deconfigured): 收到 DHCP NAK 或租约过期, 已取消配置
    ///
    /// 我们翻译为 trait DhcpState, 简化 lease_expires_at = u64::MAX
    /// (实际租约管理在 init flow 中通过 G_IPV4 / G_GATEWAY 跟踪).
    pub fn dhcp_state_stub(
        sockets: &mut SocketSet<'_>,
        dhcp_handle: Option<smoltcp::iface::SocketHandle>,
    ) -> crate::kernel::framework::net::iface_trait::DhcpState {
        use core::sync::atomic::Ordering;
        use crate::kernel::framework::net::iface_trait::DhcpState;

        // 读取 prev tag (Acquire 同步)
        let prev_tag = PREV_DHCP_TAG.load(Ordering::Acquire);

        // 无 DHCP handle 时, DHCP 未启动, 状态为 Idle
        let Some(handle) = dhcp_handle else {
            return DhcpState::Idle;
        };

        let dhcp = sockets.get_mut::<dhcpv4::Socket>(handle);
        match dhcp.poll() {
            None => {
                // 无新事件, 翻译 prev tag → DhcpState
                tag_to_dhcp_state(prev_tag)
            }
            Some(dhcpv4::Event::Deconfigured) => {
                // DHCP 取消配置, 回到 Idle (tag = 0)
                PREV_DHCP_TAG.store(0, Ordering::Release);
                DhcpState::Idle
            }
            Some(dhcpv4::Event::Configured(config)) => {
                // DHCP 配置完成, 提取 IP + 写 tag
                let ipv4 = config.address.address().octets();
                for (i, &byte) in ipv4.iter().enumerate() {
                    PREV_DHCP_IPV4[i].store(byte, Ordering::Release);
                }
                PREV_DHCP_TAG.store(3, Ordering::Release); // tag 3 = Bound
                DhcpState::Bound {
                    ipv4,
                    lease_expires_at: u64::MAX, // 简化: 实际租约管理在 init flow
                }
            }
        }
    }

    /// SmoltcpNetStack::close 的 safe wrapper (W4.2.3.4).
    ///
    /// 关闭 `[MAX_SM_FD, TOTAL_SLOTS)` 范围内的 smoltcp socket,
    /// 释放 buffer 并清空槽位状态. 与 `sm_close` 逻辑对称, 但索引
    /// 校验针对 SmoltcpNetStack 专属范围.
    ///
    /// ## 返回
    ///
    /// - `true`: 关闭成功
    /// - `false`: slot_idx 越界或槽位空闲
    pub fn smoltcp_net_stack_socket_close(slot_idx: usize) -> bool {
        // SAFETY: 调用方持有 NET_STATE 锁, 整个函数体通过 raw accessor 访问 NetState.
        unsafe {
            // 1. 校验 slot_idx 在 SmoltcpNetStack 范围内
            if !(MAX_SM_FD..TOTAL_SLOTS).contains(&slot_idx) {
                return false;
            }
            // 2. 校验槽位已占用
            if socket_handle(slot_idx).is_none() || fd_type(slot_idx) == 0 {
                return false;
            }

            let handle = socket_handle(slot_idx).unwrap();
            let stype = fd_type(slot_idx);
            let sockets = &mut *socket_set();

            // 3. 根据类型关闭 TCP/UDP socket
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

            // 4. 从 SocketSet 移除
            sockets.remove(handle);

            // 5. 释放 slab buffer
            if !tcp_rx_buf(slot_idx).is_null() {
                crate::kernel::framework::mm::k_free(tcp_rx_buf(slot_idx));
                set_tcp_rx_buf(slot_idx, core::ptr::null_mut());
            }
            if !tcp_tx_buf(slot_idx).is_null() {
                crate::kernel::framework::mm::k_free(tcp_tx_buf(slot_idx));
                set_tcp_tx_buf(slot_idx, core::ptr::null_mut());
            }
            if !udp_rx_buf(slot_idx).is_null() {
                crate::kernel::framework::mm::k_free(udp_rx_buf(slot_idx));
                set_udp_rx_buf(slot_idx, core::ptr::null_mut());
            }
            if !udp_tx_buf(slot_idx).is_null() {
                crate::kernel::framework::mm::k_free(udp_tx_buf(slot_idx));
                set_udp_tx_buf(slot_idx, core::ptr::null_mut());
            }

            // 6. 清空槽位状态
            set_socket_handle(slot_idx, None);
            set_fd_type(slot_idx, 0);
            true
        }
    }

    /// SmoltcpNetStack::poll 的 safe wrapper (W4.2.3.4).
    ///
    /// 驱动 smoltcp 协议栈轮询 (TX/RX + 定时器 + DHCP), 返回 `PollOutcome`.
    /// 与 `poll_network` 逻辑对称, 但由 SmoltcpNetStack 调用方主动触发
    /// (而非 timer ISR 自动轮询).
    pub fn smoltcp_net_stack_poll() -> crate::kernel::framework::net::iface_trait::PollOutcome {
        use crate::kernel::framework::net::iface_trait::PollOutcome;

        let nic = match device_mut() {
            Some(d) => d,
            None => return PollOutcome::idle(),
        };
        let stack = match stack_mut() {
            Some(s) => s,
            None => return PollOutcome::idle(),
        };
        // SAFETY: socket_set() 返回已初始化的 SocketSet 指针, 调用方持有 NET_LOCK.
        let sockets = unsafe { &mut *socket_set() };

        // SAFETY: stack.poll 驱动 smoltcp 协议栈 (RX/TX + 定时器), 返回 PollResult.
        let poll_result = stack.poll(nic, sockets);
        // SAFETY: process_dhcp_events 在 NET_LOCK 保护下处理 DHCP 事件.
        unsafe { super::process_dhcp_events(sockets) };

        // 将 smoltcp PollResult 翻译为 QueenX PollOutcome
        match poll_result {
            smoltcp::iface::PollResult::SocketStateChanged => PollOutcome {
                packet_received: true,
                socket_woken: true,
                dhcp_progressed: false,
                tx_pending: 0,
            },
            smoltcp::iface::PollResult::None => PollOutcome::idle(),
        }
    }

    /// DhcpState tag → DhcpState 翻译.
    ///
    /// tag 值 (来自 PREV_DHCP_TAG):
    /// - 0: Idle
    /// - 1: Discovering
    /// - 2: Requesting
    /// - 3: Bound (含 ipv4)
    /// - 4: Renewing
    /// - 5: Failed
    fn tag_to_dhcp_state(tag: u8) -> crate::kernel::framework::net::iface_trait::DhcpState {
        use core::sync::atomic::Ordering;
        use crate::kernel::framework::net::iface_trait::DhcpState;
        match tag {
            0 => DhcpState::Idle,
            1 => DhcpState::Discovering,
            2 => DhcpState::Requesting,
            3 => {
                let ipv4 = [
                    PREV_DHCP_IPV4[0].load(Ordering::Acquire),
                    PREV_DHCP_IPV4[1].load(Ordering::Acquire),
                    PREV_DHCP_IPV4[2].load(Ordering::Acquire),
                    PREV_DHCP_IPV4[3].load(Ordering::Acquire),
                ];
                DhcpState::Bound { ipv4, lease_expires_at: u64::MAX }
            }
            4 => {
                let ipv4 = [
                    PREV_DHCP_IPV4[0].load(Ordering::Acquire),
                    PREV_DHCP_IPV4[1].load(Ordering::Acquire),
                    PREV_DHCP_IPV4[2].load(Ordering::Acquire),
                    PREV_DHCP_IPV4[3].load(Ordering::Acquire),
                ];
                DhcpState::Renewing { ipv4 }
            }
            5 => DhcpState::Failed,
            _ => DhcpState::Idle, // 默认
        }
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
    use crate::kernel::framework::net::iface_trait::{
        Ipv4Addr as TraitIpv4Addr, NetEndpoint as TraitEndpoint,
    };

    /// W4.4: 验证 wire_to_smol_v4 / endpoint_to_smol 翻译不丢字段.
    #[test]
    fn test_wire_translation_roundtrip() {
        let trait_addr = TraitIpv4Addr::new(192, 168, 1, 100);
        let smol = wire_to_smol_v4(trait_addr);
        assert_eq!(smol.octets(), [192, 168, 1, 100]);
        // endpoint 翻译: 验证 addr+port 双向不丢
        let ep = TraitEndpoint::new(TraitIpv4Addr::new(10, 0, 2, 15), 8080);
        let ep_smol = endpoint_to_smol(ep);
        assert_eq!(ep_smol.port, 8080);
        if let IpAddress::Ipv4(v4) = ep_smol.addr {
            assert_eq!(v4.octets(), [10, 0, 2, 15]);
        } else {
            panic!("expected IpAddress::Ipv4");
        }
        // 反向翻译 (从 smoltcp 类型)
        let back = endpoint_from_smol(ep_smol).unwrap();
        assert_eq!(back.port, 8080);
        assert_eq!(back.addr.octets(), [10, 0, 2, 15]);
    }

    /// W4.4: 验证 parse_ipv4_endpoint_trait 解析后立即落入 trait 抽象类型.
    #[test]
    fn test_parse_ipv4_endpoint_trait_bridge() {
        // 构造一个 sockaddr_in 字节序列
        let mut buf = [0u8; 16];
        buf[0..2].copy_from_slice(&2u16.to_ne_bytes()); // AF_INET
        buf[2..4].copy_from_slice(&8080u16.to_be_bytes()); // port (big-endian)
        buf[4..8].copy_from_slice(&[192, 168, 1, 50]);
        // SAFETY: buf 完整 16 字节, 模拟 C sockaddr_in 布局
        let ep = unsafe { parse_ipv4_endpoint_trait(buf.as_ptr()) }.unwrap();
        assert_eq!(ep.addr.octets(), [192, 168, 1, 50]);
        assert_eq!(ep.port, 8080);
    }

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
        assert_eq!(dns_resolve("queenx-gateway"), Some([10, 0, 2, 2]));
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
    fn test_dns_servers_default_empty() {
        // SAFETY: 单线程测试, reset 仅修改状态原子变量
        unsafe { reset_network_state(); }
        let dns = get_dns_servers();
        assert_eq!(dns, [None, None, None]);
    }
}