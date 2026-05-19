//! 命令注册与路由 (Command Registry & Dispatch)

pub mod general;
pub mod fileops;
pub mod system;
pub mod identity;

use core::str::from_utf8;

// ── Cmd 解析结构 ──

#[derive(Clone)]
pub struct Cmd {
    pub n: usize,
    args: [u8; 1024],  // 原始参数字节
    offsets: [usize; 32], // 每个参数的起始偏移
}

impl Cmd {
    pub fn new(input: &[u8]) -> Self {
        let mut offsets = [0usize; 32];
        let mut args = [0u8; 1024];
        let mut n = 0usize;
        let mut pos = 0usize;
        let len = input.len().min(1023);

        // 复制输入 (去除尾随换行)
        let end = if len > 0 && input[len - 1] == b'\n' { len - 1 } else { len };
        let end = if end > 0 && input[end - 1] == b'\r' { end - 1 } else { end };
        let end = end.min(1023);
        args[..end].copy_from_slice(&input[..end]);
        args[end] = 0;

        // 分词
        let mut i = 0;
        while i < end {
            // 跳过空白
            while i < end && (args[i] == b' ' || args[i] == b'\t') { i += 1; }
            if i >= end { break; }

            // 引号处理
            if args[i] == b'"' {
                i += 1;
                offsets[n] = i;
                while i < end && args[i] != b'"' { i += 1; }
                if i < end { args[i] = 0; i += 1; }
            } else {
                offsets[n] = i;
                while i < end && args[i] != b' ' && args[i] != b'\t' { i += 1; }
                if i < end { args[i] = 0; i += 1; }
            }
            n += 1;
            if n >= 32 { break; }
        }
        Self { n, args, offsets }
    }

    pub fn get(&self, idx: usize) -> &[u8] {
        if idx >= self.n { return b""; }
        let start = self.offsets[idx];
        let end = start + core::ffi::CStr::from_bytes_until_nul(&self.args[start..])
            .map(|c| c.to_bytes().len()).unwrap_or(0);
        &self.args[start..start + end]
    }
}

pub fn as_str(slice: &[u8]) -> &str { from_utf8(slice).unwrap_or("") }

/// 从 cmd 获取 path 参数 (支持引号路径)
pub fn path_arg(cmd: &Cmd) -> Option<[u8; 256]> {
    if cmd.n < 2 { return None; }
    let mut buf = [0u8; 256];
    let raw = cmd.get(1);
    let len = raw.len().min(255);
    buf[..len].copy_from_slice(&raw[..len]);
    buf[len] = 0;
    Some(buf)
}

// ── 命令注册表 & 调度 ──

type CmdFn = fn(&Cmd);

struct Entry {
    name: &'static str,
    func: CmdFn,
}

static TABLE: &[Entry] = &[
    // Shell 内置
    Entry { name: "help",   func: general::help  },
    Entry { name: "clear",  func: general::clear },
    Entry { name: "echo",   func: general::echo  },
    Entry { name: "exit",   func: general::exit  },
    // 文件操作
    Entry { name: "dir",    func: fileops::dir   },
    Entry { name: "cd",     func: fileops::cd    },
    Entry { name: "pwd",    func: fileops::pwd   },
    Entry { name: "cat",    func: fileops::cat   },
    Entry { name: "mkdir",  func: fileops::mkdir },
    Entry { name: "touch",  func: fileops::touch },
    Entry { name: "del",    func: fileops::del   },
    Entry { name: "cp",     func: fileops::cp    },
    Entry { name: "mv",     func: fileops::mv    },
    Entry { name: "save",   func: fileops::save  },
    // 身份
    Entry { name: "login",  func: identity::login  },
    Entry { name: "logout", func: identity::logout },
    Entry { name: "who",    func: identity::who    },
    Entry { name: "passwd", func: identity::passwd },
    // 系统
    Entry { name: "osinfo", func: system::osinfo },
    Entry { name: "host",   func: system::host   },
    Entry { name: "ps",     func: system::ps     },
    Entry { name: "reboot", func: system::reboot },
    Entry { name: "halt",   func: system::halt   },
];

pub fn dispatch(cmd: &Cmd) {
    let name = as_str(cmd.get(0));
    if name.is_empty() { return; }

    // 搜索精确匹配
    for entry in TABLE {
        if entry.name == name {
            (entry.func)(cmd);
            return;
        }
    }

    userlib::print("axsh: '");
    userlib::print(name);
    userlib::println("' unknown — try 'help'");
}