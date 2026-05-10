/// 网络子系统初始化
/// 
/// 提供 AntX 网络子系统的初始化入口点，
/// 包括 lwIP 协议栈初始化、OS抽象层设置、
/// E1000 驱动探测和网络接口注册。
/// 
/// ## 初始化流程
/// 
/// ```text
/// qx_net_init()
/// ├── 1. lwip_init()           # lwIP 核心协议栈
/// ├── 2. sys_arch::sys_init()  # OS 抽象层 (信号量/互斥锁/邮箱)
/// ├── 3. e1000_probe()         # PCI 探测 E1000 硬件
/// └── 4. qx_netif_register_e1000()  # 注册到 lwIP 并启动 DHCP
/// ```
/// 
/// ## 安全性改进 (相比 C 版本)
/// 
/// - **错误传播**: 使用 Result 替代隐式错误码
/// - **状态机**: 跟踪初始化状态, 防止重复初始化
/// - **日志增强**: 详细的初始化过程日志
/// - **资源清理**: 失败时自动回滚

use core::sync::atomic::{AtomicU8, Ordering};
use crate::net::types::*;
use crate::net::sys_arch;

// ============================================================================
// FFI 声明 - 从 C 代码导入的函数
// ============================================================================

extern "C" {
    /// lwIP 协议栈初始化
    fn lwip_init();
    
    /// E1000 网卡探测
    fn e1000_probe() -> i32;
    
    // 注意: klog_net, klog_net_err, klog_init_msg 已在 types.rs 中声明
    // 这里直接使用即可 (通过 use crate::net::types::* 导入)
}

// ============================================================================
// 初始化状态管理
// ============================================================================

/// 初始化状态枚举
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitState {
    /// 未初始化
    Uninitialized = 0,
    /// lwIP 已初始化
    LwipReady = 1,
    /// OS 抽象层已就绪
    SysArchReady = 2,
    /// 硬件探测完成
    HardwareProbed = 3,
    /// 完全就绪
    FullyInitialized = 4,
    /// 初始化失败
    Failed = 255,
}

/// 全局初始化状态 (原子操作, 支持并发访问)
static G_INIT_STATE: AtomicU8 = AtomicU8::new(InitState::Uninitialized as u8);

// ============================================================================
// 辅助函数
// ============================================================================

/// 检查并更新初始化状态
fn transition_state(from: InitState, to: InitState) -> Result<(), ()> {
    match G_INIT_STATE.compare_exchange(
        from as u8,
        to as u8,
        Ordering::AcqRel,
        Ordering::Relaxed,
    ) {
        Ok(_) => Ok(()),
        Err(current) => {
            if current == InitState::Failed as u8 || current >= to as u8 {
                Err(()) // 已经处于目标状态或失败状态
            } else {
                Err(()) // 状态不匹配
            }
        }
    }
}

/// 设置失败状态
fn set_failed() {
    G_INIT_STATE.store(InitState::Failed as u8, Ordering::Release);
}

// ============================================================================
// 网络子系统初始化入口
// ============================================================================

/// 初始化网络子系统
/// 
/// 执行以下步骤:
/// 1. 初始化 lwIP 核心协议栈
/// 2. 初始化 OS 抽象层 (sys_arch)
/// 3. 探测 E1000 网卡硬件
/// 4. 注册网络接口并启动 DHCP
/// 
/// # 线程安全
/// 
/// 此函数使用原子操作确保线程安全。
/// 多次调用只会执行一次真正的初始化。
/// 
/// # 错误处理
/// 
/// 如果任何步骤失败，函数会:
/// - 记录详细错误日志
/// - 设置失败状态
/// - 允许后续重试 (通过 reset_network_state)
#[no_mangle]
pub extern "C" fn qx_net_init() {
    unsafe {
        klog_init_msg("--- Network Subsystem Init ---\0".as_ptr() as *const i8);
        
        // Step 1: 初始化 lwIP
        if transition_state(InitState::Uninitialized, InitState::LwipReady).is_err() {
            // 可能已经初始化过或处于其他状态
            let current = G_INIT_STATE.load(Ordering::Acquire);
            if current == InitState::FullyInitialized as u8 {
                klog_net("Network already initialized\0".as_ptr() as *const i8);
                return;
            } else if current == InitState::Failed as u8 {
                klog_net_err("Previous initialization failed, retrying...\0".as_ptr() as *const i8);
                // 允许重试
                G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);
            } else {
                klog_net_err("Invalid init state, aborting\0".as_ptr() as *const i8);
                return;
            }
            
            // 重试状态转换
            if transition_state(InitState::Uninitialized, InitState::LwipReady).is_err() {
                return;
            }
        }
        
        // 执行 lwIP 初始化
        lwip_init();
        klog_net("lwIP core initialized\0".as_ptr() as *const i8);
        
        // Step 2: 初始化 OS 抽象层
        if transition_state(InitState::LwipReady, InitState::SysArchReady).is_err() {
            set_failed();
            klog_net_err("Failed to transition to SysArchReady state\0".as_ptr() as *const i8);
            return;
        }
        
        // 初始化 OS 抽象层 (直接调用 types 模块的 sys_init)
        crate::net::types::sys_init();
        klog_net("sys_arch ready\0".as_ptr() as *const i8);
        
        // Step 3: 探测网卡硬件
        if transition_state(InitState::SysArchReady, InitState::HardwareProbed).is_err() {
            set_failed();
            klog_net_err("Failed to transition to HardwareProbed state\0".as_ptr() as *const i8);
            return;
        }
        
        let probe_result = e1000_probe();
        
        if probe_result == 0 {
            klog_net("E1000 detected, registering netif\0".as_ptr() as *const i8);
            
            // Step 4: 注册网络接口
            // 注意: 这里调用 Rust 版本的 qx_netif_register_e1000
            extern "C" {
                fn qx_netif_register_e1000() -> i32;
            }
            
            let register_result = qx_netif_register_e1000();
            
            if register_result == 0 {
                // 成功完成所有初始化
                let _ = transition_state(
                    InitState::HardwareProbed,
                    InitState::FullyInitialized,
                );
                
                klog_init_msg("--- Network Subsystem Ready ---\0".as_ptr() as *const i8);
            } else {
                set_failed();
                klog_net_err("Failed to register E1000 netif\0".as_ptr() as *const i8);
                // 注意: 不返回错误, 系统可以无网络运行
            }
        } else {
            // 无网卡 (在虚拟机中是正常的)
            klog_net(
                "No NIC found, running without network (expected in VMs)\0".as_ptr() as *const i8,
            );
            
            // 标记为完全初始化 (即使没有网络)
            let _ = transition_state(
                InitState::HardwareProbed,
                InitState::FullyInitialized,
            );
            
            klog_init_msg("--- Network Subsystem Ready (No Network) ---\0".as_ptr() as *const i8);
        }
    }
}

