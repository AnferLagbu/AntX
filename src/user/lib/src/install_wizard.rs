//! 安装向导 — 6 步交互式系统安装
//! 被 init 和 install 两个二进制共用

use crate::sys;
use crate::io::{print, println, print_char, print_dec, print_hex, read_line};
use crate::str::cmp;
use crate::fs::{file_open, file_copy};
use crate::delay_loop;
use crate::sys::{O_CREAT, O_RDONLY, O_TRUNC, O_WRONLY};

const MIN_PASSWORD_LEN: usize = 4;
const DEFAULT_HOSTNAME: &[u8] = b"localhost\0";
const MARKER_FILE: &[u8] = b"/.antx_installed\0";
const HOSTNAME_FILE: &[u8] = b"/cfg/system/hostname\0";
const FSTAB_FILE: &[u8] = b"/cfg/system/fstab\0";
const MAX_DISKS: usize = 4;

static mut SELECTED_DISK: i32 = -1;
static mut DISK_COUNT: i32 = 0;
static mut DISK_LIST: [sys::UserDiskInfo; MAX_DISKS] = [
    sys::UserDiskInfo { disk_id: 0, present: 0, total_sectors: 0, sectors: 0, model: [0; 64] },
    sys::UserDiskInfo { disk_id: 0, present: 0, total_sectors: 0, sectors: 0, model: [0; 64] },
    sys::UserDiskInfo { disk_id: 0, present: 0, total_sectors: 0, sectors: 0, model: [0; 64] },
    sys::UserDiskInfo { disk_id: 0, present: 0, total_sectors: 0, sectors: 0, model: [0; 64] },
];

fn create_dir(path: &[u8]) {
    let mut p = [0u8; 128];
    let len = core::cmp::min(path.len(), 127);
    p[..len].copy_from_slice(&path[..len]);
    p[len] = 0;
    sys::fs_mkdir(&p[..len + 1]);
}

fn create_directory_structure() {
    println("Creating directory structure...");
    for dir in [
        b"/cfg\0".as_slice(), b"/cfg/boot\0".as_slice(), b"/cfg/kernel\0".as_slice(),
        b"/cfg/system\0".as_slice(), b"/cfg/gui\0".as_slice(),
        b"/app\0".as_slice(), b"/app/bin\0".as_slice(), b"/app/sys\0".as_slice(),
        b"/data\0".as_slice(), b"/data/id\0".as_slice(), b"/data/share\0".as_slice(),
        b"/data/var\0".as_slice(), b"/data/var/log\0".as_slice(), b"/data/var/run\0".as_slice(),
        b"/gui\0".as_slice(), b"/gui/font\0".as_slice(), b"/gui/theme\0".as_slice(),
        b"/gui/wallpaper\0".as_slice(), b"/gui/cursor\0".as_slice(),
        b"/dev\0".as_slice(), b"/proc\0".as_slice(), b"/temp\0".as_slice(), b"/mnt\0".as_slice(),
    ] {
        create_dir(dir);
    }
    println("  [OK] Directory structure created");
}

fn create_fstab() {
    let fd = file_open(FSTAB_FILE, O_CREAT | O_WRONLY | O_TRUNC);
    if fd < 0 { println("  [WARN] Failed to create fstab"); return; }
    let content = b"# AntX Filesystem Configuration\n# Format: source mountpoint type options\n\nnone    /dev    devfs   defaults\nnone    /proc   procfs  defaults\nnone    /temp   ramfs   defaults,size=64M\n";
    sys::fs_write(fd, content);
    sys::fs_close(fd);
    println("  [OK] fstab created");
}

fn welcome_page() {
    println("");
    println("========================================");
    println("        AntX Installation Wizard");
    println("========================================");
    println("");
    println("Welcome to AntX Operating System!");
    println("");
    println("This wizard will guide you through the");
    println("system installation process.");
    println("");
    println("Press ENTER to continue...");
    let mut buf = [0u8; 16];
    read_line(&mut buf);
}

