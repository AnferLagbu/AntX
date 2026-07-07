//! DDC (Display Data Channel) I2C bitbang 协议实现
//!
//! HDMI 通过 DDC (基于 I2C 协议) 从显示器读取 EDID. 显示器作为 I2C 从机,
//! 主机 (HDMI 控制器) 通过 bitbang SDA/SCL 引脚模拟 I2C 通信.
//!
//! ## 协议层
//!
//! - **100 kHz 标准模式**: 半周期 5 µs, 适配 50 spin_loop ≈ 1-2 µs (P1-3 收紧到 1-2 ms 总超时)
//! - **MSB first**: 字节传输高位先行
//! - **ACK 采样**: 主机发送 8 bit 后, 释放 SDA 读 1 bit 作为从机应答
//!
//! ## 厂商差异
//!
//! - **Intel IGP GMBus**: 16-bit 端口 I/O, 走专有 GMBus 控制器 (非 bitbang, 不适用本实现)
//! - **AMD DCN**: DDC bitbang 寄存器通常在 DDI 控制器 MMIO 区
//! - **通用 SoC HDMI (Synopsys/DesignWare/IT66121)**: 8-bit bitbang 寄存器 (本实现适用)
//! - **QEMU Bochs DISPI**: 无 DDC, fallback 到 mock EDID
//!
//! ## 时序约束
//!
//! - 总事务超时: 50_000 spin_loops ≈ 1-2 ms ([`DDC_TRANSACTION_TIMEOUT_ITERS`])
//! - 1 字节传输: 8 bit + ACK = 10 ddc_delay 调用 (500 iters)
//! - 完整 EDID 读: ~150 字节 = 75_000 iters, 接近总超时上限

use super::DriverError;
use crate::kernel::framework::iomem::IoMem;

// ============================================================================
// DDC 寄存器偏移
// ============================================================================

/// DDC 控制寄存器默认偏移 (8-bit)。
///
/// bit 0: SDA 输出 (1=高, 0=低)
/// bit 1: SCL 输出 (1=高, 0=低)
pub(super) const DDC_DEFAULT_CTRL_REG: usize = 0x050;

/// DDC 状态寄存器默认偏移 (8-bit)。
///
/// bit 0: SDA 输入 (从机驱动)
pub(super) const DDC_DEFAULT_STATUS_REG: usize = 0x054;

/// DDC SDA 输出 bit (bit 0).
pub(super) const DDC_SDA_OUT_BIT: u8 = 0x01;
/// DDC SCL 输出 bit (bit 1).
pub(super) const DDC_SCL_OUT_BIT: u8 = 0x02;
/// DDC SDA 输入 bit (bit 0, status 寄存器).
pub(super) const DDC_SDA_IN_BIT: u8 = 0x01;

/// EDID I2C 从机地址 (写模式, 0xA0 = 0x50 << 1)
pub(super) const DDC_EDID_ADDR_WRITE: u8 = 0xA0;
/// EDID I2C 从机地址 (读模式, 0xA1 = 0x50 << 1 | 1)
pub(super) const DDC_EDID_ADDR_READ: u8 = 0xA1;

/// DDC I2C 时序延时针 (spin loop 次数, 适配 ~100 kHz 标准模式)。
///
/// 内核上下文不允许睡眠, 通过 `core::hint::spin_loop` 实现短延时.
/// 50 次 spin_loop 在现代 CPU 上约 1-2 微秒 (接近 I2C 标准模式半周期).
pub(super) const DDC_I2C_DELAY_ITERS: usize = 50;

/// DDC I2C 事务总超时 (P1-3)。
///
/// 50_000 次 spin_loop ≈ 1-2 ms (50 iters/µs).
///
/// 计算依据 (100 kHz I2C 标准模式, 1 bit = 10 µs):
/// - 1 字节 = 8 bits + ACK = ~100 µs = ~5_000 iters
/// - 完整 EDID 事务 ≈ 150 字节 (写 addr + offset + 读 128 字节) = 750_000 iters
/// - 设 50_000 iters 允许 6 次字节传输 (事务头部), 单个字节超时就中断
///
/// 实际 EDID 读可能需要更长时间 (完整 12 ms 事务), 此处故意收紧到 1-2 ms,
/// 以便检测到总线挂起/从机无响应时快速失败. QEMU Bochs 模拟下应远低于此阈值.
pub(super) const DDC_TRANSACTION_TIMEOUT_ITERS: usize = 50_000;

