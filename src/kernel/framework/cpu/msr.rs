//! MSR (Model Specific Register) 操作
//!
//! 提供 safe wrapper for RDMSR/WRMSR instructions.

/// 读取 64 位 MSR 寄存器
///
/// # Arguments
/// * `msr` - MSR 地址 (如 0xC0000080 for IA32_EFER)
///
/// # Returns
/// MSR 的 64 位值
///
/// # Safety
/// 必须在 Ring 0 (内核态) 调用, 否则触发 #GP 异常。
#[inline(always)]
pub unsafe fn read_msr(msr: u32) -> u64 {
    let (low, high): (u32, u32);

    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
        options(nomem, nostack, preserves_flags),
    );

    ((high as u64) << 32) | (low as u64)
}

/// 写入 64 位 MSR 寄存器
///
/// # Arguments
/// * `msr` - MSR 地址
/// * `value` - 要写入的 64 位值
///
/// # Safety
/// 必须在 Ring 0 调用, 且 MSR 必须存在且可写。
#[inline(always)]
pub unsafe fn write_msr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;

    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
        options(nomem, nostack, preserves_flags),
    );
}

/// FFI 兼容: 读取 MSR (返回两个 32 位值)
#[no_mangle]
///
/// # Safety
///
/// `msr` 是合法的 MSR 索引. 非法索引将触发 #GP 异常.
/// `low` 和 `high` 是合法可写指针.
pub unsafe extern "C" fn cpu_read_msr(msr: u32, low: *mut u32, high: *mut u32) -> i32 {
    if low.is_null() || high.is_null() {
        return -1;
    }

    let value = read_msr(msr);
    *low = value as u32;
    *high = (value >> 32) as u32;

    0
}

/// FFI 兼容: 写入 MSR
#[no_mangle]
///
/// # Safety
///
/// `msr` 是合法的 MSR 索引. 非法索引将触发 #GP 异常.
pub unsafe extern "C" fn cpu_write_msr(msr: u32, low: u32, high: u32) -> i32 {
    write_msr(msr, ((high as u64) << 32) | (low as u64));
    0
}

/// FFI 兼容: 读取 64 位 MSR
#[no_mangle]
///
/// # Safety
///
/// `msr` 是合法的 MSR 索引. 非法索引将触发 #GP 异常.
pub unsafe extern "C" fn cpu_read_msr64(msr: u32) -> u64 {
    read_msr(msr)
}

/// FFI 兼容: 写入 64 位 MSR
#[no_mangle]
///
/// # Safety
///
/// `msr` 是合法的 MSR 索引. 非法索引将触发 #GP 异常.
pub unsafe extern "C" fn cpu_write_msr64(msr: u32, value: u64) -> i32 {
    write_msr(msr, value);
    0
}
