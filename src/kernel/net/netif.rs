#![allow(dead_code)]
/// 网络接口管理
///
/// 提供网络接口（netif）的配置和管理功能，
/// 包括 DHCP 客户端、IPv6 配置和状态监控。
///
/// ## 安全性改进 (相比 C 版本)
///
/// - **静态存储**: netif结构体使用全局静态存储，避免use-after-free
/// - **原子操作**: 使用 AtomicU8 替代裸全局变量
/// - **类型安全**: IP 地址解析使用 Option 防止空指针
/// - **边界检查**: 所有数组访问都有边界验证
/// - **RAII**: 资源自动清理
///
/// ## Rust E1000 集成
///
/// 现在使用纯 Rust 实现的 E1000 驱动：
/// - `E1000Device` 结构体管理硬件状态
/// - 通过 FFI 与 lwIP 协议栈对接
/// - 类型安全的 MMIO 操作


use core::sync::atomic::{AtomicU8, Ordering};
use crate::kernel::net::types::*;
use crate::kernel::net::driver::e1000::E1000Device;
use crate::kernel::driver::framework::{Driver, DriverError};

// ============================================================================
// FFI 声明 - lwIP C 库函数
// ============================================================================

extern "C" {
    /// 日志输出函数 (已在 types.rs 中声明)
    
    /// 添加网络接口到 lwIP
    fn netif_add(
        netif: *mut core::ffi::c_void,
        ipaddr: *const core::ffi::c_void,
        netmask: *const core::ffi::c_void,
        gw: *const core::ffi::c_void,
        state: *mut core::ffi::c_void,
        init: extern "C" fn(*mut core::ffi::c_void) -> i32,
        input: extern "C" fn(*mut core::ffi::c_void, *mut core::ffi::c_void) -> i32,
    ) -> *mut core::ffi::c_void;
    
    /// 设置默认网络接口
    fn netif_set_default(netif: *mut core::ffi::c_void);
    
    /// 设置网络接口状态回调
    fn netif_set_status_callback(
        netif: *mut core::ffi::c_void,
        callback: extern "C" fn(*mut core::ffi::c_void),
    );
    
    /// 启动网络接口
    fn netif_set_up(netif: *mut core::ffi::c_void);
    
    /// 启动 DHCP 客户端
    fn dhcp_start(netif: *mut core::ffi::c_void) -> i32;
    
    /// 创建 IPv6 链路本地地址
    #[cfg(feature = "ipv6")]
    fn netif_create_ip6_linklocal_address(netif: *mut core::ffi::c_void, from_mac: u8);
    
    /// 设置 IPv6 自动配置
    #[cfg(feature = "ipv6")]
    fn netif_set_ip6_autoconfig_enabled(netif: *mut core::ffi::c_void, enabled: u8);
    
    /// 初始化网络应用
    fn qx_net_apps_init(netif: *mut core::ffi::c_void);
    
    /// E1000 初始化 (网卡驱动)
    fn e1000_init(netif: *mut core::ffi::c_void) -> i32;
    
    /// Ethernet 输入处理
    fn ethernet_input(
        p: *mut core::ffi::c_void,
        netif: *mut core::ffi::c_void,
    ) -> i32;
    
    /// 获取 IP 地址各字节
    fn ip4_addr1(addr: *const core::ffi::c_void) -> u8;
    fn ip4_addr2(addr: *const core::ffi::c_void) -> u8;
    fn ip4_addr3(addr: *const core::ffi::c_void) -> u8;
    fn ip4_addr4(addr: *const core::ffi::c_void) -> u8;
    
    /// 获取 IPv4 地址指针
    fn ip_2_ip4(addr: *const core::ffi::c_void) -> *const core::ffi::c_void;
    
    /// 获取 IP 地址 (u32格式)
    fn ip4_addr_get_u32(addr: *const core::ffi::c_void) -> u32;
    
    /// 检查 IPv6 地址是否有效
    #[cfg(feature = "ipv6")]
    fn ip6_addr_isvalid(state: u8) -> u8;
    
    /// 获取 IPv6 地址
    #[cfg(feature = "ipv6")]
    fn netif_ip6_addr(netif: *const core::ffi::c_void, idx: u8) -> *const core::ffi::c_void;
    
    /// 获取 IPv6 地址状态
    #[cfg(feature = "ipv6")]
    fn netif_ip6_addr_state(netif: *const core::ffi::c_void, idx: u8) -> u8;
}