fn detect_disks() -> i32 {
    println("");
    println("--- Step 1: Disk Detection ---");
    println("");
    println("Scanning for available disks...");
    let mut disk_ids = [0u64; MAX_DISKS];
    let count = sys::disk_list(&mut disk_ids);
    if count <= 0 { println("  [ERROR] No disks detected!"); return -1; }
    unsafe { DISK_COUNT = count; }
    println("");
    print("Detected "); print_dec(count as i64); println(" disk(s):");
    println("");
    for i in 0..(count as usize) {
        let info = unsafe { &mut DISK_LIST[i] };
        sys::disk_info(disk_ids[i] as u32, info);
        print("  ["); print_dec(i as i64); print("] Disk ");
        print_dec(info.disk_id as i64); print(": ");
        let model = core::str::from_utf8(&info.model).unwrap_or("Unknown");
        print(model.trim_end_matches('\0'));
        print(" (");
        let size_mb = info.sectors / 2 / 1024;
        if size_mb >= 1024 {
            print_dec((size_mb / 1024) as i64); print(" GB)");
        } else {
            print_dec(size_mb as i64); print(" MB)");
        }
        println("");
    }
    0
}

fn atoi(buf: &[u8], len: usize) -> i32 {
    let mut v: i32 = 0;
    for i in 0..len { if buf[i] >= b'0' && buf[i] <= b'9' { v = v * 10 + (buf[i] - b'0') as i32; } }
    v
}

fn select_disk() -> i32 {
    println(""); println("Select a disk for installation:");
    let count = unsafe { DISK_COUNT as usize };
    print("Enter disk number (0-"); print_dec((count - 1) as i64); print("): ");
    let mut buf = [0u8; 8];
    let len = read_line(&mut buf);
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
    let mut confirm = [0u8; 8];
    read_line(&mut confirm);
    if (confirm[0] | 0x20) != b'y' || (confirm[1] | 0x20) != b'e' || (confirm[2] | 0x20) != b's' {
        println("  Installation cancelled."); return -1;
    }
    0
}

fn format_disk() -> i32 {
    println(""); println("--- Step 2: Disk Partitioning & Formatting ---"); println("");
    let sel = unsafe { SELECTED_DISK } as usize;
    let info = unsafe { &DISK_LIST[sel] };
    println("Creating partition table (FAT16 boot + HvFS)...");
    let r = sys::disk_partition(info.disk_id, info.sectors as u64);
    if r != 0 { print("  [WARN] Partition table creation failed (error: "); print_dec(r); println(")");
    } else { println("  [OK] Partitions created"); }
    println("Formatting boot partition (FAT16)...");
    let r = sys::fat_format(info.disk_id);
    if r != 0 { print("  [WARN] FAT16 format failed (error: "); print_dec(r); println(")");
    } else { println("  [OK] Boot partition formatted"); }
    println("Formatting system partition (HvFS)...");
    let r = sys::disk_format(info.disk_id);
    if r != 0 { print("  [ERROR] HvFS format failed (error: "); print_dec(r as i64); println(")"); return -1; }
    println("  [OK] System partition formatted");
    println("Installing bootloader...");
    let r = sys::boot_install(info.disk_id);
    if r != 0 { print("  [ERROR] Bootloader installation failed (error: "); print_dec(r); println(")"); return -1; }
    println("  [OK] Bootloader installed (Stage1 + kernel)");
    0
}

fn install_system_files() -> i32 {
    println(""); println("--- Step 3: System File Installation ---"); println("");
    println("Installing system files...");
    let copies: [(&[u8], &[u8], &str); 4] = [
        (b"/boot/kernel.bin\0", b"/cfg/boot/kernel.bin\0", "Kernel"),
        (b"/bin/init\0", b"/app/sys/init\0", "Init process"),
        (b"/bin/axsh\0", b"/app/sys/axsh\0", "axsh"),
        (b"/bin/install\0", b"/app/sys/installguide\0", "Install guide"),
    ];
    for (src, dst, desc) in &copies {
        if file_copy(src, dst) { print("  [OK] "); print(desc); println(" installed"); }
    }
    0
}

