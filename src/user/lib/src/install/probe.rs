//! 磁盘探测与选择
//!
//! 管理全局状态: DISK_LIST (最多4块磁盘), SELECTED_DISK, DISK_COUNT

use crate::io::{print, println, print_dec, read_line};
use crate::sys::{self, UserDiskInfo};

pub(crate) const MAX_DISKS: usize = 4;

static mut DISK_COUNT: i32 = 0;
static mut SELECTED_DISK: i32 = -1;
static mut DISK_LIST: [UserDiskInfo; MAX_DISKS] = [
    UserDiskInfo { disk_id: 0, present: 0, total_sectors: 0, sectors: 0, model: [0; 64] },
    UserDiskInfo { disk_id: 0, present: 0, total_sectors: 0, sectors: 0, model: [0; 64] },
    UserDiskInfo { disk_id: 0, present: 0, total_sectors: 0, sectors: 0, model: [0; 64] },
    UserDiskInfo { disk_id: 0, present: 0, total_sectors: 0, sectors: 0, model: [0; 64] },
];

pub fn selected() -> (u32, u64) {
    let idx = unsafe { SELECTED_DISK } as usize;
    let info = unsafe { &DISK_LIST[idx] };
    (info.disk_id, info.sectors as u64)
}

fn atoi(buf: &[u8], len: usize) -> i32 {
    let mut v: i32 = 0;
    for i in 0..len { if buf[i] >= b'0' && buf[i] <= b'9' { v = v * 10 + (buf[i] - b'0') as i32; } }
    v
}

/// 扫描 ATA 总线, 列出所有磁盘。成功返回 0。
pub fn detect() -> i32 {
    println("");
    println("--- Step 1: Disk Detection ---");
    println("");
    println("Scanning for available disks...");
    let mut disk_ids = [0u64; MAX_DISKS];
    let count = sys::disk_list(&mut disk_ids);
    if count <= 0 { println("  [ERROR] No disks detected!"); return -1; }
    unsafe { DISK_COUNT = count; }
    println("");
    print("Detected "); print_dec(count as i64); println(" disk(s):"); println("");
    for i in 0..(count as usize) {
        let info = unsafe { &mut DISK_LIST[i] };
        sys::disk_info(disk_ids[i] as u32, info);
        print("  ["); print_dec(i as i64); print("] Disk ");
        print_dec(info.disk_id as i64); print(": ");
        let model = core::str::from_utf8(&info.model).unwrap_or("Unknown");
        print(model.trim_end_matches('\0'));
        print(" (");
        let size_mb = info.sectors / 2 / 1024;
        if size_mb >= 1024 { print_dec((size_mb / 1024) as i64); print(" GB)");
        } else { print_dec(size_mb as i64); print(" MB)"); }
        println("");
    }
    0
}

/// 交互式选择目标磁盘。成功返回 0。
pub fn choose() -> i32 {
    println(""); println("Select a disk for installation:");
    let count = unsafe { DISK_COUNT as usize };
    print("Enter disk number (0-"); print_dec((count - 1) as i64); print("): ");
    let mut buf = [0u8; 8]; let len = read_line(&mut buf);
    if len == 0 { println("  [ERROR] No selection made."); return -1; }
    let sel = atoi(&buf, len);
    if sel < 0 || sel as usize >= count { println("  [ERROR] Invalid selection."); return -1; }
    unsafe { SELECTED_DISK = sel; }
    println(""); println("  [WARNING] ALL DATA ON THIS DISK WILL BE ERASED!");
    print("  Selected: ");
    let info = unsafe { &DISK_LIST[sel as usize] };
    let model = core::str::from_utf8(&info.model).unwrap_or("Unknown");
    println(model.trim_end_matches('\0'));
    println(""); print("Type 'yes' to confirm: ");
    let mut confirm = [0u8; 8]; read_line(&mut confirm);
    let y = (confirm[0] | 0x20) == b'y';
    let e = (confirm[1] | 0x20) == b'e';
    let s = (confirm[2] | 0x20) == b's';
    if !y || !e || !s { println("  Installation cancelled."); return -1; }
    0
}