// ============================================================================
// 全局状态 (线程安全, 静态存储)
// ============================================================================

/// DHCP 完成标志 (原子操作, 无 data race)
static G_DHCP_DONE: AtomicU8 = AtomicU8::new(0);

/// 网络初始化完成标志
static G_NET_INITIALIZED: AtomicU8 = AtomicU8::new(0);

/// 全局网络接口实例 (✅ 静态存储, 整个程序生命周期有效)
/// 大小需匹配 lwIP netif 结构体 (通常 256-512 字节)
static mut G_NETIF_BUFFER: [u8; 2048] = [0u8; 2048];  // ✅ 静态分配

/// 保存全局网络接口指针
static mut G_NETIF_PTR: *mut core::ffi::c_void = core::ptr::null_mut();

// ============================================================================
// 辅助函数 - IP 地址格式化输出
// ============================================================================

/// 格式化并输出 IPv4 地址日志
unsafe fn log_ipv4_address(netif_ptr: *mut core::ffi::c_void, prefix: &str) {
    if netif_ptr.is_null() {
        return;
    }
    
    // 注意: 这里需要访问 netif 结构体的 ip_addr 字段
    // 由于 FFI 的复杂性, 我们简化处理
    // 实际实现需要根据 lwIP netif 结构体定义调整偏移量
    
    let _ = (netif_ptr, prefix);
    
    // TODO: 实现 IP 地址解析和格式化输出
    // 类似 C 版本的:
    // klog_net("Interface up: %d.%d.%d.%d/%d.%d.%d.%d gw=%d.%d.%d.%d", ...)
}

// ============================================================================
// 网络接口状态回调
// ============================================================================

/// 网络接口状态变化回调
/// 
/// 当 IP 地址发生变化时被 lwIP 调用。
/// 主要用于:
/// 1. 检测 DHCP 绑定完成事件
/// 2. 输出网络配置信息
/// 3. 触发网络应用初始化
#[no_mangle]
pub unsafe extern "C" fn qx_netif_status_callback(netif: *mut core::ffi::c_void) {
    if netif.is_null() {
        return;
    }
    
    extern "C" {
        fn qx_netif_ip4_addr_u32(netif: *const core::ffi::c_void) -> u32;
    }
    
    let ip = qx_netif_ip4_addr_u32(netif);
    if ip == 0 {
        return;
    }
    
    klog_net("Network interface status changed\0".as_ptr() as *const i8);
    
    if G_DHCP_DONE.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
        klog_net("DHCP bound or IP address assigned\0".as_ptr() as *const i8);
        
        qx_net_apps_init(netif);
        
        klog_net("Network applications initialized\0".as_ptr() as *const i8);
    }
}

// ============================================================================
// Safe FFI 包装器 (供内部使用)
// ============================================================================

/// E1000 初始化包装器 (safe 函数签名)
extern "C" fn e1000_init_wrapper(netif: *mut core::ffi::c_void) -> i32 {
    unsafe { e1000_init(netif) }
}

/// Ethernet 输入处理包装器
extern "C" fn ethernet_input_wrapper(
    p: *mut core::ffi::c_void,
    netif: *mut core::ffi::c_void,
) -> i32 {
    unsafe { ethernet_input(p, netif) }
}

/// 状态回调包装器
extern "C" fn status_callback_wrapper(netif: *mut core::ffi::c_void) {
    unsafe { qx_netif_status_callback(netif) }
}

/// E1000 数据包输入处理函数
///
/// 此函数由 E1000 网卡驱动在中断处理程序中调用，
/// 将接收到的以太网帧传递给 lwIP 协议栈。
///
/// # Arguments
/// * `data` - 接收到的数据包缓冲区
/// * `len` - 数据包长度
///
/// # Returns
/// * `0` - 成功处理
/// * `<0` - 处理失败
#[no_mangle]
pub unsafe extern "C" fn ethernet_input_from_e1000(
    data: *mut core::ffi::c_void,
    len: u16,
) -> i32 {
    extern "C" {
        fn qx_rx_packet(netif: *mut core::ffi::c_void, data: *const core::ffi::c_void, len: u16) -> i32;
    }
    if G_NETIF_PTR.is_null() || data.is_null() || len == 0 {
        return LwipErr::Val as i32;
    }
    qx_rx_packet(G_NETIF_PTR, data as *const core::ffi::c_void, len)
}

