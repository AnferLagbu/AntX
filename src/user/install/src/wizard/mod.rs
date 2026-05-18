//! 安装向导 — 6 步交互式系统安装 → 持久化到磁盘

mod probe;
mod prepare;
mod deploy;
mod auth;
mod config;
mod finish;

use userlib::{println, read_line, fs_mount, fs_unmount};

const MOUNT_POINT: &[u8] = b"/mnt\0";

fn mount_target() -> bool {
    let r = fs_mount(b"none\0", MOUNT_POINT, b"hvfs\0", b"defaults\0");
    r == 0
}

fn unmount_target() {
    fs_unmount(MOUNT_POINT);
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

pub fn run() {
    welcome_page();

    if probe::detect() != 0 {
        println("Installation aborted: No disks available.");
        return;
    }
    if probe::choose() != 0 {
        println("Installation cancelled by user.");
        return;
    }

    let (disk_id, sectors) = probe::selected();

    if prepare::execute(disk_id, sectors) != 0 {
        println("Installation failed: Disk preparation error.");
        return;
    }

    println(""); println("Mounting target filesystem...");
    if !mount_target() {
        println("  [ERROR] Failed to mount HvFS to /mnt");
        println("Installation failed: Unable to access target disk.");
        return;
    }
    println("  [OK] Target mounted at /mnt");

    if deploy::deploy_all() != 0 {
        println("Installation failed: Application deployment error.");
        unmount_target();
        return;
    }
    if auth::create() != 0 {
        println("Installation failed at root identity setup.");
        unmount_target();
        return;
    }

    config::hostname();
    config::directory_tree();
    config::fstab();

    if finish::execute() != 0 {
        println("Warning: Installation may be incomplete.");
    }

    println("Unmounting target filesystem...");
    unmount_target();
}

pub fn needed() -> bool {
    finish::check_marker()
}
