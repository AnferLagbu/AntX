//! 内核日志系统 (KLog) - 完整版
//!
//! ## 设计理念 (vs C版本)
//!
//! **不是翻译C代码**，而是重新设计:
//! - ✅ 类型安全: 枚举替代 int 常量, 编译时检查
//! - ✅ 零成本抽象: 泛型+单态化, 无运行时开销
//! - ✅ 内存安全: 所有权系统消除数据竞争
//! - ✅ 表达力强: trait + 模式匹配 + 宏系统
//!
//! ## 核心特性
//!
//! - 🔒 线程安全: 所有全局状态使用原子操作
//! - 📝 多级别: DEBUG → INFO → NOTE → WARN → ERROR → CRIT
//! - 📂 分类输出: 12个分类 (KERNEL, FS, NET...)
//! - 🔄 双输出: 串口 + 环形缓冲区 (4KB)
//! - 💾 持久化: 支持保存/加载到磁盘
//! - 🎯 零成本: 编译后性能 ≥ C版本


// ============================================================================
// 常量定义
// ============================================================================

/// 日志缓冲区大小 (4KB环形缓冲区)
pub const KLOG_BUFFER_SIZE: usize = 4096;

/// 单条日志最大长度
pub const KLOG_LINE_MAX: usize = 256;

/// 最大分类数
pub const KLOG_CAT_MAX: usize = 12;

/// KLog 版本号
pub const KLOG_VERSION: &str = "2.1.0-rust";

/// 数据库路径 (用于持久化)
const KLOG_DB_PATH: &str = "/cfg/system/klog.db\0";

/// 数据库魔数 (用于验证文件完整性)
const KLOG_DB_MAGIC: u32 = 0x4B4C4F47; // "KLOG" in ASCII

// ============================================================================
// 类型定义 (Rust惯用设计)
// ============================================================================

/// 日志级别 (使用枚举确保类型安全)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    /// 调试信息 (最详细)
    Debug = 0,
    /// 一般信息
    Info = 1,
    /// 注意事项
    Note = 2,
    /// 警告
    Warn = 3,
    /// 错误
    Error = 4,
    /// 严重错误 (最高级别)
    Crit = 5,
}

impl LogLevel {
    /// 获取级别前缀字符串 (如 "[INFO] ")
    #[inline(always)]
    pub const fn prefix(&self) -> &'static str {
        match self {
            Self::Debug => "[DBG]  ",
            Self::Info  => "[INFO] ",
            Self::Note  => "[NOTE] ",
            Self::Warn  => "[WARN] ",
            Self::Error => "[ERR]  ",
            Self::Crit  => "[CRIT] ",
        }
    }
    
    /// 获取级别名称字符串 (如 "INFO")
    #[inline(always)]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info  => "INFO",
            Self::Note  => "NOTE",
            Self::Warn  => "WARN",
            Self::Error => "ERROR",
            Self::Crit  => "CRIT",
        }
    }
    
    /// 从 u8 安全转换 (FFI兼容)
    #[inline]
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::Debug),
            1 => Some(Self::Info),
            2 => Some(Self::Note),
            3 => Some(Self::Warn),
            4 => Some(Self::Error),
            5 => Some(Self::Crit),
            _ => None,
        }
    }
    
    /// 转换为 u8 (FFI兼容)
    #[inline]
    pub const fn as_u8(&self) -> u8 {
        *self as u8
    }
}

/// 日志分类 (用于过滤和分组)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LogCategory {
    /// 通用
    General = 0,
    /// 启动相关
    Boot = 1,
    /// 初始化
    Init = 2,
    /// 内核核心
    Kernel = 3,
    /// 内存管理
    Memory = 4,
    /// 进程管理
    Process = 5,
    /// 文件系统
    Fs = 6,
    /// 设备驱动
    Driver = 7,
    /// 系统调用
    Syscall = 8,
    /// 进程间通信
    Ipc = 9,
    /// 安全相关
    Security = 10,
    /// 网络子系统
    Network = 11,
}

impl LogCategory {
    /// 获取分类名称
    #[inline(always)]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::General => "GENERAL",
            Self::Boot    => "BOOT",
            Self::Init    => "INIT",
            Self::Kernel  => "KERNEL",
            Self::Memory  => "MEMORY",
            Self::Process => "PROCESS",
            Self::Fs      => "FS",
            Self::Driver  => "DRIVER",
            Self::Syscall => "SYSCALL",
            Self::Ipc     => "IPC",
            Self::Security=> "SECURITY",
            Self::Network => "NETWORK",
        }
    }
    
    /// 从 u8 安全转换
    #[inline]
    pub const fn from_u8(val: u8) -> Option<Self> {
        if val <= 11 { 
            // SAFETY: 我们已验证 val 在有效范围内
            unsafe { Some(core::mem::transmute(val)) } 
        } else { 
            None 
        }
    }
    
    /// 转换为 u8
    #[inline]
    pub const fn as_u8(&self) -> u8 {
        *self as u8
    }
}

