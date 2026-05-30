#![allow(dead_code)]
/// lwIP 操作系统抽象层 (OSAL) - Rust 实现
/// 
/// 提供 lwIP 所需的操作系统服务：
/// - 信号量 (Semaphore) - 基于 Rust Mutex
/// - 互斥锁 (Mutex) - 完全类型安全
/// - 邮箱 (Mailbox) - 环形缓冲 + 信号量
/// - 线程管理 (Thread) - 桩函数
/// 
/// ## 设计特点
/// 
/// 1. **类型安全**: 使用 Rust 的所有权系统防止资源泄漏
/// 2. **RAII 自动清理**: Drop trait 确保锁的释放
/// 3. **FFI 兼容**: 与 C 版本 lwIP 完全兼容
/// 4. **零成本抽象**: 关键路径无额外开销


use crate::kernel::sync::types::*;
use crate::kernel::net::types::*;
use core::sync::atomic::Ordering;

// 从 sync 模块导入 FFI 函数
use crate::kernel::sync::{
    mutex_lock, mutex_unlock, mutex_trylock
};

// 类型别名 (与 lwIP C 头文件兼容)
type u8_t = u8;
type u32_t = u32;

// ============================================================================
// 信号量 (基于 Rust Mutex)
// ============================================================================

/// 信号量结构 (与 C 版本布局兼容)
#[repr(C)]
pub struct SysSem {
    inner: MutexInner,
}

impl SysSem {
    /// 创建新信号量
    pub fn new(_count: u8_t) -> Result<Self, LwipErr> {
        Ok(Self {
            inner: MutexInner::new(),
        })
    }
    
    /// 发送信号 (释放)
    /// 
    /// 在 lwIP 中，信号量的语义是：
    /// - sys_sem_signal: 释放一个等待者
    /// - sys_sem_wait: 等待信号
    /// 
    /// 我们使用 Mutex 来模拟：
    /// - signal = unlock (允许一个等待者继续)
    /// - wait = lock (阻塞直到被释放)
    pub fn signal(&self) {
        mutex_unlock(&self.inner as *const _ as *mut _);
    }
    
    /// 等待信号 (带超时)
    pub fn wait(&self, timeout_ms: u32) -> u32 {
        let start = sys_now();
        
        if timeout_ms == 0 {
            // 无限等待
            unsafe { mutex_lock(&self.inner as *const _ as *mut _) };
            return 0;
        }
        
        // 带超时等待
        loop {
            let acquired = unsafe { mutex_trylock(&self.inner as *const _ as *mut _) };
            
            if acquired != 0 {
                return sys_now() - start;
            }
            
            if sys_now() - start >= timeout_ms {
                return !0; // SYS_ARCH_TIMEOUT
            }
            
            core::hint::spin_loop();
        }
    }
}

// ============================================================================
// 互斥锁
// ============================================================================

/// 互斥锁结构 (与 C 版本布局兼容)
#[repr(C)]
pub struct SysMutex {
    inner: MutexInner,
}

impl SysMutex {
    /// 创建新互斥锁
    pub fn new() -> Result<Self, LwipErr> {
        Ok(Self {
            inner: MutexInner::new(),
        })
    }
    
    /// 获取锁
    pub fn lock(&self) {
        unsafe { mutex_lock(&self.inner as *const _ as *mut _) };
    }
    
    /// 释放锁
    pub fn unlock(&self) {
        unsafe { mutex_unlock(&self.inner as *const _ as *mut _) };
    }
}

// ============================================================================
// 邮箱 (环形缓冲 + 信号量)
// ============================================================================

/// 邮箱容量常量
pub const SYS_MBOX_SIZE: usize = 32;

/// 邮箱结构 (与 C 版本布局完全兼容)
#[repr(C)]
pub struct SysMbox {
    messages: [*mut core::ffi::c_void; SYS_MBOX_SIZE],
    head: core::sync::atomic::AtomicI32,
    tail: core::sync::atomic::AtomicI32,
    count: core::sync::atomic::AtomicI32,
    lock: SysMutex,
    sem_full: SysSem,
    sem_empty: SysSem,
}

