//! 磁盘准备: 分区表创建、FAT16/HvFS 格式化、引导器安装
//!
//! 调用顺序:
//!   1. disk_partition(did, sectors) → 双分区 MBR
//!   2. fat_format(did)              → FAT16 boot 分区
//!   3. disk_format(did)             → HvFS 系统分区
//!   4. boot_install(did)            → Stage1 + kernel raw 写入

use crate::io::{print, println, print_dec};
use crate::sys;

pub fn execute(disk_id: u32, sectors: u64) -> i32 {
    println(""); println("--- Step 2: Disk Partitioning & Formatting ---"); println("");

    step("Creating partition table (FAT16 boot + HvFS)", || {
        let r = sys::disk_partition(disk_id, sectors);
        (r == 0, r)
    });

    step("Formatting boot partition (FAT16)", || {
        let r = sys::fat_format(disk_id);
        (r == 0, r)
    });

    step("Formatting system partition (HvFS)", || {
        let r = sys::disk_format(disk_id);
        (r == 0, r as i64)
    });

    println("Installing bootloader...");
    let r = sys::boot_install(disk_id);
    if r != 0 {
        print("  [ERROR] Bootloader installation failed (error: ");
        print_dec(r); println(")");
        return -1;
    }
    println("  [OK] Bootloader installed (Stage1 + kernel)");
    0
}

fn step(label: &str, f: impl FnOnce() -> (bool, i64)) {
    print("  "); print(label); println("...");
    let (ok, code) = f();
    if ok { println("    [OK]"); }
    else { print("    [WARN] (code "); print_dec(code); println(")"); }
}