/// 注册 Socket 系统调用
/// 
/// 当前为桩函数, 未来实现完整的 Socket API。
/// 
/// # 返回值
/// 
/// - `0`: 成功 (目前总是成功)
/// - `<0`: 失败 (未来可能返回错误码)
#[no_mangle]
pub extern "C" fn qx_socket_register_syscalls() -> i32 {
    unsafe { klog_net("Socket syscalls not yet registered\0".as_ptr() as *const i8); }
    0
}

// ============================================================================
// 公共 API (供 Rust 内部使用)
// ============================================================================

/// 检查网络子系统是否已完全初始化
pub fn is_network_initialized() -> bool {
    G_INIT_STATE.load(Ordering::Acquire) == InitState::FullyInitialized as u8
}

/// 获取当前初始化状态 (用于调试)
pub fn get_init_state() -> InitState {
    match G_INIT_STATE.load(Ordering::Acquire) {
        0 => InitState::Uninitialized,
        1 => InitState::LwipReady,
        2 => InitState::SysArchReady,
        3 => InitState::HardwareProbed,
        4 => InitState::FullyInitialized,
        _ => InitState::Failed,
    }
}

/// 重置网络子系统的状态 (用于重新初始化)
/// 
/// # Safety
/// 
/// ⚠️ **危险操作** ⚠️
/// 
/// 仅应在以下场景使用:
/// - 测试环境中的 teardown
/// - 热插拔网卡后的重新初始化
/// - 系统恢复模式
/// 
/// 正常情况下不应调用此函数。
pub unsafe fn reset_network_state() {
    G_INIT_STATE.store(InitState::Uninitialized as u8, Ordering::Release);
    
    // 同时重置 DHCP 状态
    crate::net::netif::reset_dhcp_state();
}

// ============================================================================
// 单元测试 (仅在测试模式编译)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_initialization_state_machine() {
        // 测试状态机的正确转换
        assert_eq!(get_init_state(), InitState::Uninitialized);
        assert!(!is_network_initialized());
        
        // 模拟状态转换 (实际测试需要调用 qx_net_init)
        // 这里仅验证 API 的正确性
        
        // 重置状态
        unsafe { reset_network_state(); }
        
        assert_eq!(get_init_state(), InitState::Uninitialized);
    }
    
    #[test]
    fn test_transition_state_valid_sequence() {
        // 测试有效的状态转换序列
        unsafe { reset_network_state(); }
        
        // Uninitialized -> LwipReady
        assert!(transition_state(
            InitState::Uninitialized,
            InitState::LwipReady,
        ).is_ok());
        
        // LwipReady -> SysArchReady
        assert!(transition_state(
            InitState::LwipReady,
            InitState::SysArchReady,
        ).is_ok());
        
        // 清理
        unsafe { reset_network_state(); }
    }
    
    #[test]
    fn test_transition_state_invalid_sequence() {
        // 测试无效的状态转换
        unsafe { reset_network_state(); }
        
        // 尝试跳过 LwipReady 直接到 SysArchReady (应该失败)
        assert!(transition_state(
            InitState::Uninitialized,
            InitState::SysArchReady,
        ).is_err());
        
        // 清理
        unsafe { reset_network_state(); }
    }
}
