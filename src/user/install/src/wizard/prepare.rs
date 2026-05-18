//! 磁盘准备: 分区表创建、FAT16/HvFS 格式化、引导器安装

use userlib::{print, println, print_dec};
use userlib::sys;

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
