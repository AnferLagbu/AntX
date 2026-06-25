//! HDMI 像素时钟 (PLL) 配置
//!
//! HDMI 像素时钟由 PLL 产生, 通过 mul/div 寄存器配置:
//!   `pixel_clock = base_khz × mul / div`
//! 默认 base = 27 MHz (HDMI 规范标准参考时钟).
//!
//! ## 厂商差异
//!
//! - **Intel IGP (IBX/HSW/SKL)**: DPLL (Display PLL) 通过 PCH transcoder, 内部 MMIO 寄存器
//! - **AMD DCN**: DENTIST 时钟发生器 + DISPCLK
//! - **Synopsys DesignWare HDMI**: phy_clock + tmds_clock 寄存器
//! - **QEMU Bochs DISPI**: 不使用像素时钟寄存器 (走 index/data port I/O)
//!
//! ## P1-2 PLL 锁定等待
//!
//! 写入 mul/div 后, 真实硬件需等待 PLL 稳定 (100-500 µs).
//! 锁定状态由 `HDMI_PCLK_LOCK_REG_OFFSET` 寄存器 bit 0 表示.
//! 超时返回 `Err(DriverError::Timeout)`, 调用方应放弃整个 `set_video_mode`.
//!
//! ## 单元测试可见性
//!
//! `compute_pixel_clock_mul_div` 和 `poll_hdmi_pll_locked` 为 `pub(super)`,
//! 仅 hdmi/ 子模块内可见; 外部调用方通过 `HdmiController` 间接使用.

use super::DriverError;
use crate::kernel::framework::iomem::IoMem;

// ============================================================================
// 像素时钟寄存器偏移
// ============================================================================

/// HDMI 像素时钟参考时钟 (kHz).
///
/// 默认 27_000 kHz = 27 MHz, HDMI 规范标准参考时钟.
/// 部分硬件 (e.g. AMD) 使用 100 MHz 参考, 此时通过
/// [`crate::kernel::framework::driver::display::hdmi::HdmiController::new_with_iomem_pixel_clock`]
/// 指定自家 mul/div 偏移, 但 base_khz 仍用本常量 (vendor 自定义算法接管).
pub(super) const HDMI_PCLK_BASE_KHZ: u32 = 27_000;

/// HDMI 像素时钟乘法寄存器默认偏移 (8-bit)。
///
/// 实际写入值 = `mul` (1..=255, 0 视作 1)。
pub(super) const HDMI_PCLK_MUL_REG_OFFSET: usize = 0x060;

/// HDMI 像素时钟除法寄存器默认偏移 (8-bit)。
///
/// 实际写入值 = `div` (1..=255, 0 视作 1)。
pub(super) const HDMI_PCLK_DIV_REG_OFFSET: usize = 0x064;

/// HDMI 像素时钟 PLL 锁定状态寄存器默认偏移 (P1-2, 8-bit).
///
/// bit 0: PLL 锁定 (1 = 锁定, 0 = 未锁定).
/// 写入 mul/div 后需轮询此寄存器, 锁定后才使能 TMDS 输出.
pub(super) const HDMI_PCLK_LOCK_REG_OFFSET: usize = 0x066;
/// PLL 锁定状态 bit (bit 0).
pub(super) const HDMI_PCLK_LOCK_BIT: u8 = 0x01;

/// HDMI PLL 锁定轮询超时 (P1-2).
///
/// 典型 PLL 锁定时间: 100-500 µs; 给 10 ms 裕量 (50 iters/µs × 500_000 = 10 ms).
pub(super) const HDMI_PLL_LOCK_TIMEOUT_ITERS: usize = 500_000;

// ============================================================================
// 像素时钟算法
// ============================================================================