// ============================================================================
// DDC 协议原子
// ============================================================================

/// 带计数器的延时函数 (P1-3)。
///
/// 与原 `ddc_delay()` 行为一致, 但累加 `elapsed_iters` 计数, 用于事务级超时检查.
///
/// 调用方应在调用前预算剩余 iters; 超出 [`DDC_TRANSACTION_TIMEOUT_ITERS`]
/// 时返回 `Err(DriverError::Timeout)`.
#[inline]
fn ddc_delay_with_counter(elapsed_iters: &mut usize) {
    *elapsed_iters += DDC_I2C_DELAY_ITERS;
    for _ in 0..DDC_I2C_DELAY_ITERS {
        core::hint::spin_loop();
    }
}

/// 同时设置 SDA 与 SCL 输出电平。
///
/// # Safety
/// 调用方必须保证 `ctrl_reg_offset + 1 <= iomem.len()`。
#[inline]
unsafe fn ddc_set_sda_scl(iomem: &IoMem, ctrl_reg_offset: usize, sda_high: bool, scl_high: bool) {
    let mut val = 0u8;
    if sda_high {
        val |= DDC_SDA_OUT_BIT;
    }
    if scl_high {
        val |= DDC_SCL_OUT_BIT;
    }
    iomem.write_u8(ctrl_reg_offset, val);
}

/// I2C START 条件: SDA 在 SCL 高电平时由高变低 (P1-3 带 timeout)。
///
/// `elapsed_iters` 由调用方持有, 每次 `ddc_delay_with_counter` 内部累加;
/// 超过 [`DDC_TRANSACTION_TIMEOUT_ITERS`] 时返回 `Err(DriverError::Timeout)`.
///
/// # Safety
/// 调用方必须保证 `ctrl_reg_offset + 1 <= iomem.len()`。
#[inline]
unsafe fn ddc_i2c_start(
    iomem: &IoMem,
    ctrl_reg_offset: usize,
    elapsed_iters: &mut usize,
) -> core::result::Result<(), DriverError> { unsafe {
    if *elapsed_iters + DDC_I2C_DELAY_ITERS * 2 > DDC_TRANSACTION_TIMEOUT_ITERS {
        return Err(DriverError::Timeout);
    }
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, true);
    ddc_delay_with_counter(elapsed_iters);
    ddc_set_sda_scl(iomem, ctrl_reg_offset, false, true);
    ddc_delay_with_counter(elapsed_iters);
    Ok(())
}}

/// I2C STOP 条件: SDA 在 SCL 高电平时由低变高 (P1-3 带 timeout)。
///
/// # Safety
/// 调用方必须保证 `ctrl_reg_offset + 1 <= iomem.len()`。
#[inline]
unsafe fn ddc_i2c_stop(
    iomem: &IoMem,
    ctrl_reg_offset: usize,
    elapsed_iters: &mut usize,
) -> core::result::Result<(), DriverError> { unsafe {
    if *elapsed_iters + DDC_I2C_DELAY_ITERS * 2 > DDC_TRANSACTION_TIMEOUT_ITERS {
        return Err(DriverError::Timeout);
    }
    ddc_set_sda_scl(iomem, ctrl_reg_offset, false, true);
    ddc_delay_with_counter(elapsed_iters);
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, true);
    ddc_delay_with_counter(elapsed_iters);
    Ok(())
}}