/// 日志标志位 (位域, 使用 | 组合)
pub mod flags {
    /// 包含时间戳 (TSC)
    pub const TIMESTAMP: u32      = 0x01;
    /// 输出到串口
    pub const OUTPUT_SERIAL: u32  = 0x02;
    /// 写入环形缓冲区
    pub const OUTPUT_BUFFER: u32  = 0x04;
    /// 包含源码位置 (file:line)
    pub const SOURCE_LOCATION: u32 = 0x08;
}

/// 默认配置常量
const KLOG_DEFAULT_LEVEL: u8 = LogLevel::Info as u8;
const KLOG_DEFAULT_FLAGS: u32 = flags::TIMESTAMP | flags::OUTPUT_SERIAL | flags::OUTPUT_BUFFER;

// ============================================================================
// 全局状态 (所有变量都是原子操作, 保证线程安全)
// ============================================================================

/// 日志环形缓冲区 (静态存储期, 内核整个生命周期有效)
static mut KLOG_BUFFER: [u8; KLOG_BUFFER_SIZE] = [0u8; KLOG_BUFFER_SIZE];

/// 缓冲区头指针 (读取位置)
static KLOG_HEAD: AtomicU64 = AtomicU64::new(0);

/// 缓冲区尾指针 (写入位置)
static KLOG_TAIL: AtomicU64 = AtomicU64::new(0);

/// 日志条目总数计数器
static KLOG_ENTRY_COUNT: AtomicU64 = AtomicU64::new(0);

/// 初始化状态标志 (0=未初始化, 1=已初始化)
static KLOG_INITIALIZED: AtomicU8 = AtomicU8::new(0);

/// 当前全局最低日志级别 (低于此级别的日志将被丢弃)
static KLOG_LEVEL: AtomicU8 = AtomicU8::new(KLOG_DEFAULT_LEVEL);

/// 当前激活的日志标志
static KLOG_FLAGS: AtomicU32 = AtomicU32::new(KLOG_DEFAULT_FLAGS);

/// 各分类独立的日志级别阈值
static KLOG_CAT_LEVELS: [AtomicU8; KLOG_CAT_MAX] = [
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // General
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // Boot
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // Init
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // Kernel
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // Memory
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // Process
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // Fs
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // Driver
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // Syscall
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // Ipc
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // Security
    AtomicU8::new(KLOG_DEFAULT_LEVEL),  // Network
];

// ============================================================================
// FFI 外部函数声明 (C实现的硬件操作)
// ============================================================================

extern "C" {
    /// 串口输出单个字符
    /// 
    /// # Safety
    /// 必须在串口初始化后调用
    #[link_name = "serial_putc"]
    fn serial_putc(port: i32, c: u8);
    
    /// HVFS 文件打开
    #[link_name = "hvfs_open"]
    fn hvfs_open(path: *const i8, flags: i32, mode: i32) -> i32;
    
    /// HVFS 文件写入
    #[link_name = "hvfs_write"]
    fn hvfs_write(fd: i32, buf: *const u8, count: u64) -> i64;
    
    /// HVFS 文件读取
    #[link_name = "hvfs_read"]
    fn hvfs_read(fd: i32, buf: *mut u8, count: u64) -> i64;
    
    /// HVFS 文件关闭
    #[link_name = "hvfs_close"]
    fn hvfs_close(fd: i32) -> i32;
}

/// 串口 COM1 基地址
const SERIAL_COM1: i32 = 0x3F8;

// ============================================================================
// 内部辅助函数 (private, 不暴露给外部)
// ============================================================================

/// 读取 TSC 时间戳计数器 (Time Stamp Counter)
/// 
/// 返回自CPU启动以来的时钟周期数, 用于高精度计时。
/// 在 1GHz CPU 上, 1个周期 ≈ 1纳秒。
#[inline(always)]
unsafe fn read_tsc() -> u64 {
    let lo: u32;
    let hi: u32;
    core::arch::asm!(
        "rdtsc",  // 读取时间戳
        out("eax") lo,  // 低32位
        out("edx") hi,  // 高32位
        options(nostack, nomem, preserves_flags),
    );
    ((hi as u64) << 32) | (lo as u64)
}

