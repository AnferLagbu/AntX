//! IrqLine — 中断线安全句柄 (TCB)
//!
//! 设备驱动通过此句柄注册 ISR, 框架负责 IDT/APIC/GIC 编排。
//! 隐藏中断向量号、中断控制器等硬件细节。
//!
//! ## 与 Asterinas OSTD `IrqLine` 的关系
//!
//! 等价于 OSTD 的 `IrqLine`。
//!
//! ## SAFETY 不变量
//!
//! - 一个 IrqLine 最多注册一个 ISR (可通过重新注册覆盖)。
//! - ISR 在中断上下文中调用: 不可 sleep / 不可持 Mutex / 不可阻塞。
//! - 中断向量 ≤ 255 (x86_64) 或 ≤ 1023 (aarch64 GIC)。

/// 中断处理函数签名。
///
/// # 约束
/// - ISR 上下文调用 (栈深度受限, 不可睡眠)。
/// - 返回 true 表示本 handler 处理了此中断。
pub type InterruptHandler = fn() -> bool;

/// 中断线句柄。
///
/// 每个设备通过此句柄注册/注销 ISR。
pub struct IrqLine {
    vector: u8,
    irq: u32,
    registered: bool,
}

impl IrqLine {
    /// 创建中断线句柄。
    ///
    /// # SAFETY
    /// - irq 必须是有效的中断请求号。
    /// - vector 是 IDT 中断向量号。
    pub unsafe fn new(irq: u32, vector: u8) -> Self {
        Self { vector, irq, registered: false }
    }

    #[inline(always)] pub fn irq(&self) -> u32 { self.irq }
    #[inline(always)] pub fn vector(&self) -> u8 { self.vector }

    /// 注册 ISR 到全局中断表。
    ///
    /// # 安全约束
    /// - handler 必须在中断上下文安全 (无 sleep, 无 Mutex, 快速返回)。
    pub fn on_interrupt(&mut self, handler: InterruptHandler) -> Result<(), &'static str> {
        // SAFETY: 启动阶段单线程调用, 无竞争。
        unsafe {
            register_isr(self.vector, handler);
        }
        self.registered = true;
        Ok(())
    }

    /// 启用该中断线 (unmask)
    #[cfg(target_arch = "x86_64")]
    pub fn enable(&self) {
        crate::kernel::framework::arch::x86_64::ioapic::unmask_irq(self.irq as u8);
    }

    #[cfg(target_arch = "aarch64")]
    pub fn enable(&self) {
        let _ = self;
    }

    /// 禁用该中断线 (mask)
    #[cfg(target_arch = "x86_64")]
    pub fn disable(&self) {
        crate::kernel::framework::arch::x86_64::ioapic::mask_irq(self.irq as u8);
    }

    #[cfg(target_arch = "aarch64")]
    pub fn disable(&self) {
        let _ = self;
    }

    #[inline(always)] pub fn is_registered(&self) -> bool { self.registered }
}

// ============================================================================
// 内部: ISR 注册表
// ============================================================================

const MAX_ISR_VECTORS: usize = 256;

/// 全局 ISR 函数指针表, 由 idt handlers 分发调用。
/// 初始化时单线程写入, 运行时只读 → 无锁安全。
static mut ISR_TABLE: [Option<InterruptHandler>; MAX_ISR_VECTORS] = [None; MAX_ISR_VECTORS];

/// 注册中断向量对应的 ISR 处理器。
///
/// # SAFETY
///
/// 1. 仅在启动单线程阶段 (无并发中断) 调用, 写 ISR_TABLE 安全
/// 2. `vector` 必须小于 `MAX_ISR_VECTORS` (内部会检查)
/// 3. `handler` 必须是 `'static` 生命周期的合法函数指针, 可被中断上下文调用
///    (不持有任何 Rust 锁, 不分配, 不睡眠)
unsafe fn register_isr(vector: u8, handler: InterruptHandler) {
    // SAFETY: 单线程初始化上下文, 无竞争。
    let idx = vector as usize;
    if idx < MAX_ISR_VECTORS {
        // SAFETY: 启动阶段单线程, ISR_TABLE 全局表只被本函数写, 后续只读。
        // idx 已通过 `idx < MAX_ISR_VECTORS` 边界检查, 不会越界写入。
        unsafe { ISR_TABLE[idx] = Some(handler); }
    }
}

/// 分发中断到已注册的 handler (由 IDT ISR stub 调用)。
pub fn dispatch_irq(vector: u8) -> bool {
    let idx = vector as usize;
    if idx < MAX_ISR_VECTORS {
        // SAFETY: ISR_TABLE 运行时只读, 中断上下文无竞争。
        if let Some(handler) = unsafe { ISR_TABLE[idx] } {
            return handler();
        }
    }
    false
}

// SAFETY: IrqLine 句柄在设备驱动中独占, 启动阶段单线程注册, 运行时只读。
unsafe impl Send for IrqLine {}
// SAFETY: ISR_TABLE 初始化后运行时只读, 句柄字段 (vector/irq/registered) 在驱动上下文独占访问。
unsafe impl Sync for IrqLine {}