/// I2C 写 1 字节 (MSB first) 并采样 ACK (P1-3 带 timeout)。
///
/// 返回:
/// - `Ok(true)` = ACK (从机应答)
/// - `Ok(false)` = NACK (从机不应答)
/// - `Err(DriverError::Timeout)` = 事务超时
///
/// # Safety
/// 调用方必须保证 `ctrl_reg_offset + 1 <= iomem.len()` 与
/// `status_reg_offset + 1 <= iomem.len()`。
unsafe fn ddc_i2c_write_byte(
    iomem: &IoMem,
    ctrl_reg_offset: usize,
    status_reg_offset: usize,
    byte: u8,
    elapsed_iters: &mut usize,
) -> core::result::Result<bool, DriverError> { unsafe {
    // 预算检查: 8 bit 传输 + ACK = 10 ddc_delay() 调用
    if *elapsed_iters + DDC_I2C_DELAY_ITERS * 10 > DDC_TRANSACTION_TIMEOUT_ITERS {
        return Err(DriverError::Timeout);
    }
    for i in 0..8u8 {
        let bit = (byte >> (7 - i)) & 1 != 0;
        ddc_set_sda_scl(iomem, ctrl_reg_offset, bit, false);
        ddc_delay_with_counter(elapsed_iters);
        ddc_set_sda_scl(iomem, ctrl_reg_offset, bit, true);
        ddc_delay_with_counter(elapsed_iters);
    }
    // 释放 SDA 让从机 ACK
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, false);
    ddc_delay_with_counter(elapsed_iters);
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, true);
    ddc_delay_with_counter(elapsed_iters);
    // 采样 SDA: 0 = ACK, 1 = NACK
    let sda = iomem.read_u8(status_reg_offset) & DDC_SDA_IN_BIT;
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, false);
    Ok(sda == 0)
}}

/// I2C 读 1 字节 (MSB first), 由主机发送 ACK/NACK (P1-3 带 timeout)。
///
/// `send_ack = true` 表示读完后主机 ACK (从机继续发送, 用于读 0..126 字节),
/// `send_ack = false` 表示 NACK (从机停止发送, 用于读最后 1 字节)。
///
/// # Safety
/// 调用方必须保证 `ctrl_reg_offset + 1 <= iomem.len()` 与
/// `status_reg_offset + 1 <= iomem.len()`。
unsafe fn ddc_i2c_read_byte(
    iomem: &IoMem,
    ctrl_reg_offset: usize,
    status_reg_offset: usize,
    send_ack: bool,
    elapsed_iters: &mut usize,
) -> core::result::Result<u8, DriverError> { unsafe {
    // 预算检查: 8 bit 读 + ACK = 10 ddc_delay() 调用
    if *elapsed_iters + DDC_I2C_DELAY_ITERS * 10 > DDC_TRANSACTION_TIMEOUT_ITERS {
        return Err(DriverError::Timeout);
    }
    let mut byte = 0u8;
    // 释放 SDA 让从机驱动
    ddc_set_sda_scl(iomem, ctrl_reg_offset, true, false);
    ddc_delay_with_counter(elapsed_iters);

    for i in 0..8u8 {
        ddc_set_sda_scl(iomem, ctrl_reg_offset, true, true);
        ddc_delay_with_counter(elapsed_iters);
        let bit = iomem.read_u8(status_reg_offset) & DDC_SDA_IN_BIT;
        byte |= bit << (7 - i);
        ddc_set_sda_scl(iomem, ctrl_reg_offset, true, false);
        ddc_delay_with_counter(elapsed_iters);
    }

    // 主机发送 ACK/NACK
    ddc_set_sda_scl(iomem, ctrl_reg_offset, !send_ack, false);
    ddc_delay_with_counter(elapsed_iters);
    ddc_set_sda_scl(iomem, ctrl_reg_offset, !send_ack, true);
    ddc_delay_with_counter(elapsed_iters);
    ddc_set_sda_scl(iomem, ctrl_reg_offset, !send_ack, false);

    Ok(byte)
}}

// ============================================================================
// EDID 块读取事务
// ============================================================================