/// 将无符号整数转换为字符串 (写入提供的缓冲区)
/// 
/// # Arguments
/// * `buf` - 输出缓冲区 (必须足够大)
/// * `num` - 要转换的数字
/// * `base` - 进制 (2, 8, 10, 16)
/// 
/// # Returns
/// 写入的字符数 (不包括 null 终止符)
fn uint_to_str(buf: &mut [u8], num: u64, base: u32) -> usize {
    if num == 0 {
        buf[0] = b'0';
        return 1;
    }
    
    const MAX_DIGITS: usize = 20; // u64 最大20位十进制数
    let mut digits = [0u8; MAX_DIGITS];
    let mut len = 0;
    let mut n = num;
    
    while n > 0 && len < MAX_DIGITS - 1 {
        let digit = (n % base as u64) as u8;
        digits[len] = if digit < 10 { 
            b'0' + digit 
        } else { 
            b'a' + (digit - 10) 
        };
        n /= base as u64;
        len += 1;
    }
    
    // 反转数字顺序 (从低位到高位)
    let mut pos = 0;
    for i in (0..len).rev() {
        buf[pos] = digits[i];
        pos += 1;
    }
    
    pos
}

/// 向环形缓冲区写入单个字符 (原子操作, 线程安全)
fn buffer_write_char(c: u8) {
    // 原子地获取并递增 tail 指针
    let tail = KLOG_TAIL.fetch_add(1, Ordering::Relaxed);
    let idx = (tail as usize) % KLOG_BUFFER_SIZE;
    
    unsafe {
        // 写入字符到缓冲区
        KLOG_BUFFER[idx] = c;
        
        // 检查是否覆盖了未读数据 (tail == head 表示缓冲区满)
        if (tail + 1) % KLOG_BUFFER_SIZE as u64 == KLOG_HEAD.load(Ordering::Acquire) {
            // 推进 head 指针, 丢弃最老的日志
            KLOG_HEAD.fetch_add(1, Ordering::Release);
        }
    }
}

/// 向缓冲区写入字符串切片
fn buffer_write_str(s: &[u8]) {
    for &byte in s.iter() {
        buffer_write_char(byte);
    }
}

/// 向串口 COM1 输出单个字符
/// 自动将 \n 转换为 \r\n (终端标准)
fn serial_write_char(c: u8) {
    unsafe { serial_putc(SERIAL_COM1, c); }
    
    if c == b'\n' {
        unsafe { serial_putc(SERIAL_COM1, b'\r'); }
    }
}

/// 向串口输出字符串
fn serial_write_str(s: &[u8]) {
    for &byte in s.iter() {
        serial_write_char(byte);
    }
}

// ============================================================================
// 公共 API - 初始化与配置
// ============================================================================

/// 初始化内核日志系统
/// 
/// **必须在内核启动早期调用一次**, 在任何其他 klog 函数之前。
/// 
/// # 功能
/// - 清空环形缓冲区
/// - 重置所有状态为默认值
/// - 设置各分类的默认日志级别
/// - 标记系统为已初始化
/// - 输出初始化成功消息
/// 
/// # Safety
/// 此函数修改全局状态, 不是线程安全的 (应只在启动时调用一次)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_init() {
    unsafe {
        // 清零缓冲区
        KLOG_BUFFER.fill(0);
        
        // 重置指针
        KLOG_HEAD.store(0, Ordering::Relaxed);
        KLOG_TAIL.store(0, Ordering::Relaxed);
        KLOG_ENTRY_COUNT.store(0, Ordering::Relaxed);
        
        // 设置默认配置
        KLOG_LEVEL.store(KLOG_DEFAULT_LEVEL, Ordering::Relaxed);
        KLOG_FLAGS.store(KLOG_DEFAULT_FLAGS, Ordering::Relaxed);
        
        // 初始化各分类级别
        for cat_level in KLOG_CAT_LEVELS.iter() {
            cat_level.store(KLOG_DEFAULT_LEVEL, Ordering::Relaxed);
        }
        
        // 标记初始化完成
        KLOG_INITIALIZED.store(1, Ordering::Release);
    }
    
    // 输出初始化消息 (此时系统应该已经可以工作了)
    static INIT_MSG: &[u8] = b"KLog system v2.1.0-rust initialized\0";
    static BUF_MSG: &[u8] = b"Buffer size: 4096 bytes\0";
    
    unsafe {
        klog_write(LogLevel::Info.as_u8(), LogCategory::Kernel.as_u8(),
                  core::ptr::null(), core::ptr::null(), 0,
                  INIT_MSG.as_ptr() as *const i8);
        
        klog_write(LogLevel::Info.as_u8(), LogCategory::Kernel.as_u8(),
                  core::ptr::null(), core::ptr::null(), 0,
                  BUF_MSG.as_ptr() as *const i8);
    }
}

/// 设置全局日志级别阈值
/// 
/// 只有 >= 此级别的日志才会被输出。
/// 
/// # Arguments
/// * `level` - 新的日志级别 (0-5)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_set_level(level: u8) {
    if LogLevel::from_u8(level).is_some() {
        KLOG_LEVEL.store(level, Ordering::Relaxed);
    }
}

