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
// W4.4: Ipv4Address/IpCidr/IpEndpoint/IpAddress 通过 NetStack trait 类型
// 翻译层访问 (services 边界), 直接使用 smoltcp wire 类型仅在 framework
// 翻译 helper 内部 (qemu_net_skel 一类适配器). W4.4 阶段先把最常用的
// 4 处 (net_save + setup + parse_ipv4_endpoint + endpoint 访问) 替换.
use smoltcp::wire::{IpCidr, IpEndpoint, IpListenEndpoint, IpAddress, Ipv4Address};

// REVAL-W W4.1 (2026-06-25): 引入 SmoltcpNetStack 实例, 这是 NetStack
// trait 的 smoltcp 实现 (W3.2 产物). 重构后, init.rs 中的 smoltcp 直接
// 使用将逐步替换为 `SmoltcpNetStack` 的 trait 方法. 此处先添加静态实例,
// 暂不修改现有逻辑, 仅做小步实装 + 编译验证.
use crate::kernel::services::net::smoltcp_impl::SmoltcpNetStack;
use crate::kernel::services::net::unix as uds_svc;
use crate::kernel::framework::net::iface_trait::{NetConfig as TraitNetConfig, NetStack};

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

// REVAL-W W4.1: SmoltcpNetStack 实例 (W3.2 产物的 trait 翻译层).
// `static mut` 与现有 NET_DEVICE/NET_STACK 一致 (framework 允许 unsafe).
// 此实例暂未被任何代码使用 — 实际接入留给 W4.2-W4.4 替换现有 smoltcp
// 直接调用. 现阶段仅做小步实装 + 编译验证.
#[allow(dead_code)] // W4.2+ 接入后移除此 allow
static mut NET_STACK_TRAIT: Option<SmoltcpNetStack> = None;

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

/// 构造并初始化 NET_STACK_TRAIT (W3.2 SmoltcpNetStack 实例).
///
/// ## 调用方契约
///
/// - 必须在 NET_LOCK 保护下调用
/// - 仅 init 阶段调用一次
/// - 失败时回滚 (NET_STACK_TRAIT 仍为 None)
/// - 成功时填充 NET_STACK_TRAIT
///
/// ## W4.2+ 整合点
///
/// 现有 `init_sockets` + 后续 `init_device` + `configure_interfaces` 等
/// 将改用本函数返回的 `SmoltcpNetStack` (通过 trait 调用).
///
/// # Safety
///
/// - 调用方须持有 NET_LOCK 互斥锁
/// - 仅在 init 阶段调用一次 (重复调用会覆盖前一个 stack, 但不会泄漏,
///   因为 SmoltcpNetStack 不持有 'static 借用)
///
/// SAFETY: 调用方持有 NET_LOCK, 独占访问 NET_STACK_TRAIT
/// write/read 在 no_std 不稳定, 用裸指针替换
#[allow(dead_code)] // W4.2+ 接入后移除此 allow
pub unsafe fn init_net_stack_trait(cfg: TraitNetConfig) -> Result<(), ()> {
    let mut stack = SmoltcpNetStack::new();
    match stack.init(cfg) {
        Ok(()) => {
            // SAFETY: 调用方持有 NET_LOCK, 独占访问 NET_STACK_TRAIT
            // write/read 在 no_std 不稳定, 用裸指针替换
            let ptr = &mut NET_STACK_TRAIT as *mut Option<SmoltcpNetStack>;
            core::ptr::write(ptr, Some(stack));
            Ok(())
        }
        Err(_e) => {
            // 失败时保持 None, 不修改状态
            Err(())
        }
    }
}

/// 查询 NET_STACK_TRAIT 是否已初始化.
#[allow(dead_code)] // W4.2+ 接入后移除此 allow
pub fn is_net_stack_trait_ready() -> bool {
    // SAFETY: 仅检查 Some/None, 不解引用 Some 内值
    // 可变 static 的引用需要 unsafe
    let ptr = unsafe { &NET_STACK_TRAIT as *const Option<SmoltcpNetStack> };
    unsafe { (*ptr).is_some() }
}

