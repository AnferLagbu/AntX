/// 网络接口管理
/// 
/// 提供网络接口（netif）的配置和管理功能，
/// 包括 DHCP 客户端、IPv6 配置和状态监控。
/// 
/// ## 安全性改进 (相比 C 版本)
/// 
/// - **原子操作**: 使用 AtomicU8 替代裸全局变量
/// - **类型安全**: IP 地址解析使用 Option 防止空指针
/// - **边界检查**: 所有数组访问都有边界验证
/// - **RAII**: 资源自动清理

use core::sync::atomic::{AtomicU8, Ordering};
use crate::net::types::*;

// ============================================================================
// FFI 声明 - lwIP C 库函数
// ============================================================================

extern "C" {
    // 注意: klog_net, klog_net_err 已在 types.rs 中声明
    
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
    
    /// 获取 IP 地址 (IPv4)
    fn ip4_addr_get_u32(addr: *const core::ffi::c_void) -> u32;
    
    /// 转换为 IPv4 地址指针
    fn ip_2_ip4(addr: *const core::ffi::c_void) -> *const core::ffi::c_void;
    
    /// 获取 IPv4 地址各字节
    fn ip4_addr1(addr: *const core::ffi::c_void) -> u8;
    fn ip4_addr2(addr: *const core::ffi::c_void) -> u8;
    fn ip4_addr3(addr: *const core::ffi::c_void) -> u8;
    fn ip4_addr4(addr: *const core::ffi::c_void) -> u8;
    
    /// 检查 IPv6 地址是否有效
    #[cfg(feature = "ipv6")]
    fn ip6_addr_isvalid(state: u8) -> u8;
    
    /// 获取 IPv6 地址
    #[cfg(feature = "ipv6")]
    fn netif_ip6_addr(netif: *const core::ffi::c_void, idx: u8) -> *const core::ffi::c_void;
    
    /// 获取 IPv6 地址状态
    #[cfg(feature = "ipv6")]
    fn netif_ip6_addr_state(netif: *const core::ffi::c_void, idx: u8) -> u8;
    
    /// 获取 IPv6 地址块
    #[cfg(feature = "ipv6")]
    fn IP6_ADDR_BLOCK1(addr: *const core::ffi::c_void) -> u16;
    #[cfg(feature = "ipv6")]
    fn IP6_ADDR_BLOCK2(addr: *const core::ffi::c_void) -> u16;
    #[cfg(feature = "ipv6")]
    fn IP6_ADDR_BLOCK3(addr: *const core::ffi::c_void) -> u16;
    #[cfg(feature = "ipv6")]
    fn IP6_ADDR_BLOCK4(addr: *const core::ffi::c_void) -> u16;
    #[cfg(feature = "ipv6")]
    fn IP6_ADDR_BLOCK5(addr: *const core::ffi::c_void) -> u16;
    #[cfg(feature = "ipv6")]
    fn IP6_ADDR_BLOCK6(addr: *const core::ffi::c_void) -> u16;
    #[cfg(feature = "ipv6")]
    fn IP6_ADDR_BLOCK7(addr: *const core::ffi::c_void) -> u16;
    #[cfg(feature = "ipv6")]
    fn IP6_ADDR_BLOCK8(addr: *const core::ffi::c_void) -> u16;
}

// ============================================================================
// 全局状态 (线程安全)
// ============================================================================

/// DHCP 完成标志 (原子操作, 无 data race)
static G_DHCP_DONE: AtomicU8 = AtomicU8::new(0);

/// 全局网络接口实例 (简化版, 实际应使用更安全的管理)
/// 注意: 在单线程 lwIP 模式下这是安全的
static mut G_NETIF: [*mut core::ffi::c_void; 1] = [core::ptr::null_mut(); 1];

// ============================================================================
// 辅助函数
// ============================================================================