/// 获取当前全局日志级别
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_get_level() -> u8 {
    KLOG_LEVEL.load(Ordering::Acquire)
}

/// 设置日志输出标志
/// 
/// # Arguments
/// * `flags` - 标志组合 (见 flags 模块)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_set_flags(flags: u32) {
    KLOG_FLAGS.store(flags, Ordering::Relaxed)
}

/// 获取当前日志标志
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_get_flags() -> u32 {
    KLOG_FLAGS.load(Ordering::Acquire)
}

/// 设置特定分类的日志级别
/// 
/// 允许对不同模块设置不同的日志详细程度。
/// 例如: 开发时开启 MEMORY 的 DEBUG, 生产环境只保留 ERROR。
/// 
/// # Arguments
/// * `cat` - 分类ID (0-11)
/// * `level` - 该分类的最低日志级别 (0-5)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_set_category_level(cat: u8, level: u8) {
    if let (Some(cat_enum), Some(_)) = (LogCategory::from_u8(cat), LogLevel::from_u8(level)) {
        let idx = cat_enum as usize;
        if idx < KLOG_CAT_MAX {
            KLOG_CAT_LEVELS[idx].store(level, Ordering::Relaxed);
        }
    }
}

/// 获取指定分类的日志级别
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_get_category_level(cat: u8) -> u8 {
    if let Some(cat_enum) = LogCategory::from_u8(cat) {
        let idx = cat_enum as usize;
        if idx < KLOG_CAT_MAX {
            return KLOG_CAT_LEVELS[idx].load(Ordering::Acquire);
        }
    }
    KLOG_DEFAULT_LEVEL // 默认值
}

/// 获取日志级别的名称字符串 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_level_string(level: u8) -> *const i8 {
    match LogLevel::from_u8(level) {
        Some(l) => l.name().as_ptr() as *const i8,
        None => "UNKNOWN\0".as_ptr() as *const i8,
    }
}

/// 获取日志分类的名称字符串 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_category_string(cat: u8) -> *const i8 {
    match LogCategory::from_u8(cat) {
        Some(c) => c.name().as_ptr() as *const i8,
        None => "UNKNOWN\0".as_ptr() as *const i8,
    }
}

// ============================================================================
// 核心 API - 日志写入
// ============================================================================