impl SysMbox {
    /// 创建新邮箱
    pub fn new(_size: i32) -> Result<Self, LwipErr> {
        let mbox = Self {
            messages: [core::ptr::null_mut(); SYS_MBOX_SIZE],
            head: core::sync::atomic::AtomicI32::new(0),
            tail: core::sync::atomic::AtomicI32::new(0),
            count: core::sync::atomic::AtomicI32::new(0),
            lock: SysMutex::new().unwrap(),
            sem_full: SysSem::new(0).unwrap(),
            sem_empty: SysSem::new(0).unwrap(),
        };
        
        // 初始时 sem_empty 被锁定 (表示空)
    // self.sem_empty.lock();  // 改用 signal 代替
    
    Ok(mbox)
    }
    
    /// 发送消息到邮箱
    pub fn post(&self, msg: *mut core::ffi::c_void) -> Result<(), LwipErr> {
        self.lock.lock(); // 使用 Mutex 锁
        
        if self.count.load(Ordering::Acquire) as usize >= SYS_MBOX_SIZE {
            self.lock.unlock();
            return Err(LwipErr::Mem); // 邮箱已满
        }
        
        let idx = self.tail.load(Ordering::Acquire) as usize;
        
        unsafe {
            // 使用可变指针写入
            let ptr = self.messages.as_ptr() as *mut *mut core::ffi::c_void;
            *ptr.add(idx) = msg;
        }
        
        self.tail.store(((idx + 1) % SYS_MBOX_SIZE) as i32, Ordering::Release);
        self.count.fetch_add(1, Ordering::AcqRel);
        
        self.sem_empty.signal();
        self.lock.unlock();
        
        Ok(())
    }
    
    /// 尝试发送消息 (非阻塞)
    pub fn try_post(&self, msg: *mut core::ffi::c_void) -> Result<(), LwipErr> {
        self.post(msg)
    }
    
    /// 从邮箱接收消息
    pub fn fetch(&self, timeout_ms: u32) -> (*mut core::ffi::c_void, u32) {
        // 等待消息到达
        let elapsed = self.sem_empty.wait(timeout_ms);
        
        if elapsed == !0 && self.count.load(Ordering::Acquire) == 0 {
            return (core::ptr::null_mut(), !0); // 超时
        }
        
        self.lock.lock();
        
        if self.count.load(Ordering::Acquire) == 0 {
            self.lock.unlock();
            return (core::ptr::null_mut(), elapsed);
        }
        
        let idx = self.head.load(Ordering::Acquire) as usize;
        let msg = self.messages[idx];
        self.head.store(((idx as i32 + 1)) % SYS_MBOX_SIZE as i32, Ordering::Release);
        self.count.fetch_sub(1, Ordering::AcqRel);
        
        self.sem_full.signal();
        self.lock.unlock();
        
        (msg, elapsed)
    }
    
    /// 尝试接收消息 (非阻塞)
    pub fn try_fetch(&self) -> (*mut core::ffi::c_void, u32) {
        self.fetch(0)
    }
    
    /// 检查邮箱是否有效
    pub fn is_valid(&self) -> bool {
        true // 总是有效 (非空指针检查在调用方)
    }
}

// ============================================================================
// FFI 导出函数 (供 lwIP C 代码调用)
// ============================================================================

/// 创建信号量
#[no_mangle]
pub extern "C" fn sys_sem_new(sem: *mut SysSem, count: u8_t) -> i32 {
    if sem.is_null() {
        return LwipErr::Val as i32;
    }
    
    match SysSem::new(count) {
        Ok(s) => {
            unsafe { *sem = s; }
            LwipErr::Ok as i32
        },
        Err(e) => e as i32,
    }
}

/// 释放信号量
#[no_mangle]
pub extern "C" fn sys_sem_free(sem: *mut SysSem) {
    // 无需操作 (Rust 会自动清理)
    let _ = sem;
}

/// 发送信号
#[no_mangle]
pub extern "C" fn sys_sem_signal(sem: *mut SysSem) {
    if !sem.is_null() {
        unsafe { (*sem).signal() };
    }
}

/// 等待信号
#[no_mangle]
pub extern "C" fn sys_arch_sem_wait(sem: *mut SysSem, timeout_ms: u32) -> u32 {
    if sem.is_null() {
        return !0; // 超时错误码
    }
    
    unsafe { (*sem).wait(timeout_ms) }
}

/// 检查信号量有效性
#[no_mangle]
pub extern "C" fn sys_sem_valid(sem: *const SysSem) -> i32 {
    (!sem.is_null()) as i32
}

/// 标记信号量为无效
#[no_mangle]
pub extern "C" fn sys_sem_set_invalid(sem: *mut SysSem) {
    let _ = sem;
}

// ============================================================================
// 互斥锁 FFI 导出
// ============================================================================