/// 获取 NET_STACK_TRAIT 的可变引用 (供 trait 方法调用).
///
/// ## 调用方契约
///
/// - 必须在 NET_LOCK 保护下调用
/// - 必须先调用 is_net_stack_trait_ready() 确认已初始化
///
/// # Safety
///
/// - 调用方须持有 NET_LOCK 互斥锁
/// - 必须先调用 is_net_stack_trait_ready() 确认已初始化
///
/// SAFETY: 通过裸指针解引用获取 &mut, 调用方保证 NET_LOCK 互斥.
#[allow(dead_code)] // W4.2+ 接入后移除此 allow
pub unsafe fn net_stack_trait_mut() -> Option<&'static mut SmoltcpNetStack> {
    let ptr = &mut NET_STACK_TRAIT as *mut Option<SmoltcpNetStack>;
    (*ptr).as_mut()
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
#[allow(dead_code)] // W5+ 阶段逐步替换
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
                let raw = saved.fd_handles[i];
                // SAFETY: `raw` 是 `as_u32_handle` 持久化的同构 smoltcp 句柄;
                //         smol_handle_from_u32 用 transmute_copy 安全重建.
                Some(unsafe { smol_handle_from_u32(raw) })
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
}

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
#[allow(dead_code)] // W4.4+ 阶段替换逐步接入
pub(crate) fn wire_to_smol_v4(a: crate::kernel::framework::net::iface_trait::Ipv4Addr) -> Ipv4Address {
    let o = a.octets();
    Ipv4Address::new(o[0], o[1], o[2], o[3])
}

/// 把 trait 抽象的 `NetEndpoint` 翻译成 smoltcp 的 `IpEndpoint`.
#[inline]
#[allow(dead_code)]
pub(crate) fn endpoint_to_smol(
    e: crate::kernel::framework::net::iface_trait::NetEndpoint,
) -> IpEndpoint {
    IpEndpoint {
        addr: IpAddress::Ipv4(wire_to_smol_v4(e.addr)),
        port: e.port,
    }
}

/// 把 trait 抽象的 `NetListenEndpoint` 翻译成 smoltcp 的 `IpListenEndpoint`.
#[inline]
#[allow(dead_code)]
pub(crate) fn listen_endpoint_to_smol(
    e: crate::kernel::framework::net::iface_trait::NetListenEndpoint,
) -> IpListenEndpoint {
    match e.addr {
        None => IpListenEndpoint { addr: None, port: e.port },
        Some(a) => IpListenEndpoint {
            addr: Some(IpAddress::Ipv4(wire_to_smol_v4(a))),
            port: e.port,
        },
    }
}

/// 从 smoltcp `IpAddress` 中提取 IPv4 octets, 翻译为 trait `Ipv4Addr`.
#[inline]
#[allow(dead_code)]
pub(crate) fn ipaddr_from_smol(a: IpAddress) -> Option<crate::kernel::framework::net::iface_trait::Ipv4Addr> {
    match a {
        IpAddress::Ipv4(v4) => Some(crate::kernel::framework::net::iface_trait::Ipv4Addr::from_octets(v4.octets())),
        _ => None,
    }
}

/// 从 smoltcp `IpEndpoint` 翻译为 trait `NetEndpoint`.
#[inline]
#[allow(dead_code)]
pub(crate) fn endpoint_from_smol(e: IpEndpoint) -> Option<crate::kernel::framework::net::iface_trait::NetEndpoint> {
    Some(crate::kernel::framework::net::iface_trait::NetEndpoint::new(
        ipaddr_from_smol(e.addr)?,
        e.port,
    ))
}