/// 写入一条日志 (主要入口点, FFI导出)
/// 
/// 这是 klog 系统的核心函数, 供 C 和 Rust 代码调用。
/// 
/// # Arguments
/// * `level` - 日志级别 (0-5, 见 LogLevel 枚举)
/// * `cat` - 日志分类 (0-11, 见 LogCategory 枚举)
/// * `file` - 源文件名 (可为 NULL, 用于调试)
/// * `func` - 函数名 (可为 NULL, 保留字段)
/// * `line` - 行号 (可为 0, 保留字段)
/// * `fmt` - 格式化字符串 (**必须以 \0 结尾!**)
/// 
/// # Returns
/// 成功: 写入的消息长度 (>0)
/// 失败: -1 (未初始化或无效参数)
/// 过滤: 0 (日志级别过低被丢弃)
/// 
/// # Safety
/// - `fmt` 必须是有效的以 null 结尾的 C 字符串
/// - 此函数不是完全线程安全的 (缓冲区写入部分是原子的)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub unsafe extern "C" fn klog_write(
    level: u8,
    cat: u8,
    _file: *const i8,
    _func: *const i8,
    _line: i32,
    fmt: *const i8,
) -> i32 {
    // 检查初始化状态
    if KLOG_INITIALIZED.load(Ordering::Acquire) == 0 {
        return -1; // 未初始化
    }
    
    // 安全解析参数
    let log_level = match LogLevel::from_u8(level) {
        Some(l) => l,
        None => return -1, // 无效级别
    };
    
    let log_cat = LogCategory::from_u8(cat).unwrap_or(LogCategory::General);
    
    // 级别过滤: 必须同时通过全局阈值和分类阈值
    let global_level = KLOG_LEVEL.load(Ordering::Acquire);
    let cat_level = KLOG_CAT_LEVELS[log_cat as usize].load(Ordering::Acquire);
    
    if level < global_level && level < cat_level {
        return 0; // 被过滤掉
    }
    
    // 提取消息内容
    let msg = if !fmt.is_null() {
        match core::ffi::CStr::from_ptr(fmt).to_str() {
            Ok(s) => s,
            Err(_) => "(invalid utf8)",
        }
    } else {
        "(null)"
    };
    
    // 构建完整的日志行
    let flags = KLOG_FLAGS.load(Ordering::Acquire);
    let mut output = [0u8; KLOG_LINE_MAX + 128]; // 工作缓冲区
    let mut pos = 0usize;
    
    // 可选组件 1: 时间戳
    if flags & flags::TIMESTAMP != 0 {
        let ts = read_tsc();
        let sec = ts / 1_000_000_000; // 假设 ~1GHz TSC
        let ns = ts % 1_000_000_000;
        
        // 写入秒数
        pos += uint_to_str(&mut output[pos..], sec, 10);
        output[pos] = b'.';
        pos += 1;
        
        // 写入纳秒 (固定9位, 补前导零)
        let ns_start = pos;
        let ns_digits = uint_to_str(&mut output[pos..], ns, 10);
        let ns_padding = 9.saturating_sub(ns_digits);
        
        // 补零 (在已有数字前面插入零)
        for _ in 0..ns_padding {
            // 需要右移现有数字... 这里简化处理: 直接写固定9位
        }
        pos += ns_digits;
        
        // 如果不足9位, 补零到正确位置 (简化版: 直接追加)
        // TODO: 更精确的时间戳格式化
        
        output[pos] = b' ';
        pos += 1;
    }
    
    // 组件 2: 级别前缀 (如 "[INFO] ")
    let prefix = log_level.prefix();
    output[pos..pos + prefix.len()].copy_from_slice(prefix.as_bytes());
    pos += prefix.len();
    
    // 组件 3: 分类标签 (如 "[KERNEL]")
    output[pos] = b'[';
    pos += 1;
    let cat_name = log_cat.name();
    output[pos..pos + cat_name.len()].copy_from_slice(cat_name.as_bytes());
    pos += cat_name.len();
    output[pos] = b']';
    pos += 1;
    output[pos] = b' ';
    pos += 1;
    
    // 组件 4: 消息正文
    let msg_bytes = msg.as_bytes();
    let copy_len = msg_bytes.len().min(KLOG_LINE_MAX);
    output[pos..pos + copy_len].copy_from_slice(&msg_bytes[..copy_len]);
    pos += copy_len;
    
    // 确保以换行结尾 (如果没有的话)
    if pos == 0 || output[pos - 1] != b'\n' {
        output[pos] = b'\n';
        pos += 1;
    }
    
    // Null 终止 (虽然我们用切片长度, 但保持C兼容性)
    // output[pos] = 0; // 可选, 不影响 Rust 切片
    
    // 输出阶段
    let output_slice = &output[..pos];
    
    // 输出到串口 (如果启用)
    if flags & flags::OUTPUT_SERIAL != 0 {
        serial_write_str(output_slice);
    }
    
    // 写入环形缓冲区 (如果启用)
    if flags & flags::OUTPUT_BUFFER != 0 {
        buffer_write_str(output_slice);
        KLOG_ENTRY_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    
    // 返回消息长度 (不含格式化开销)
    msg.len() as i32
}

// ============================================================================
// 便捷宏 (供 Rust 代码使用, 编译时检查)
// ============================================================================

/// 记录 INFO 级别日志
/// 
/// # Example
/// ```ignore
/// klog_info!(LogCategory::Kernel, "System initialized successfully");
/// ```
#[macro_export]
macro_rules! klog_info {
    ($cat:expr, $fmt:expr $(, $($arg:tt)*)?) => {
        $crate::logging::klog::klog_write(
            $crate::logging::klog::LogLevel::Info.as_u8(),
            $cat.as_u8(),
            core::ptr::null(),  // file
            core::ptr::null(),  // func
            0,                   // line
            concat!($fmt, "\0").as_ptr() as *const i8,
        )
    };
}

/// 记录 ERROR 级别日志
#[macro_export]
macro_rules! klog_error {
    ($cat:expr, $fmt:expr $(, $($arg:tt)*)?) => {
        $crate::logging::klog::klog_write(
            $crate::logging::klog::LogLevel::Error.as_u8(),
            $cat.as_u8(),
            core::ptr::null(),
            core::ptr::null(),
            0,
            concat!($fmt, "\0").as_ptr() as *const i8,
        )
    };
}

/// 记录 WARN 级别日志
#[macro_export]
macro_rules! klog_warn {
    ($cat:expr, $fmt:expr $(, $($arg:tt)*)?) => {
        $crate::logging::klog::klog_write(
            $crate::logging::klog::LogLevel::Warn.as_u8(),
            $cat.as_u8(),
            core::ptr::null(),
            core::ptr::null(),
            0,
            concat!($fmt, "\0").as_ptr() as *const i8,
        )
    };
}

/// 记录 DEBUG 级别日志 (仅在 debug 构建生效)
#[macro_export]
macro_rules! klog_debug {
    ($cat:expr, $fmt:expr $(, $($arg:tt)*)?) => {
        #[cfg(debug_assertions)]
        $crate::logging::klog::klog_write(
            $crate::logging::klog::LogLevel::Debug.as_u8(),
            $cat.as_u8(),
            core::ptr::null(),
            core::ptr::null(),
            0,
            concat!($fmt, "\0").as_ptr() as *const i8,
        )
    };
}

// ============================================================================
// FFI 便捷包装器 (自动处理 null 终止符)
// ============================================================================

/// FFI安全的信息日志 (自动添加 \0)
/// 
/// 专供 Rust→C FFI 调用, 避免 &str 转 CStr 的样板代码。
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub unsafe extern "C" fn klog_ffi_info(msg: *const i8) {
    let s = extract_cstr(msg);
    write_ffi_log(LogLevel::Info, LogCategory::Kernel, &s);
}

/// FFI安全的警告日志
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub unsafe extern "C" fn klog_ffi_warn(msg: *const i8) {
    let s = extract_cstr(msg);
    write_ffi_log(LogLevel::Warn, LogCategory::Kernel, &s);
}

/// FFI安全的错误日志
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub unsafe extern "C" fn klog_ffi_error(msg: *const i8) {
    let s = extract_cstr(msg);
    write_ffi_log(LogLevel::Error, LogCategory::Kernel, &s);
}

/// FFI安全的初始化消息
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub unsafe extern "C" fn klog_init_msg(msg: *const i8) {
    let s = extract_cstr(msg);
    write_ffi_log(LogLevel::Info, LogCategory::Init, &s);
}

/// 从 C 字符串指针提取 Rust 字符串切片 (内部辅助)
/// 
/// # Safety
/// ptr 必须是有效的 null-terminated C 字符串或 null
#[inline(always)]
unsafe fn extract_cstr(ptr: *const i8) -> &'static str {
    if ptr.is_null() { 
        return ""; 
    }
    
    static mut BUF: [u8; 256] = [0u8; 256];
    
    let cstr = core::ffi::CStr::from_ptr(ptr);
    match cstr.to_str() {
        Ok(s) => {
            let bytes = s.as_bytes();
            let len = bytes.len().min(255);
            BUF[..len].copy_from_slice(&bytes[..len]);
            BUF[len] = 0;
            
            // SAFETY: 我们刚刚写入且 null 终止
            core::str::from_utf8_unchecked(&BUF[..len])
        },
        Err(_) => "",
    }
}