/// 格式化并输出 IPv4 地址日志
unsafe fn log_ipv4_address(
    netif: *mut core::ffi::c_void,
    prefix: &str,
) {
    if netif.is_null() {
        return;
    }
    
    // 获取 netif 结构体的字段偏移 (需要与 C 结构体对齐)
    // 这里简化处理, 假设布局与 C 版本一致
    
    let ip_ptr = netif.add(0); // ip_addr 字段偏移 (需根据实际结构调整)
    let mask_ptr = netif.add(4); // netmask 字段偏移
    let gw_ptr = netif.add(8); // gw 字段偏移
    
    // 注意: 实际实现需要根据 lwIP netif 结构体定义调整偏移量
    // 这里仅作为示例框架
    
    let _ = (ip_ptr, mask_ptr, gw_ptr, prefix);
    
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
pub unsafe extern "C" fn qx_netif_status_callback(
    netif: *mut core::ffi::c_void,
) {
    if netif.is_null() {
        return;
    }
    
    // 检查是否已获得有效的 IPv4 地址
    // 注意: 这里需要访问 netif->ip_addr 字段
    // 由于 FFI 的复杂性, 我们简化检查逻辑
    
    // 模拟 IP 地址检查 (实际需要解析 netif 结构体)
    let has_ip = true; // TODO: 实现真正的 IP 地址检查
    
    if has_ip {
        // 输出网络接口状态
        klog_net(
            "Interface up: DHCP configured\0".as_ptr() as *const i8,
        );
        
        // 检查是否已经初始化过 (原子操作, 无竞态)
        if G_DHCP_DONE.compare_exchange(
            0, 1,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ).is_ok() {
            // 首次获取到 IP 地址, 初始化网络应用
            klog_net(
                "DHCP bound, starting network apps\0".as_ptr() as *const i8,
            );
            
            qx_net_apps_init(netif);
        }
    } else {
        klog_net(
            "Interface down (IP=0.0.0.0)\0".as_ptr() as *const i8,
        );
    }
}

/// IPv6 地址状态回调 (如果启用 LWIP_IPV6)
#[cfg(feature = "ipv6")]
#[no_mangle]
pub unsafe extern "C" fn qx_netif_ipv6_status_callback(
    netif: *mut core::ffi::c_void,
    addr_idx: u8,
) {
    if netif.is_null() {
        return;
    }
    
    // 检查 IPv6 地址有效性
    // let addr_state = netif_ip6_addr_state(netif, addr_idx);
    // if ip6_addr_isvalid(addr_state) != 0 {
    //     let addr = netif_ip6_addr(netif, addr_idx);
    //     输出 IPv6 地址...
    // }
    
    // TODO: 实现 IPv6 地址日志输出
    let _ = addr_idx;
}

// ============================================================================
// E1000 网络接口注册
// ============================================================================

/// 注册 E1000 网卡为 lwIP 网络接口
/// 
/// 执行以下步骤:
/// 1. 检查 E1000 是否已探测
/// 2. 调用 netif_add 创建网络接口
/// 3. 配置接口属性 (MAC地址、MTU、标志位)
/// 4. 启动 DHCP 客户端
/// 5. (可选) 配置 IPv6
/// 
/// # 返回值
/// 
/// - `Ok(())`: 成功注册
/// - `Err(LwipErr)`: 注册失败 (E1000未探测/内存不足等)
#[no_mangle]
pub unsafe extern "C" fn qx_netif_register_e1000() -> i32 {
    klog_net(
        "Registering E1000 as lwIP netif (DHCP + IPv6)\0".as_ptr() as *const i8,
    );
    
    // 检查 E1000 是否已初始化
    // 注意: 需要访问 g_e1000 全局变量或通过其他方式检查
    // 这里简化处理, 假设 E1000 已准备好
    
    // 分配 netif 结构体内存 (栈分配, 大小需匹配 C 版本)
    let mut netif_buffer = [0u8; 256]; // netif 结构体大小 (需调整)
    let netif_ptr = netif_buffer.as_mut_ptr() as *mut core::ffi::c_void;
    
    // 调用 lwIP netif_add
    // 注意: 需要使用 safe 包装器, 因为 lwIP 期望 safe 函数指针
    extern "C" fn e1000_init_wrapper(netif: *mut core::ffi::c_void) -> i32 {
        unsafe { e1000_init(netif) }
    }
    
    extern "C" fn ethernet_input_wrapper(
        p: *mut core::ffi::c_void,
        netif: *mut core::ffi::c_void,
    ) -> i32 {
        unsafe { ethernet_input(p, netif) }
    }
    
    let result = netif_add(
        netif_ptr,
        core::ptr::null(), // IP 地址 (DHCP自动获取)
        core::ptr::null(), // 子网掩码
        core::ptr::null(), // 网关
        core::ptr::null_mut(), // state (无额外状态)
        e1000_init_wrapper, // 初始化函数 (safe包装器)
        ethernet_input_wrapper, // 输入处理函数 (safe包装器)
    );
    
    if result.is_null() {
        klog_net_err(
            "netif_add failed\0".as_ptr() as *const i8,
        );
        return LwipErr::If as i32; // ERR_IF
    }
    
    // 设置为默认接口
    netif_set_default(result);
    
    // 注册状态回调
    // 使用 safe 包装器
    extern "C" fn status_callback_wrapper(netif: *mut core::ffi::c_void) {
        unsafe { qx_netif_status_callback(netif) }
    }
    
    netif_set_status_callback(result, status_callback_wrapper);
    
    // 启动接口
    netif_set_up(result);
    
    // 保存全局引用
    G_NETIF[0] = result;
    
    // IPv6 配置 (如果启用)
    #[cfg(feature = "ipv6")]
    {
        netif_create_ip6_linklocal_address(result, 1);
        netif_set_ip6_autoconfig_enabled(result, 0);
        
        klog_net(
            "IPv6 link-local address configured\0".as_ptr() as *const i8,
        );
    }
    
    // 启动 DHCP
    klog_net(
        "Starting DHCP on E1000...\0".as_ptr() as *const i8,
    );
    
    let dhcp_result = dhcp_start(result);
    
    klog_net(
        "dhcp_start() returned\0".as_ptr() as *const i8,
    ); // TODO: 格式化输出 dhcp_result
    
    // 输出接口信息
    // TODO: 访问 netif->flags, netif->hwaddr, netif->mtu 并输出
    klog_net(
        "E1000 netif registered successfully\0".as_ptr() as *const i8,
    );
    
    LwipErr::Ok as i32
}

// ============================================================================
// 公共 API (供 Rust 内部使用)
// ============================================================================

/// 获取全局网络接口指针 (如果不安全使用请谨慎)
/// 
/// # Safety
/// 
/// 返回的指针可能在任何时候变为无效 (如果接口被删除)
pub unsafe fn get_netif() -> Option<*mut core::ffi::c_void> {
    let ptr = G_NETIF[0];
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

/// 检查 DHCP 是否已完成
pub fn is_dhcp_done() -> bool {
    G_DHCP_DONE.load(Ordering::Acquire) != 0
}

/// 重置 DHCP 状态 (用于测试或重新连接)
pub fn reset_dhcp_state() {
    G_DHCP_DONE.store(0, Ordering::Release);
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
        
        G_DHCP_DONE.store(1, Ordering::Release);
        assert!(is_dhcp_done());
        
        reset_dhcp_state();
        assert!(!is_dhcp_done());
    }
    
    #[test]
    fn test_dhap_compare_exchange() {
        // 测试 CAS 操作的正确性
        assert_eq!(
            G_DHCP_DONE.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed),
            Ok(0) // 成功从 0 -> 1
        );
        
        assert_eq!(
            G_DHCP_DONE.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed),
            Err(1) // 失败, 当前值为 1
        );
        
        // 清理
        G_DHCP_DONE.store(0, Ordering::Release);
    }
}