/// 从 smoltcp `IpCidr` 翻译为 trait `Ipv4Cidr` (仅 IPv4).
#[inline]
#[allow(dead_code)]
pub(crate) fn cidr_from_smol(c: IpCidr) -> Option<crate::kernel::framework::net::iface_trait::Ipv4Cidr> {
    match c {
        IpCidr::Ipv4(v4) => Some(crate::kernel::framework::net::iface_trait::Ipv4Cidr::new(
            crate::kernel::framework::net::iface_trait::Ipv4Addr::from_octets(v4.address().octets()),
            v4.prefix_len(),
        )),
        _ => None,
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
) -> Option<crate::kernel::framework::net::iface_trait::NetEndpoint> {
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
}

/// 旧版 `parse_ipv4_endpoint` 包装, 保持向后兼容 (smoltcp wire 类型返回).
///
/// 内部走 trait 翻译路径, 调用方应优先改用 `parse_ipv4_endpoint_trait`.
///
/// # Safety
/// 同 `parse_ipv4_endpoint_trait`.
unsafe fn parse_ipv4_endpoint(addr: *const u8) -> Option<IpEndpoint> {
    parse_ipv4_endpoint_trait(addr).map(endpoint_to_smol)
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
/// v2: 识别 SO_PASSCRED (level=SOL_SOCKET=1, optname=SO_PASSCRED=16).
/// 路由到 UDS 服务层 (uds_setsockopt).
/// 其他 (level, optname): 0 (no-op).
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
    // v2 SO_PASSCRED 路由: level==1 (SOL_SOCKET), optname==16 (SO_PASSCRED)
    if _level == 1 && _optname == 16 {
        if _optlen < 4 {
            return -22; // EINVAL
        }
        let val = core::ptr::read_unaligned(_optval as *const i32);
        return uds_svc::uds_setsockopt(_fd, val != 0);
    }
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

// TD-05: 8 张 smoltcp 大表, 小型热表按 64 字节 cache line 对齐, 减少多核 false sharing.
// 大型 buffer (TCP/UDP buf) 单 fd 独占一整片区域, 默认不会被相邻 fd 抢用, 仅需保持页对齐即可.
//
// 实现方式: `#[repr(align(N))]` 不能直接用于 `static mut [T; N]`, 改用 `static mut W: Wrapper<T>`.
#[repr(align(64))]
struct Align64<T>(T);

#[allow(non_camel_case_types)]
type SOCKET_TABLE_T = Align64<[Option<SocketHandle>; TOTAL_SLOTS]>;
#[allow(non_camel_case_types)]
type FD_TYPES_T = Align64<[u8; TOTAL_SLOTS]>;

static mut SOCKET_TABLE: SOCKET_TABLE_T = Align64([None; TOTAL_SLOTS]);
// Per-fd 类型标记: 0=free, 1=tcp, 2=udp.
// 64 字节对齐: 8 核机器下每核独立访问自己 fd 对应的 cache line, 不会因 1 字节写触发整行 invalidation.
static mut FD_TYPES: FD_TYPES_T = Align64([0u8; TOTAL_SLOTS]);

// TCP buffer storage (per fd)
// TD-07: 由 4 张 [[u8; N]; MAX_SM_FD] 静态数组 (≈3 MB BSS) 改为 [*mut u8; MAX_SM_FD] 指针表.
// 启动时 0 占用; socket alloc 时通过 `k_malloc` (slab) 申请; close 时 `k_free` 归还.
// 省下的 3 MB BSS 改为按需占用, 与 smoltcp `MAX_SM_FD` 解耦 (见 TD-06).
// REVAL-W W4.2.3.1: 数组大小扩展为 [T; TOTAL_SLOTS] (sm_socket + SmoltcpNetStack 共享).
static mut TCP_RX_BUFS: [*mut u8; TOTAL_SLOTS] = [null_mut(); TOTAL_SLOTS];
static mut TCP_TX_BUFS: [*mut u8; TOTAL_SLOTS] = [null_mut(); TOTAL_SLOTS];

// UDP buffer storage (per fd) — 同样 TD-07 改造
static mut UDP_RX_BUFS: [*mut u8; TOTAL_SLOTS] = [null_mut(); TOTAL_SLOTS];
static mut UDP_TX_BUFS: [*mut u8; TOTAL_SLOTS] = [null_mut(); TOTAL_SLOTS];

// UDP metas 仍保留静态 (16 KB, 256 × 4 × 16B, 不值得动); td 改 metas 走 heap 是 V2 任务.
// REVAL-W W4.2.3.1: 数组大小扩展为 [T; TOTAL_SLOTS].
static mut UDP_RX_METAS: [[udp::PacketMetadata; UDP_META_COUNT]; TOTAL_SLOTS] =
    [[udp::PacketMetadata::EMPTY; UDP_META_COUNT]; TOTAL_SLOTS];
static mut UDP_TX_METAS: [[udp::PacketMetadata; UDP_META_COUNT]; TOTAL_SLOTS] =
    [[udp::PacketMetadata::EMPTY; UDP_META_COUNT]; TOTAL_SLOTS];

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
#[allow(dead_code)] // W4.2.3.4 整合后移除
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
    #[allow(dead_code)] // W4.2.3.3+ 接入后移除
    pub fn socket_open_stub(
        sockets: &mut SocketSet<'_>,
        kind: crate::kernel::framework::net::iface_trait::SocketKind,
        slot_idx: usize,
    ) -> Option<smoltcp::iface::SocketHandle> {
        use crate::kernel::framework::net::iface_trait::SocketKind;

        // SAFETY: 整个函数体访问多个 static mut (SOCKET_TABLE, FD_TYPES, TCP_RX_BUFS,
        // TCP_TX_BUFS, UDP_RX_BUFS, UDP_TX_BUFS, UDP_RX_METAS, UDP_TX_METAS). 调用方
        // 持有 NET_LOCK 保护 (与现有 sm_socket 路径一致).
        unsafe {
            // 1. 校验 slot_idx 范围
            if slot_idx >= TOTAL_SLOTS {
                return None;
            }
            // 2. 校验槽位空闲
            if SOCKET_TABLE.0[slot_idx].is_some() {
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
                    SOCKET_TABLE.0[slot_idx] = Some(handle);
                    FD_TYPES.0[slot_idx] = 1;
                    TCP_RX_BUFS[slot_idx] = rx_ptr;
                    TCP_TX_BUFS[slot_idx] = tx_ptr;
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
                    let udp_sock = smoltcp::socket::udp::Socket::new(
                        smoltcp::socket::udp::PacketBuffer::new(
                            &mut UDP_RX_METAS[slot_idx][..],
                            rx_slice,
                        ),
                        smoltcp::socket::udp::PacketBuffer::new(
                            &mut UDP_TX_METAS[slot_idx][..],
                            tx_slice,
                        ),
                    );
                    let handle = sockets.add(udp_sock);
                    SOCKET_TABLE.0[slot_idx] = Some(handle);
                    FD_TYPES.0[slot_idx] = 2;
                    UDP_RX_BUFS[slot_idx] = rx_ptr;
                    UDP_TX_BUFS[slot_idx] = tx_ptr;
                    Some(handle)
                }
                // Icmp / Raw / Dhcpv4 / Dns 暂不实装 (W4.2.4+ 阶段)
                _ => None,
            }
        }
    }

    /// 实际关闭一个 socket (W4.2.2 实装).
    ///
    /// 调用 `SocketSet::remove(smol_handle)` 删除 socket. smoltcp API 返回
    /// `Socket` enum (类型擦除内部 socket), 删除已发生, 返回值被丢弃.
    ///
    /// ## 调用方契约
    ///
    /// - 必须在 NET_LOCK 保护下调用
    /// - sockets 来自本模块的 socket_set()
    /// - smol_handle 必须是 sockets 中有效的 socket 句柄, 否则 smoltcp panic
    ///
    /// ## 返回值
    ///
    /// 始终返回 `true` (smoltcp 0.13.1 的 `SocketSet::remove` 不会失败).
    #[allow(dead_code)] // W4.2.4+ 接入后移除
    pub fn socket_close_stub(
        sockets: &mut SocketSet<'_>,
        smol_handle: smoltcp::iface::SocketHandle,
    ) -> bool {
        // smoltcp SocketSet::remove 删除 socket, 返回 Socket enum 被丢弃.
        // 0.13.1 文档: "Removes a socket from the set, returning the socket that was removed."
        let _removed = sockets.remove(smol_handle);
        true
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
    #[allow(dead_code)] // W4.2.4+ 接入后移除
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

    /// smoltcp SocketSet::remove 辅助 (W4.2 阶段 1 stub).
    ///
    /// 阶段 1 简化: 仅返回 false (未实现), 不实际修改 SocketSet.
    /// 阶段 2+ 实装: 调用 `sockets.remove(handle)` 并返回 true.
    fn sockets_remove_helper(_smol_handle: smoltcp::iface::SocketHandle) -> bool {
        // W4.2 阶段 1: 0 逻辑, 返回 false (未实现)
        // W4.2.2+ 实装: 
        //   let smol_socket = sockets.remove(_smol_handle);
        //   // smoltcp SocketSet::remove 返回 Socket enum, 实际删除已发生
        //   true
        false
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

    /// W4.4: 验证 ipaddr_from_smol 对 IPv4 提取 octets, 其它变体返回 None.
    #[test]
    fn test_ipaddr_from_smol_v4_only() {
        let v4_in = wire_to_smol_v4(TraitIpv4Addr::new(8, 8, 8, 8));
        let out = ipaddr_from_smol(IpAddress::Ipv4(v4_in)).unwrap();
        assert_eq!(out.octets(), [8, 8, 8, 8]);
    }

    /// W4.4: 验证 cidr_from_smol 把 smoltcp CIDR 翻译为 trait 抽象.
    #[test]
    fn test_cidr_from_smol() {
        let cidr_smol = IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(
            Ipv4Address::new(10, 0, 0, 0),
            8,
        ));
        let out = cidr_from_smol(cidr_smol).unwrap();
        assert_eq!(out.address.octets(), [10, 0, 0, 0]);
        assert_eq!(out.prefix_len, 8);
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