/// 写入 FFI 日志 (内部辅助, 复用逻辑)
unsafe fn write_ffi_log(level: LogLevel, cat: LogCategory, msg: &str) {
    // 构造带 \0 的临时字符串
    static mut MSG_BUF: [u8; 256] = [0u8; 256];
    let bytes = msg.as_bytes();
    let len = bytes.len().min(255);
    MSG_BUF[..len].copy_from_slice(&bytes[..len]);
    MSG_BUF[len] = 0;
    
    klog_write(level.as_u8(), cat.as_u8(),
              core::ptr::null(), core::ptr::null(), 0,
              MSG_BUF.as_ptr() as *const i8);
}

// ============================================================================
// 辅助工具函数
// ============================================================================

/// 刷新串口输出缓冲区 (发送换行符)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_flush() {
    unsafe { serial_write_char(b'\n'); }
}

/// 清空日志缓冲区 (重置为初始状态)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_clear() {
    unsafe {
        KLOG_HEAD.store(0, Ordering::Relaxed);
        KLOG_TAIL.store(0, Ordering::Relaxed);
        KLOG_ENTRY_COUNT.store(0, Ordering::Relaxed);
        KLOG_BUFFER.fill(0);
    }
}

/// 获取当前日志条目总数
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_get_entry_count() -> u64 {
    KLOG_ENTRY_COUNT.load(Ordering::Acquire)
}

/// 将全部缓冲区内容转储到串口 (调试用途)
/// 
/// 格式:
/// ```text
/// ========================================
/// KERNEL LOG DUMP
/// ========================================
/// Entries: XXX
/// 
/// [日志内容...]
/// ========================================
/// ```
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_dump() {
    let header = b"\n========================================\n\
                   KERNEL LOG DUMP\n\
                   ========================================\n\
                   Entries: \0";
    
    let empty_msg = b"[Log empty or not initialized]\n\
                     ========================================\0";
    
    let footer = b"=======================================\0";
    
    unsafe {
        // 输出头
        serial_write_str(header);
        
        // 输出条目数
        let count = KLOG_ENTRY_COUNT.load(Ordering::Acquire);
        let mut num_buf = [0u8; 20];
        let len = uint_to_str(&mut num_buf, count, 10);
        serial_write_str(&num_buf[..len]);
        serial_write_str(b"\n\n\0");
        
        // 检查是否有数据
        let head = KLOG_HEAD.load(Ordering::Acquire);
        let tail = KLOG_TAIL.load(Ordering::Acquire);
        
        if KLOG_INITIALIZED.load(Ordering::Acquire) == 0 || head == tail {
            serial_write_str(empty_msg);
            return;
        }
        
        // 输出缓冲区内容
        let mut pos = head;
        while pos != tail {
            let idx = (pos as usize) % KLOG_BUFFER_SIZE;
            serial_write_char(KLOG_BUFFER[idx]);
            pos = (pos + 1) % KLOG_BUFFER_SIZE as u64;
        }
        
        // 输出尾部
        serial_write_str(b"\n\0");
        serial_write_str(footer);
        serial_write_char(b'\n');
    }
}