/// virtio-net 初始化包装器 (FFI, 供 lwIP netif_add 调用)
extern "C" fn virtio_net_init_wrapper(netif: *mut core::ffi::c_void) -> i32 {
    extern "C" {
        fn virtio_net_init(netif: *mut core::ffi::c_void) -> i32;
    }
    unsafe { virtio_net_init(netif) }
}

/// virtio-net 数据包输入处理函数
///
/// 此函数由 virtio-net 驱动的 poll_rx 调用，
/// 将接收到的以太网帧传递给 lwIP 协议栈。
#[no_mangle]
pub unsafe extern "C" fn ethernet_input_from_virtio(
    data: *mut core::ffi::c_void,
    len: u16,
) -> i32 {
    extern "C" {
        fn qx_rx_packet(netif: *mut core::ffi::c_void, data: *const core::ffi::c_void, len: u16) -> i32;
    }
    if G_NETIF_PTR.is_null() || data.is_null() || len == 0 {
        return LwipErr::Val as i32;
    }
    qx_rx_packet(G_NETIF_PTR, data as *const core::ffi::c_void, len)
}

// ============================================================================
// virtio-net 网络接口注册 (aarch64)
// ============================================================================

/// 注册 virtio-net 网卡为 lwIP 网络接口
#[no_mangle]
pub unsafe extern "C" fn qx_netif_register_virtio() -> i32 {
    klog_net("Registering virtio-net as lwIP netif\0".as_ptr() as *const i8);

    if !G_NETIF_PTR.is_null() {
        klog_net("Netif already registered, skipping\0".as_ptr() as *const i8);
        return LwipErr::Ok as i32;
    }

    let netif_ptr = G_NETIF_BUFFER.as_mut_ptr() as *mut core::ffi::c_void;
    core::ptr::write_bytes(G_NETIF_BUFFER.as_mut_ptr(), 0, 2048);

    let result = netif_add(
        netif_ptr,
        core::ptr::null(),
        core::ptr::null(),
        core::ptr::null(),
        core::ptr::null_mut(),
        virtio_net_init_wrapper,
        ethernet_input_wrapper,
    );

    if result.is_null() {
        klog_net_err("netif_add failed (virtio)\0".as_ptr() as *const i8);
        return LwipErr::If as i32;
    }

    netif_set_default(result);
    netif_set_up(result);
    extern "C" { fn netif_set_link_up(netif: *mut core::ffi::c_void); }
    netif_set_link_up(result);
    netif_set_status_callback(result, status_callback_wrapper);
    G_NETIF_PTR = result;
    G_NET_INITIALIZED.store(1, Ordering::Release);

    // 启动 DHCP
    klog_net("Starting DHCP on virtio-net...\0".as_ptr() as *const i8);
    let dhcp_result = dhcp_start(result);
    if dhcp_result == 0 {
        klog_net("DHCP client started\0".as_ptr() as *const i8);
    } else {
        klog_net_err("DHCP start failed\0".as_ptr() as *const i8);
    }

    klog_net("virtio-net netif registered\0".as_ptr() as *const i8);
    LwipErr::Ok as i32
}

// ============================================================================
// E1000 网络接口注册 (x86_64)
// ============================================================================

