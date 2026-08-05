pub mod gfx_console;

use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use gfx_console::GfxConsole;

static GFX_READY: AtomicBool = AtomicBool::new(false);
static GFX_CONSOLE_PTR: AtomicPtr<GfxConsole> = AtomicPtr::new(core::ptr::null_mut());

#[expect(
    clippy::ref_as_ptr,
    reason = "ref_as_ptr: &T as *const T 是已知安全 (Rust 2024 可用 &raw const; 当前优先 expect"
)]
/// 初始化图形控制台 —— 绑定到已分配在静态存储中的 `GfxConsole`
///
/// # Safety
///
/// - `console` 必须来自静态存储（`Box::leak` 出品）
/// - 仅调用一次
pub fn gfx_console_init(console: &'static mut GfxConsole) {
    GFX_CONSOLE_PTR.store(console as *mut GfxConsole, Ordering::Release);
    GFX_READY.store(true, Ordering::Release);
}

/// 将内核日志同步到图形控制台（在 `klog_output` 中调用）
pub fn gfx_console_write(msg: &[u8]) {
    if !GFX_READY.load(Ordering::Acquire) {
        return;
    }
    let ptr = GFX_CONSOLE_PTR.load(Ordering::Acquire);
    if !ptr.is_null() {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let console = &mut *ptr;
            if let Ok(s) = core::str::from_utf8(msg) {
                console.write_str(s);
            }
        }
    }
}

/// Panic 发生时接管图形控制台 — 绘制崩溃横幅并输出消息
///
/// 此函数在 panic handler 中调用，无论 `GFX_READY` 是否为 true 都尝试接管。
/// 优先使用已存在的 `GfxConsole` 实例，如果没有则什么也不做（至少串口已输出）。
///
/// # Safety
///
/// 仅在 panic 上下文中调用。不依赖锁或中断，直接操作帧缓冲。
pub fn gfx_console_panic_reclaim(msg: &str) {
    let ptr = GFX_CONSOLE_PTR.load(Ordering::Acquire);
    if !ptr.is_null() {
        unsafe {
            let console = &mut *ptr;
            console.panic_reclaim(msg);
        }
    }
}

/// Panic 模式下向图形控制台追加崩溃详情
///
/// 在 `gfx_console_panic_reclaim` 之后再调用此函数输出寄存器转储等信息。
pub fn gfx_console_panic_write(msg: &str) {
    let ptr = GFX_CONSOLE_PTR.load(Ordering::Acquire);
    if !ptr.is_null() {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let console = &mut *ptr;
            console.panic_write(msg);
        }
    }
}