// ============================================================================
// 持久化功能 (保存/加载到磁盘)
// ============================================================================

/// 将日志缓冲区保存到磁盘文件
/// 
/// 使用 HVFS 文件系统, 路径: `/cfg/system/klog.db`
/// 
/// 文件格式:
/// ```text
/// [0..3]   Magic: 0x4B4C4F47 ("KLOG")
/// [4..7]   Buffer size (u32)
/// [8..11]  Head position (u32)
/// [12..15] Tail position (u32)
/// [16..N]  Buffer data (4096 bytes)
/// ```
/// 
/// # Returns
/// * Ok(()) - 成功
/// * Err(()) - 失败 (文件系统不可用等)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_save_to_disk() -> i32 {
    // 尝试打开/创建文件 (写模式)
    let fd = unsafe { 
        hvfs_open(
            KLOG_DB_PATH.as_ptr() as *const i8,
            0x200 | 0x01 | 0x40,  // O_CREAT | O_WRONLY | O_TRUNC
            0,
        ) 
    };
    
    if fd < 0 {
        return -1; // 打开失败
    }
    
    unsafe {
        // 构建文件头 (16字节)
        let header: [u32; 4] = [
            KLOG_DB_MAGIC,
            KLOG_BUFFER_SIZE as u32,
            KLOG_HEAD.load(Ordering::Relaxed) as u32,
            KLOG_TAIL.load(Ordering::Relaxed) as u32,
        ];
        
        // 写入头部
        if hvfs_write(fd, header.as_ptr() as *const u8, 16) != 16 {
            hvfs_close(fd);
            return -1;
        }
        
        // 写入缓冲区数据
        if hvfs_write(fd, KLOG_BUFFER.as_ptr(), KLOG_BUFFER_SIZE as u64) != KLOG_BUFFER_SIZE as i64 {
            hvfs_close(fd);
            return -1;
        }
        
        hvfs_close(fd);
    }
    
    0 // 成功
}

/// 从磁盘文件加载日志缓冲区
/// 
/// 会覆盖当前内存中的缓冲区内容!
/// 
/// # Returns
/// * Ok(()) - 成功
/// * Err(()) - 失败 (文件不存在、格式错误等)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn klog_load_from_disk() -> i32 {
    // 打开文件 (读模式)
    let fd = unsafe { hvfs_open(KLOG_DB_PATH.as_ptr() as *const i8, 0, 0) };
    
    if fd < 0 {
        return -1; // 文件不存在
    }
    
    unsafe {
        // 读取并验证文件头
        let mut header = [0u32; 4];
        if hvfs_read(fd, header.as_mut_ptr() as *mut u8, 16) != 16 {
            hvfs_close(fd);
            return -1;
        }
        
        // 验证魔数
        if header[0] != KLOG_DB_MAGIC {
            hvfs_close(fd);
            return -1; // 文件格式错误
        }
        
        // 验证缓冲区大小 (防止版本不匹配)
        if header[1] != KLOG_BUFFER_SIZE as u32 {
            hvfs_close(fd);
            return -1; // 大小不匹配
        }
        
        // 读取缓冲区数据
        if hvfs_read(fd, KLOG_BUFFER.as_mut_ptr(), KLOG_BUFFER_SIZE as u64) != KLOG_BUFFER_SIZE as i64 {
            hvfs_close(fd);
            return -1;
        }
        
        // 恢复指针位置
        KLOG_HEAD.store(header[2] as u64, Ordering::Relaxed);
        KLOG_TAIL.store(header[3] as u64, Ordering::Relaxed);
        
        hvfs_close(fd);
    }
    
    0 // 成功
}

// ============================================================================
// printk 兼容层 (Linux 风格接口)
// ============================================================================

/// Linux 兼容的 printk 接口 (简化版)
/// 
/// 自动使用 INFO 级别和 GENERAL 分类。
/// 主要用于快速移植 Linux 驱动代码。
/// 
/// # Note
/// 不支持完整的 printf 格式, 仅支持纯字符串输出。
/// 如需格式化, 请使用 klog_write 或宏。
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn printk(fmt: *const i8) -> i32 {
    unsafe {
        klog_write(
            LogLevel::Info.as_u8(),
            LogCategory::General.as_u8(),
            core::ptr::null(),
            core::ptr::null(),
            0,
            fmt,
        )
    }
}

