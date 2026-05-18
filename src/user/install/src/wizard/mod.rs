//! 安装向导 — 6 步交互式系统安装
//!
//! 模块划分:
//!   probe    — Step 1: 磁盘探测与选择
//!   prepare  — Step 2: 分区、格式化、引导安装
//!   deploy   — Step 3: 应用批量部署
//!   auth     — Step 4: 管理员 PWID 创建
//!   config   — Step 5: 主机名、目录树、fstab
//!   finish   — Step 6: 安装标记、sync、重启
//!
//! 用法:
//!   use install::wizard;
//!   wizard::run();
//!   if wizard::needed() { ... }

mod probe;
mod prepare;
mod deploy;
mod auth;
mod config;
mod finish;

use userlib::{println, read_line};

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
    if deploy::deploy_all() != 0 {
        println("Installation failed: Application deployment error.");
        return;
    }
    if auth::create() != 0 {
        println("Installation failed at root identity setup.");
        return;
    }

    config::hostname();

    if finish::execute() != 0 {
        println("Warning: Installation may be incomplete.");
    }
}

pub fn needed() -> bool {
    finish::check_marker()
}
