//! sys_ioctl 行为契约测试 (P1-I-39)
//!
//! 验证:
//! 1. TCGETS stub 必须返回 -ENOSYS 而非假装成功
//! 2. 未知 ioctl 命令返回 -ENOTTY
//! 3. arg=0 返回 -EINVAL
//! 4. TIOCGWINSZ 返回 0 (真实实现, 填 ws 结构)
//!
//! 本测试通过自包含的 mini-syscall 分发器镜像 `sys_ioctl` 行为, 避免 host
//! 端链接内核 crate. 内核 `src/kernel/framework/syscall/mod.rs::sys_ioctl` 是
//! 该契约的权威实现.

const TIOCGWINSZ: u64 = 0x5413;
const TCGETS: u64 = 0x5401;
const TIOCSETAF: u64 = 0x5404;
const FIONREAD: u64 = 0x541B;

// 镜像 Errno::as_ret 的编码
const ENOSYS: i64 = -38;
const ENOTTY: i64 = -25;
const EINVAL: i64 = -22;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

fn write_struct<T: Copy>(dst: *mut T, src: &T) {
    unsafe { core::ptr::write_volatile(dst, *src) }
}

/// 镜像内核 sys_ioctl 的最小契约
fn sys_ioctl_contract(_fd: i32, request: u64, arg: u64) -> i64 {
    if arg == 0 {
        return EINVAL;
    }
    match request {
        TIOCGWINSZ => {
            let ws = Winsize {
                ws_row: 25,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let dst = arg as *mut Winsize;
            write_struct(dst, &ws);
            0
        }
        TCGETS => ENOSYS,
        _ => ENOTTY,
    }
}

#[test]
fn tcgets_stub_returns_enosys_not_zero() {
    // P1-I-39 验收: TCGETS 必须返回 ENOSYS, 不能假装成功
    let mut ws = Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    let ret = sys_ioctl_contract(1, TCGETS, &mut ws as *mut _ as u64);
    assert_eq!(ret, ENOSYS, "P1-I-39: TCGETS stub 必须返回 -ENOSYS, 实际 = {}", ret);
    // termios 缓冲区不被填充, 也不被破坏 (内核端 stub 路径无副作用)
    assert_eq!(ws.ws_row, 0, "P1-I-39: ENOSYS 路径不应修改 termios 缓冲区");
}

#[test]
fn tcgets_returns_enosys_for_any_fd() {
    // P1-I-39 验收: 任意 fd 调用 TCGETS 返回 ENOSYS
    for fd in [0i32, 1, 2, 100, -1] {
        let mut ws = Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
        let ret = sys_ioctl_contract(fd, TCGETS, &mut ws as *mut _ as u64);
        assert_eq!(ret, ENOSYS, "P1-I-39: fd={} 调用 TCGETS 必须返回 -ENOSYS", fd);
    }
}

#[test]
fn unknown_ioctl_returns_enotty() {
    // 未知命令必须返回 ENOTTY (POSIX 约定)
    let mut ws = Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    let ret = sys_ioctl_contract(0, FIONREAD, &mut ws as *mut _ as u64);
    assert_eq!(ret, ENOTTY, "P1-I-39: 未知 ioctl 必须返回 -ENOTTY");
    let ret = sys_ioctl_contract(0, TIOCSETAF, &mut ws as *mut _ as u64);
    assert_eq!(ret, ENOTTY, "P1-I-39: 未知 ioctl 必须返回 -ENOTTY");
}

#[test]
fn arg_zero_returns_einval() {
    // arg=0 是无效指针, 必须返回 EINVAL
    let ret = sys_ioctl_contract(0, TIOCGWINSZ, 0);
    assert_eq!(ret, EINVAL, "P1-I-39: arg=0 必须返回 -EINVAL");
}

#[test]
fn tiocgwinsz_real_impl_fills_winsize() {
    // TIOCGWINSZ 是真实实现, 返回 0 并填充 ws 结构
    let mut ws = Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    let ret = sys_ioctl_contract(1, TIOCGWINSZ, &mut ws as *mut _ as u64);
    assert_eq!(ret, 0, "P1-I-39: TIOCGWINSZ 应返回 0");
    assert_eq!(ws.ws_row, 25, "P1-I-39: 终端行数应为 25");
    assert_eq!(ws.ws_col, 80, "P1-I-39: 终端列数应为 80");
}

#[test]
fn isatty_simulation_via_ioctl_return_code() {
    // P1-I-39 验收: isatty() 在非 tty fd 上正确返回 0 (不假设是终端).
    // isatty() = (ioctl(TCGETS) == 0); 修复后 TCGETS 返回 ENOSYS, 故 isatty() 正确返回 0.
    let mut ws = Winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    let fd = 99; // 非 tty fd
    let rc = sys_ioctl_contract(fd, TCGETS, &mut ws as *mut _ as u64);
    let isatty_result = if rc == 0 { 1 } else { 0 };
    assert_eq!(isatty_result, 0, "P1-I-39: ioctl 失败时 isatty() 必须返回 0 (非终端)");
}