fn config_root_pwid() -> i32 {
    println(""); println("--- Step 4: Administrator PWID Setup ---"); println("");
    println("Creating the first administrator identity.");
    println("This identity will have full system access."); println("");
    loop {
        print("Enter root password (min 4 chars): ");
        let mut pw1 = [0u8; 64]; let len1 = read_line(&mut pw1);
        if len1 < MIN_PASSWORD_LEN { print("Password too short! Minimum "); print_dec(MIN_PASSWORD_LEN as i64); println(" characters required."); continue; }
        print("Confirm root password: ");
        let mut pw2 = [0u8; 64]; let len2 = read_line(&mut pw2);
        if len1 != len2 || cmp(&pw1[..len1], &pw2[..len2]) != 0 { println("Passwords do not match! Please try again."); continue; }
        println(""); println("Creating root identity...");
        let mut p = [0u8; 65]; p[..len1].copy_from_slice(&pw1[..len1]); p[len1] = 0;
        let r = sys::auth_create_first(&p[..len1 + 1]);
        if r >= 0 { println("Root identity created successfully!"); return 0; }
        else { print("Failed to create root identity (error: "); print_dec(r as i64); println("). Please try again."); }
    }
}

fn config_system() {
    println(""); println("--- Step 5: System Configuration ---"); println("");
    print("Enter hostname (default: localhost): ");
    let mut hostname = [0u8; 64]; let len = read_line(&mut hostname);
    let name_slice: &[u8] = if len == 0 { DEFAULT_HOSTNAME } else { &hostname[..len + 1] };
    let name_len = if len == 0 { 9 } else { len };
    let r = sys::sethostname(&name_slice[..name_len]);
    if r == 0 { print("Hostname set to: "); let s = core::str::from_utf8(&name_slice[..name_len]).unwrap_or("?"); println(s); }
    else { println("Warning: Failed to set hostname, using default."); }
    let fd = file_open(HOSTNAME_FILE, O_CREAT | O_WRONLY | O_TRUNC);
    if fd >= 0 { sys::fs_write(fd, &name_slice[..name_len]); sys::fs_close(fd); }
    println(""); println("System configuration complete!");
}

fn complete_page() -> i32 {
    println(""); println("--- Step 6: Finalizing Installation ---"); println("");
    create_directory_structure(); create_fstab();
    println("Syncing filesystem to disk..."); sys::fs_sync();
    println("Creating installation marker...");
    let fd = file_open(MARKER_FILE, O_CREAT | O_WRONLY);
    if fd < 0 { println("Error: Failed to create installation marker!"); return -1; }
    sys::fs_write(fd, b"installed\n"); sys::fs_close(fd); sys::fs_sync();
    println(""); println("========================================");
    println("     Installation Complete!"); println("========================================");
    println(""); println("AntX has been installed to your disk.");
    println("Please remove the installation media"); println("and press ENTER to reboot your system.");
    println("");
    let mut buf = [0u8; 16]; read_line(&mut buf);
    println("Rebooting system..."); delay_loop(10_000_000);
    sys::reboot(0);
    0
}

pub fn run() {
    welcome_page();
    if detect_disks() != 0 { println("Installation aborted: No disks available."); return; }
    if select_disk() != 0 { println("Installation cancelled by user."); return; }
    if format_disk() != 0 { println("Installation failed: Disk formatting error."); return; }
    if install_system_files() != 0 { println("Installation failed: File copy error."); return; }
    if config_root_pwid() != 0 { println("Installation failed at root identity setup."); return; }
    config_system();
    if complete_page() != 0 { println("Warning: Installation may be incomplete."); }
}

pub fn check_needed() -> bool {
    let fd = file_open(MARKER_FILE, O_RDONLY);
    if fd >= 0 { sys::fs_close(fd); false } else { true }
}