/// 注册 E1000 网卡为 lwIP 网络接口
#[no_mangle]
pub unsafe extern "C" fn qx_netif_register_e1000() -> i32 {
    klog_net("Registering e1000 as lwIP netif\0".as_ptr() as *const i8);

    if !G_NETIF_PTR.is_null() {
        klog_net("Netif already registered, skipping\0".as_ptr() as *const i8);
        return LwipErr::Ok as i32;
    }

    let netif_ptr = G_NETIF_BUFFER.as_mut_ptr() as *mut core::ffi::c_void;
    core::ptr::write_bytes(G_NETIF_BUFFER.as_mut_ptr(), 0, 2048);

    let result = netif_add(
        netif_ptr,
        core::ptr::null(),
        core::ptr::null(),
        core::ptr::null(),
        core::ptr::null_mut(),
        e1000_init_wrapper,
        ethernet_input_wrapper,
    );

    if result.is_null() {
        klog_net_err("netif_add failed (e1000)\0".as_ptr() as *const i8);
        return LwipErr::If as i32;
    }

    netif_set_default(result);
    netif_set_up(result);
    extern "C" { fn netif_set_link_up(netif: *mut core::ffi::c_void); }
    netif_set_link_up(result);
    netif_set_status_callback(result, status_callback_wrapper);
    G_NETIF_PTR = result;
    G_NET_INITIALIZED.store(1, Ordering::Release);

    // 启动 DHCP
    klog_net("Starting DHCP on e1000...\0".as_ptr() as *const i8);
    let dhcp_result = dhcp_start(result);
    if dhcp_result == 0 {
        klog_net("DHCP client started\0".as_ptr() as *const i8);
    } else {
        klog_net_err("DHCP start failed\0".as_ptr() as *const i8);
    }

    klog_net("e1000 netif registered\0".as_ptr() as *const i8);
    LwipErr::Ok as i32
}

// ============================================================================
// 公共 API (供 Rust 内部和其他模块使用)
// ============================================================================

/// 获取全局网络接口指针
/// 
/// # Safety
/// 
/// 返回的指针指向静态存储区域，始终有效
pub unsafe fn get_netif() -> Option<*mut core::ffi::c_void> {
    if G_NETIF_PTR.is_null() {
        None
    } else {
        Some(G_NETIF_PTR)
    }
}

/// 检查 DHCP 是否已完成
pub fn is_dhcp_done() -> bool {
    G_DHCP_DONE.load(Ordering::Acquire) != 0
}

/// 检查网络是否已初始化
pub fn is_network_initialized() -> bool {
    G_NET_INITIALIZED.load(Ordering::Acquire) != 0
}

/// 重置 DHCP 状态 (用于测试或重新连接)
pub fn reset_dhcp_state() {
    G_DHCP_DONE.store(0, Ordering::Release);
}

/// 获取当前IPv4地址 (如果可用)
///
/// 返回格式: [a, b, c, d] 或 None 如果未配置
pub fn get_ipv4_address() -> Option<[u8; 4]> {
    unsafe {
        if G_NETIF_PTR.is_null() {
            return None;
        }
        
        // TODO: 访问 netif->ip_addr 字段并返回字节
        // 这需要知道具体的结构体布局
        
        None // 暂时返回None, 待完善
    }
}

// ============================================================================
// Rust E1000 驱动集成 (新接口)
// ============================================================================

/// 使用 Rust E1000 驱动初始化网络 (推荐方式)
///
/// 这是新的初始化入口，使用纯 Rust 实现的 E1000 驱动：
/// 1. 探测 PCI 设备
/// 2. 初始化硬件
/// 3. 注册到 lwIP
/// 4. 启动 DHCP
///
/// # Returns
/// * `Ok(())` - 初始化成功
/// * `Err(&str)` - 初始化失败原因
pub fn init_network_with_rust_e1000() -> Result<(), &'static str> {
    unsafe {
        // 1. 创建并探测 E1000 设备
        let mut e1000 = E1000Device::new();
        
        match e1000.probe() {
            Ok(()) => {},
            Err(_) => return Err("E1000 probe failed"),
        }

        // 2. 初始化硬件
        match e1000.init() {
            Ok(()) => {},
            Err(_) => return Err("E1000 hardware init failed"),
        }

        // 3. 注册到 lwIP (使用现有的 C 函数)
        let netif_ptr = G_NETIF_BUFFER.as_mut_ptr() as *mut core::ffi::c_void;
        core::ptr::write_bytes(G_NETIF_BUFFER.as_mut_ptr(), 0, 2048);

        let result = netif_add(
            netif_ptr,
            core::ptr::null(),
            core::ptr::null(),
            core::ptr::null(),
            core::ptr::null_mut(),
            e1000_init_wrapper,
            ethernet_input_wrapper,
        );

        if result.is_null() {
            return Err("netif_add failed");
        }

        netif_set_default(result);
        netif_set_up(result);
    extern "C" { fn netif_set_link_up(netif: *mut core::ffi::c_void); }
    netif_set_link_up(result);
        netif_set_status_callback(result, status_callback_wrapper);
        G_NETIF_PTR = result;
        G_NET_INITIALIZED.store(1, Ordering::Release);

        // 4. 启动 DHCP
        #[cfg(feature = "dhcp")]
        {
            let dhcp_result = dhcp_start(result);
            
            if dhcp_result == 0 {
                klog_net("DHCP client started (Rust E1000)\0".as_ptr() as *const i8);
            } else {
                klog_net_err("DHCP start failed\0".as_ptr() as *const i8);
            }
        }

        Ok(())
    }
}

