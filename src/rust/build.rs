use std::fs;
use std::path::Path;

fn ensure_placeholder(path: &str, size: usize) {
    let p = Path::new(path);
    if !p.exists() {
        if let Some(parent) = p.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(p, vec![0u8; size]);
    }
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let base = Path::new(&manifest_dir).parent().unwrap().parent().unwrap();

    let stage1 = base.join("build/stage1.bin");
    ensure_placeholder(stage1.to_str().unwrap(), 440);

    let init = base.join("build/user/init.bin");
    ensure_placeholder(init.to_str().unwrap(), 512);

    println!("cargo:rerun-if-changed=build/stage1.bin");
    println!("cargo:rerun-if-changed=build/user/init.bin");
}