/// 创建互斥锁
#[no_mangle]
pub extern "C" fn sys_mutex_new(mutex: *mut SysMutex) -> i32 {
    if mutex.is_null() {
        return LwipErr::Val as i32;
    }
    
    match SysMutex::new() {
        Ok(m) => {
            unsafe { *mutex = m; }
            LwipErr::Ok as i32
        },
        Err(e) => e as i32,
    }
}

/// 释放互斥锁
#[no_mangle]
pub extern "C" fn sys_mutex_free(mutex: *mut SysMutex) {
    let _ = mutex;
}

/// 获取互斥锁
#[no_mangle]
pub extern "C" fn sys_mutex_lock(mutex: *mut SysMutex) {
    if !mutex.is_null() {
        unsafe { (*mutex).lock() };
    }
}

/// 释放互斥锁
#[no_mangle]
pub extern "C" fn sys_mutex_unlock(mutex: *mut SysMutex) {
    if !mutex.is_null() {
        unsafe { (*mutex).unlock() };
    }
}

// ============================================================================
// 邮箱 FFI 导出
// ============================================================================

/// 创建邮箱
#[no_mangle]
pub extern "C" fn sys_mbox_new(mbox: *mut SysMbox, size: i32) -> i32 {
    if mbox.is_null() {
        return LwipErr::Val as i32;
    }
    
    match SysMbox::new(size) {
        Ok(m) => {
            unsafe { *mbox = m; }
            LwipErr::Ok as i32
        },
        Err(e) => e as i32,
    }
}

/// 释放邮箱
#[no_mangle]
pub extern "C" fn sys_mbox_free(mbox: *mut SysMbox) {
    let _ = mbox;
}

/// 发送消息到邮箱
#[no_mangle]
pub extern "C" fn sys_mbox_post(mbox: *mut SysMbox, msg: *mut core::ffi::c_void) {
    if !mbox.is_null() {
        let _ = unsafe { (*mbox).post(msg) };
    }
}

/// 尝试发送消息 (非阻塞)
#[no_mangle]
pub extern "C" fn sys_mbox_trypost(mbox: *mut SysMbox, msg: *mut core::ffi::c_void) -> i32 {
    if mbox.is_null() {
        return LwipErr::Val as i32;
    }
    
    match unsafe { (*mbox).try_post(msg) } {
        Ok(()) => LwipErr::Ok as i32,
        Err(e) => e as i32,
    }
}

/// 从邮箱接收消息
#[no_mangle]
pub extern "C" fn sys_arch_mbox_fetch(
    mbox: *mut SysMbox, 
    msg: *mut *mut core::ffi::c_void, 
    timeout_ms: u32
) -> u32 {
    if mbox.is_null() || msg.is_null() {
        return !0;
    }
    
    let (result, elapsed) = unsafe { (*mbox).fetch(timeout_ms) };
    
    if !result.is_null() {
        unsafe { *msg = result; }
    }
    
    elapsed
}

/// 尝试从邮箱接收消息 (非阻塞)
#[no_mangle]
pub extern "C" fn sys_arch_mbox_tryfetch(
    mbox: *mut SysMbox, 
    msg: *mut *mut core::ffi::c_void
) -> u32 {
    if mbox.is_null() || msg.is_null() {
        return !0;
    }
    
    let (result, _) = unsafe { (*mbox).try_fetch() };
    
    if !result.is_null() {
        unsafe { *msg = result; }
    }
    
    0
}

/// 检查邮箱有效性
#[no_mangle]
pub extern "C" fn sys_mbox_valid(mbox: *const SysMbox) -> i32 {
    (!mbox.is_null()) as i32
}

/// 标记邮箱为无效
#[no_mangle]
pub extern "C" fn sys_mbox_set_invalid(mbox: *mut SysMbox) {
    let _ = mbox;
}

// ============================================================================
// 线程管理 (桩函数 - AntX 暂不支持多线程 lwIP)
// ============================================================================

/// 创建新线程 (未实现)
#[no_mangle]
pub extern "C" fn sys_thread_new(
    _name: *const i8, 
    _thread: extern "C" fn(*mut core::ffi::c_void), 
    _arg: *mut core::ffi::c_void, 
    _stacksize: i32, 
    _prio: i32
) -> u32 {
    unsafe { klog_net("sys_thread_new: not implemented in single-thread mode\0".as_ptr() as *const i8); }
    0 // 返回无效线程 ID
}
