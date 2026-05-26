pub mod gfx_console;

use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use gfx_console::GfxConsole;

static GFX_READY: AtomicBool = AtomicBool::new(false);
static GFX_CONSOLE_PTR: AtomicPtr<GfxConsole> = AtomicPtr::new(core::ptr::null_mut());

/// 初始化图形控制台 —— 绑定到已分配在静态存储中的 GfxConsole
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
        unsafe {
            let console = &mut *ptr;
            if let Ok(s) = core::str::from_utf8(msg) {
                console.write_str(s);
            }
        }
    }
}