/// 通过 DDC 总线读取 EDID 块 (128 字节)。
///
/// I2C 事务序列:
/// ```text
/// START -> [0xA0] -> [offset] -> REPEATED_START -> [0xA1] -> [128 字节] -> STOP
/// ```
///
/// 返回 `Ok([u8; 128])` 表示读取成功 (含 ACK), 失败返回 `Err`。
///
/// # Safety
/// 调用方必须保证:
/// - `iomem` 已映射到有效 HDMI 控制器 MMIO 区域
/// - `ctrl_reg_offset + 1 <= iomem.len()` 且 `status_reg_offset + 1 <= iomem.len()`
pub(super) unsafe fn read_edid_block_via_ddc(
    iomem: &IoMem,
    ctrl_reg_offset: usize,
    status_reg_offset: usize,
    block: u8,
) -> core::result::Result<[u8; 128], DriverError> { unsafe {
    let mut data = [0u8; 128];
    // P1-3: 事务级超时计数器, 跨 I2C 调用累加
    let mut elapsed_iters: usize = 0;

    ddc_i2c_start(iomem, ctrl_reg_offset, &mut elapsed_iters)?;
    match ddc_i2c_write_byte(
        iomem, ctrl_reg_offset, status_reg_offset,
        DDC_EDID_ADDR_WRITE, &mut elapsed_iters,
    ) {
        Ok(true) => {} // ACK
        Ok(false) => {
            let _ = ddc_i2c_stop(iomem, ctrl_reg_offset, &mut elapsed_iters);
            return Err(DriverError::HardwareError);
        }
        Err(e) => {
            let _ = ddc_i2c_stop(iomem, ctrl_reg_offset, &mut elapsed_iters);
            return Err(e);
        }
    }
    // EDID 块偏移 = block * 128 (block 0 起始于 0, block 1 起始于 128)
    let offset = block.wrapping_mul(128);
    match ddc_i2c_write_byte(
        iomem, ctrl_reg_offset, status_reg_offset, offset, &mut elapsed_iters,
    ) {
        Ok(true) => {}
        Ok(false) => {
            let _ = ddc_i2c_stop(iomem, ctrl_reg_offset, &mut elapsed_iters);
            return Err(DriverError::HardwareError);
        }
        Err(e) => {
            let _ = ddc_i2c_stop(iomem, ctrl_reg_offset, &mut elapsed_iters);
            return Err(e);
        }
    }

    // REPEATED START 切换到读模式
    ddc_i2c_start(iomem, ctrl_reg_offset, &mut elapsed_iters)?;
    match ddc_i2c_write_byte(
        iomem, ctrl_reg_offset, status_reg_offset,
        DDC_EDID_ADDR_READ, &mut elapsed_iters,
    ) {
        Ok(true) => {}
        Ok(false) => {
            let _ = ddc_i2c_stop(iomem, ctrl_reg_offset, &mut elapsed_iters);
            return Err(DriverError::HardwareError);
        }
        Err(e) => {
            let _ = ddc_i2c_stop(iomem, ctrl_reg_offset, &mut elapsed_iters);
            return Err(e);
        }
    }

    // 读 128 字节: 前 127 字节 ACK, 最后 1 字节 NACK
    for i in 0..127 {
        match ddc_i2c_read_byte(
            iomem, ctrl_reg_offset, status_reg_offset, true, &mut elapsed_iters,
        ) {
            Ok(b) => data[i] = b,
            Err(e) => {
                let _ = ddc_i2c_stop(iomem, ctrl_reg_offset, &mut elapsed_iters);
                return Err(e);
            }
        }
    }
    match ddc_i2c_read_byte(
        iomem, ctrl_reg_offset, status_reg_offset, false, &mut elapsed_iters,
    ) {
        Ok(b) => data[127] = b,
        Err(e) => {
            let _ = ddc_i2c_stop(iomem, ctrl_reg_offset, &mut elapsed_iters);
            return Err(e);
        }
    }

    ddc_i2c_stop(iomem, ctrl_reg_offset, &mut elapsed_iters)?;
    Ok(data)
}}
