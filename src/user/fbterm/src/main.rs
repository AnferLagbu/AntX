#![no_std]
#![no_main]

use userlib::*;

const FB_USER_VADDR: u64 = 0x100000000;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    userlib::print("[fbterm] PANIC: ");
    if let Some(loc) = info.location() {
        userlib::print("at "); userlib::print(loc.file());
        userlib::print(":"); print_dec(loc.line() as i64);
    }
    userlib::print("\n");
    proc_exit(1);
}

fn draw_test_pattern(fb: &mut [u8], width: u32, height: u32, pitch: u32, bpp: u8) {
    let bytes_per_pixel = (bpp as u32 / 8) as usize;
    if bytes_per_pixel < 2 || bytes_per_pixel > 4 {
        return;
    }

    // 顶部渐变条 (彩色)
    let bar_height = height / 8;
    let colors: [(u8, u8, u8); 7] = [
        (255, 0, 0), (255, 127, 0), (255, 255, 0),
        (0, 255, 0), (0, 0, 255), (75, 0, 130), (143, 0, 255),
    ];

    for y in 0..bar_height {
        let color_idx = ((y * 7) / bar_height) as usize;
        let (r, g, b) = colors[color_idx.min(6)];
        for x in 0..width {
            let pixel_offset = (y * pitch) as usize + (x as u32) as usize * bytes_per_pixel;
            if pixel_offset + bytes_per_pixel <= fb.len() {
                match bytes_per_pixel {
                    2 => {
                        let pixel = ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3);
                        fb[pixel_offset] = pixel as u8;
                        fb[pixel_offset + 1] = (pixel >> 8) as u8;
                    }
                    3 | 4 => {
                        fb[pixel_offset] = b;
                        fb[pixel_offset + 1] = g;
                        fb[pixel_offset + 2] = r;
                        if bytes_per_pixel == 4 {
                            fb[pixel_offset + 3] = 255;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // 中间区域：白色背景
    let term_y_start = bar_height;
    let term_y_end = height;
    for y in term_y_start..term_y_end {
        for x in 0..width {
            let pixel_offset = (y * pitch) as usize + (x as u32) as usize * bytes_per_pixel;
            if pixel_offset + bytes_per_pixel <= fb.len() {
                let (r, g, b) = if y < term_y_start + 24 {
                    (240, 240, 245) // 略偏灰白
                } else {
                    (255, 255, 255)
                };
                match bytes_per_pixel {
                    2 => {
                        let pixel = ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3);
                        fb[pixel_offset] = pixel as u8;
                        fb[pixel_offset + 1] = (pixel >> 8) as u8;
                    }
                    3 | 4 => {
                        fb[pixel_offset] = b;
                        fb[pixel_offset + 1] = g;
                        fb[pixel_offset + 2] = r;
                        if bytes_per_pixel == 4 {
                            fb[pixel_offset + 3] = 255;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // 底部状态栏底色
    let status_y_start = height - 24.min(height);
    for y in status_y_start..height {
        for x in 0..width {
            let pixel_offset = (y * pitch) as usize + (x as u32) as usize * bytes_per_pixel;
            if pixel_offset + bytes_per_pixel <= fb.len() {
                match bytes_per_pixel {
                    2 => {
                        let pixel: u16 = ((0x30 as u16 >> 3) << 11) | ((0x30 as u16 >> 2) << 5) | (0x40 >> 3);
                        fb[pixel_offset] = pixel as u8;
                        fb[pixel_offset + 1] = (pixel >> 8) as u8;
                    }
                    3 | 4 => {
                        fb[pixel_offset] = 0x40;
                        fb[pixel_offset + 1] = 0x30;
                        fb[pixel_offset + 2] = 0x30;
                        if bytes_per_pixel == 4 {
                            fb[pixel_offset + 3] = 255;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn fbterm_main(fb: &mut [u8], width: u32, _height: u32, pitch: u32, bpp: u8) {
    let bytes_per_pixel = (bpp as u32 / 8) as usize;
    println("[fbterm] Framebuffer ready");
    print("[fbterm] Resolution: "); print_dec(width as i64);
    print(" x "); print_dec(_height as i64);
    print(" @ "); print_dec(bpp as i64); println(" bpp");

    draw_test_pattern(fb, width, _height, pitch, bpp);

    println("[fbterm] Test pattern drawn. Press Enter to exit.");

    // 简单的文本叠加 — 在顶部白底区域写几行
    let line_y_start: u32 = (_height / 8) + 4; // 紧接渐变条下方
    let msg_lines: [&str; 6] = [
        "*** AntX fbterm - User-Space Framebuffer Terminal ***",
        "",
        "Display info:",
        "",
        "This is a proof-of-concept user-space framebuffer terminal.",
        "fb_open/fb_mmap syscalls are working correctly.",
    ];

    // 读取回车退出
    let mut buf = [0u8; 1];
    let _ = read(0, &mut buf);

    // 清空屏幕为黑色（礼貌退出）
    for y in 0.._height {
        for x in 0..width {
            let offset = (y * pitch) as usize + (x as u32) as usize * bytes_per_pixel;
            if offset + bytes_per_pixel <= fb.len() {
                for b in 0..bytes_per_pixel {
                    if b < fb.len() - offset {
                        fb[offset + b] = 0;
                    }
                }
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    println("[fbterm] Starting user-space framebuffer terminal...");

    let mut info = FbInfo {
        phys_addr: 0, size: 0, width: 0, height: 0, pitch: 0, bpp: 0, _pad: [0; 3],
    };

    let ret = fb_open(&mut info);
    if ret < 0 {
        print("[fbterm] ERROR: fb_open failed: "); print_dec(ret); println("");
        println("[fbterm] No framebuffer device available.");
        proc_exit(1);
    }

    print("[fbterm] FB phys=0x"); print_hex(info.phys_addr);
    print(" size="); print_dec(info.size as i64);
    print(" "); print_dec(info.width as i64);
    print("x"); print_dec(info.height as i64);
    print(" pitch="); print_dec(info.pitch as i64);
    print(" bpp="); print_dec(info.bpp as i64); println("");

    let map_size = info.size;
    let ret = fb_mmap(FB_USER_VADDR, map_size, 3);
    if ret < 0 {
        print("[fbterm] ERROR: fb_mmap failed: "); print_dec(ret); println("");
        proc_exit(1);
    }

    println("[fbterm] Framebuffer mapped to user space");

    let fb_slice = unsafe {
        core::slice::from_raw_parts_mut(FB_USER_VADDR as *mut u8, map_size as usize)
    };

    fbterm_main(fb_slice, info.width, info.height, info.pitch, info.bpp);

    let _ = fb_release(FB_USER_VADDR);
    proc_exit(0);
}