/// Shell 内置命令: help, clear, echo, exit

use userlib::{print, println, print_hex};

use super::{Cmd, as_str};

pub fn help(cmd: &Cmd) {
    let cat = if cmd.n > 1 { as_str(cmd.get(1)) } else { "" };

    let show = |c: &str| cat.is_empty() || cat == c;

    if show("file") { println("\n▼ 文件操作"); }
    if show("file") {
        println("  dir      列出目录内容        dir [/path]");
        println("  cd       切换工作目录        cd <dir>");
        println("  pwd      显示当前路径        pwd");
        println("  cat      显示文件内容        cat <file>");
        println("  mkdir    创建目录            mkdir <dir>");
        println("  touch    创建空文件          touch <file>");
        println("  del      删除文件/目录       del <path>");
        println("  cp       复制文件            cp <src> <dst>");
        println("  mv       移动/重命名         mv <src> <dst>");
        println("  save     写入文本到文件      save <file> <text>");
    }
    if show("sys") { println("\n▼ 系统"); }
    if show("sys") {
        println("  osinfo   系统版本/架构       osinfo");
        println("  host     显示/设置主机名     host [name]");
        println("  ps       进程列表            ps");
        println("  reboot   重启系统            reboot");
        println("  halt     关机                halt");
    }
    if show("id") { println("\n▼ 身份"); }
    if show("id") {
        println("  login    登录                login <note> <pw>");
        println("  logout   登出                logout");
        println("  who      当前身份            who");
        println("  passwd   修改口令            passwd");
    }
    if show("shell") { println("\n▼ Shell 内置"); }
    if show("shell") {
        println("  help     显示帮助            help [file|sys|id|shell]");
        println("  clear    清屏                clear");
        println("  echo     回显文本            echo [text...]");
        println("  exit     退出 Shell          exit");
    }

    if cat.is_empty() {
        println("\nAntX Shell — deep & lightweight");
        println("Use 'help <category>' for details");
    }
}

pub fn clear(_: &Cmd) {
    // ANSI 清屏
    print("\x1b[2J\x1b[H");
}

pub fn echo(cmd: &Cmd) {
    let parts: alloc::vec::Vec<&str> = (1..cmd.n).map(|i| as_str(cmd.get(i))).collect();
    println(&parts.join(" "));
}

pub fn exit(_: &Cmd) {
    // 由 shell 主循环处理
    crate::MAIN_EXIT.store(true, core::sync::atomic::Ordering::SeqCst);
}