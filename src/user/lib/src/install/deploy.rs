/// 应用部署 — 批量复制系统二进制文件至安装目标

use crate::io::{print, println, print_dec};
use crate::fs::file_copy;

pub struct AppManifest {
    pub src:  &'static [u8],   // 源路径 (RamFS 上的 Live 文件)
    pub dst:  &'static [u8],   // 目标路径 (磁盘 HvFS)
    pub desc: &'static str,    // 人类可读名称
}

static MANIFEST: &[AppManifest] = &[
    AppManifest { src: b"/boot/kernel.bin\0", dst: b"/cfg/boot/kernel.bin\0", desc: "Kernel" },
    AppManifest { src: b"/bin/init\0",        dst: b"/app/sys/init\0",        desc: "Init process" },
    AppManifest { src: b"/bin/axsh\0",        dst: b"/app/sys/axsh\0",        desc: "axsh Shell" },
    AppManifest { src: b"/bin/install\0",     dst: b"/app/sys/installguide\0",desc: "Install guide" },
];

#[allow(dead_code)]
pub fn register(_app: AppManifest) {}

pub fn deploy_all() -> i32 {
    println(""); println("--- Step 3: Application Deployment ---"); println("");
    let mut ok = 0u32;
    let mut fail = 0u32;
    for m in MANIFEST {
        print("  "); print(m.desc);
        if file_copy(m.src, m.dst) { println(" ... OK"); ok += 1; }
        else { println(" ... FAIL"); fail += 1; }
    }
    println("");
    print("  Installed: "); print_dec(ok as i64);
    print(" / "); print_dec((ok + fail) as i64); println("");
    if fail > 0 { return -1; }
    0
}