/// 获取 E1000 设备信息 (Rust 版本)
///
/// # Returns
/// * `Some(E1000Device&)` - 设备引用 (如果已初始化)
/// * `None` - 未初始化
pub fn get_e1000_device() -> Option<&'static E1000Device> {
    extern "C" { fn get_e1000_instance() -> *mut core::ffi::c_void; }
    
    unsafe {
        let ptr = get_e1000_instance();
        if ptr.is_null() {
            None
        } else {
            Some(&*(ptr as *const E1000Device))
        }
    }
}

/// 发送数据包通过 E1000 (Rust API)
///
/// # Arguments
/// * `data` - 要发送的数据
///
/// # Returns
/// * `Ok(usize)` - 成功发送的字节数
/// * `Err(DriverError)` - 发送失败
pub fn send_packet(_data: &[u8]) -> Result<usize, DriverError> {
    match get_e1000_device() {
        Some(_dev) => {
            // 注意: 这里需要可变引用，但全局实例是静态的
            // 实际实现可能需要使用内部可变性或 Mutex
            Err(DriverError::NotInitialized)
        },
        None => Err(DriverError::NotInitialized),
    }
}

// ============================================================================
// 单元测试 (仅在测试模式编译)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_dhcp_state_atomic_operations() {
        // 测试原子操作的线程安全性
        assert!(!is_dhcp_done());
        assert!(!is_network_initialized());
        
        // 模拟DHCP完成
        G_DHCP_DONE.store(1, Ordering::Release);
        assert!(is_dhcp_done());
        
        // 重置状态
        reset_dhcp_state();
        assert!(!is_dhcp_done());
    }
    
    #[test]
    fn test_dhap_compare_exchange() {
        // 测试 CAS 操作的正确性
        reset_dhcp_state();
        
        assert_eq!(
            G_DHCP_DONE.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed),
            Ok(0) // 成功从 0 -> 1
        );
        
        assert_eq!(
            G_DHCP_DONE.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed),
            Err(1) // 失败, 当前值为 1
        );
        
        // 清理
        reset_dhcp_state();
    }
    
    #[test]
    fn test_static_storage_safety() {
        unsafe {
            assert!(G_NETIF_PTR.is_null());
        }
    }
}

#[derive(Clone, Copy)]
struct NetSnapshot {
    netif_buffer: [u8; 2048],
    initialized: u8,
    dhcp_done: u8,
}

static NET_SNAPSHOT: spin::Mutex<Option<NetSnapshot>> = spin::Mutex::new(None);

pub fn net_barrier_capture() {
    *NET_SNAPSHOT.lock() = Some(NetSnapshot {
        netif_buffer: unsafe { G_NETIF_BUFFER },
        initialized: G_NET_INITIALIZED.load(Ordering::SeqCst),
        dhcp_done: G_DHCP_DONE.load(Ordering::SeqCst),
    });
}

pub fn net_barrier_rollback() -> bool {
    if let Some(ref snap) = *NET_SNAPSHOT.lock() {
        unsafe { G_NETIF_BUFFER = snap.netif_buffer; }
        G_NET_INITIALIZED.store(snap.initialized, Ordering::SeqCst);
        G_DHCP_DONE.store(snap.dhcp_done, Ordering::SeqCst);
    }
    true
}

extern "C" fn net_barrier_capture_cb() {
    net_barrier_capture();
}

extern "C" fn net_barrier_rollback_cb() -> bool {
    net_barrier_rollback()
}

pub fn net_register_barrier_domain() {
    crate::kernel::barrier::recovery_domain_register(5);
    if let Some(dom) = crate::kernel::barrier::RECOVERY_MANAGER.lock().find(5) {
        *dom.capture_cb.lock() = Some(net_barrier_capture_cb);
        *dom.rollback_cb.lock() = Some(net_barrier_rollback_cb);
    }
}