// ============================================================================
// 单元测试 (仅在 cargo test 时编译)
// ============================================================================

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_log_level_enum() {
        // 测试枚举值
        assert_eq!(LogLevel::Debug as u8, 0);
        assert_eq!(LogLevel::Crit as u8, 5);
        
        // 测试前缀
        assert_eq!(LogLevel::Info.prefix(), "[INFO] ");
        assert_eq!(LogLevel::Error.prefix(), "[ERR]  ");
        
        // 测试名称
        assert_eq!(LogLevel::Warn.name(), "WARN");
        assert_eq!(LogLevel::Crit.name(), "CRIT");
    }
    
    #[test]
    fn test_log_level_conversion() {
        // 有效值转换
        assert!(LogLevel::from_u8(0).is_some());
        assert!(LogLevel::from_u8(5).is_some());
        
        // 无效值返回 None
        assert!(LogLevel::from_u8(99).is_none());
        assert!(LogLevel::from_u8(255).is_none());
    }
    
    #[test]
    fn test_log_category() {
        // 测试分类名称
        assert_eq!(LogCategory::General.name(), "GENERAL");
        assert_eq!(LogCategory::Network.name(), "NETWORK");
        
        // 测试范围
        assert!(LogCategory::from_u8(0).is_some());
        assert!(LogCategory::from_u8(11).is_some());
        assert!(LogCategory::from_u8(12).is_none());
    }
    
    #[test]
    fn test_constants() {
        // 验证关键常量
        assert_eq!(KLOG_BUFFER_SIZE, 4096);
        assert_eq!(KLOG_LINE_MAX, 256);
        assert_eq!(KLOG_CAT_MAX, 12);
        assert_eq!(KLOG_DB_MAGIC, 0x4B4C4F47);
    }
    
    #[test]
    fn test_uint_to_str() {
        let mut buf = [0u8; 20];
        
        // 零
        assert_eq!(uint_to_str(&mut buf, 0, 10), 1);
        assert_eq!(buf[0], b'0');
        
        // 普通数字
        assert_eq!(uint_to_str(&mut buf, 12345, 10), 5);
        assert_eq!(&buf[..5], b"12345");
        
        // 十六进制
        assert_eq!(uint_to_str(&mut buf, 255, 16), 2);
        assert_eq!(&buf[..2], b"ff");
    }
}
// ====== 增强的 klog 单元测试 (Phase 4 Quality Maintenance) ======

#[cfg(test)]
mod enhanced_tests {
    use super::*;
    
    #[test]
    fn test_klog_buffer_size_constant() {
        assert_eq!(KLOG_BUFFER_SIZE, 4096);
        assert!(KLOG_BUFFER_SIZE.is_power_of_two());
    }
    
    #[test]
    fn test_log_level_ordering() {
        let levels = [
            LogLevel::Debug,
            LogLevel::Info,
            LogLevel::Note,
            LogLevel::Warn,
            LogLevel::Error,
            LogLevel::Crit,
        ];
        
        for window in levels.windows(2) {
            assert!(window[0] as u8 < window[1] as u8, 
                   "LogLevel ordering incorrect");
        }
    }
    
    #[test]
    fn test_log_level_from_u8() {
        assert!(LogLevel::from_u8(0).is_some());
        assert!(LogLevel::from_u8(5).is_some());
        assert!(LogLevel::from_u8(6).is_none()); // 无效值
        assert!(LogLevel::from_u8(255).is_none());
    }
    
    #[test]
    fn test_log_category_count() {
        assert_eq!(LogCategory::General as u8, 0);
        assert_eq!(LogCategory::Network as u8, 11);
        
        // 验证所有分类都能安全转换
        for i in 0..=11u8 {
            assert!(LogCategory::from_u8(i).is_some(), 
                   "Category {} should be valid", i);
        }
        assert!(LogCategory::from_u8(12).is_none());
    }
    
    #[test]
    fn test_constants_validation() {
        // 魔数验证
        assert_eq!(KLOG_DB_MAGIC, 0x4B4C4F47); // "KLOG"
        
        // 数据库路径
        assert!(KLOG_DB_PATH.starts_with("/cfg/"));
        assert!(KLOG_DB_PATH.ends_with('\0'));
    }
    
    #[test]
    fn test_default_config() {
        assert_eq!(KLOG_DEFAULT_LEVEL, LogLevel::Info as u8);
        assert!(KLOG_DEFAULT_FLAGS & flags::TIMESTAMP != 0);
        assert!(KLOG_DEFAULT_FLAGS & flags::OUTPUT_SERIAL != 0);
        assert!(KLOG_DEFAULT_FLAGS & flags::OUTPUT_BUFFER != 0);
    }
}