/// 从目标像素时钟 (kHz) 计算 mul/div 寄存器值。
///
/// 给定参考时钟 `base_khz`, 寻找满足 `base * mul / div ≈ target_khz` 的
/// 8-bit mul/div 对 (mul, div ∈ 1..=255).
///
/// 算法: 贪心搜索 `div ∈ 1..=16` (HDMI 控制器 PLL 典型范围), 选取
/// |base * mul / div - target| 最小的 (mul, div) 对。
///
/// 返回 `(mul, div)`; 写入寄存器时 mul/div == 0 视作 1.
///
/// # 示例
///
/// - target=148500 kHz (1080p60), base=27000 kHz:
///   - div=1: mul=6 → 162000 kHz (误差 13500)
///   - div=2: mul=11 → 148500 kHz (误差 0) ✓
///   返回 (11, 2)。
///
/// 这是 HDMI 控制器最常用的 PLL 配置; 真实硬件如需更精确算法 (e.g. N/M/frac)
/// 应通过 vendor 自定义路径接管, 不走本函数。
pub(super) fn compute_pixel_clock_mul_div(target_khz: u32, base_khz: u32) -> (u8, u8) {
    if target_khz == 0 || base_khz == 0 {
        return (1, 1);
    }
    let mut best = (1u8, 1u8);
    let mut best_err: u32 = u32::MAX;
    for div in 1u32..=16 {
        // mul = round(target * div / base), 避免溢出
        let mul = target_khz
            .saturating_mul(div)
            .saturating_add(base_khz / 2)
            / base_khz;
        // mul 必须落在 1..=255
        if mul == 0 || mul > 255 {
            continue;
        }
        let actual = base_khz.saturating_mul(mul) / div;
        let err = if actual > target_khz {
            actual - target_khz
        } else {
            target_khz - actual
        };
        if err < best_err {
            best_err = err;
            best = (mul as u8, div as u8);
            if err == 0 {
                break; // 完美匹配
            }
        }
    }
    best
}

/// 配置 HDMI 像素时钟 PLL (mul/div 寄存器)。
///
/// 计算 `mul = round(target / base)` 并写入对应寄存器.
///
/// # Safety
/// 调用方必须保证:
/// - `iomem` 已映射到有效 HDMI 控制器 MMIO 区域
/// - `pclk_mul_reg_offset + 1 <= iomem.len()` 且 `pclk_div_reg_offset + 1 <= iomem.len()`
pub(super) unsafe fn configure_hdmi_pixel_clock(
    iomem: &IoMem,
    pclk_mul_reg_offset: usize,
    pclk_div_reg_offset: usize,
    target_khz: u32,
) {
    let (mul, div) = compute_pixel_clock_mul_div(target_khz, HDMI_PCLK_BASE_KHZ);
    iomem.write_u8(pclk_mul_reg_offset, mul);
    iomem.write_u8(pclk_div_reg_offset, div);
}

/// 轮询 HDMI 像素时钟 PLL 锁定状态 (P1-2).
///
/// 写入 mul/div 后调用, 阻塞等待 [`HDMI_PLL_LOCK_TIMEOUT_ITERS`] iters
/// 或直到 PLL 锁定 bit = 1.
///
/// 返回:
/// - `Ok(())` = PLL 锁定
/// - `Err(DriverError::Timeout)` = 锁定超时 (PLL 未稳定)
///
/// QEMU/Bochs 等无真实硬件环境下, 锁定寄存器可能读出 0 (硬件无意义),
/// 此函数会超时. 调用方应根据环境决定是否视为成功 (IoMem None 路径不调用).
///
/// # Safety
/// 调用方必须保证 `pll_lock_reg_offset + 1 <= iomem.len()`.
pub(super) unsafe fn poll_hdmi_pll_locked(
    iomem: &IoMem,
    pll_lock_reg_offset: usize,
) -> core::result::Result<(), DriverError> {
    let mut elapsed: usize = 0;
    while elapsed < HDMI_PLL_LOCK_TIMEOUT_ITERS {
        let status = iomem.read_u8(pll_lock_reg_offset);
        if status & HDMI_PCLK_LOCK_BIT != 0 {
            return Ok(());
        }
        // 短暂自旋等待 (10-20 µs, 适配典型 PLL 锁定时间)
        for _ in 0..50 {
            core::hint::spin_loop();
        }
        elapsed += 50;
    }
    Err(DriverError::Timeout)
}
