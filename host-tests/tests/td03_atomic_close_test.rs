// TD-03: 验证 VFS/HvFS 关闭路径已升级为原子 claim-and-clear
// 验收:
//   1. vfs_close_internal 在单一锁内同时检查 used 并清零, 不再分两段 (get_fd_info → free_fd)
//   2. hvfs close 在单一锁内同时检查 used 并清零
//   3. 两次连续 close 同一 fd, 第二次不应触发 pcache/inotify 二次回调
//
// 注: 静态契约扫描, 不进内核态.

use std::fs;
use std::path::Path;

const VFS_API: &str = "src/kernel/framework/fs/vfs/api.rs";
const HVFS: &str = "src/kernel/services/fs/hvfs/hvfs.rs";

fn read(path: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(path);
    fs::read_to_string(&p).unwrap_or_else(|_| panic!("读 {}", path))
}

#[test]
fn test_vfs_close_uses_atomic_claim_and_clear() {
    // TD-03: vfs_close_internal 必须在单一锁内同时检查 used + 清零 (atomic claim-and-clear)
    let src = read(VFS_API);
    // 截取 vfs_close_internal 函数体 (下一个 pub fn 之前)
    let body_start = src.find("pub fn vfs_close_internal")
        .expect("vfs_close_internal 必须存在");
    let after = &src[body_start..];
    let next_fn = after[40..].find("#[no_mangle]\npub fn ")
        .or_else(|| after[40..].find("pub fn vfs_"))
        .or_else(|| after[40..].find("pub fn "))
        .map(|i| 40 + i)
        .unwrap_or(after.len());
    let body = &after[..next_fn];
    // 验: 函数体内不能再调用 get_fd_info (V2 bug 的根源)
    assert!(!body.contains("get_fd_info("),
        "vfs_close_internal 不应再调用 get_fd_info (TD-03 原子化后快照内置):\n{}", body);
    // 验: 函数体内必须含 let mut fd_table = VFS_MANAGER.fd_table.lock();
    assert!(body.contains("let mut fd_table = VFS_MANAGER.fd_table.lock();"),
        "vfs_close_internal 必须使用 let mut fd_table 拿写锁 (TD-03 原子化)");
    // 验: 锁内必须同时含 used 检查与清零
    let lock_idx = body.find("let mut fd_table = VFS_MANAGER.fd_table.lock();")
        .expect("锁位置");
    let lock_block = &body[lock_idx..lock_idx.saturating_add(800).min(body.len())];
    assert!(lock_block.contains(".used = false"),
        "TD-03: 锁内必须清零 used 标志 (原子回收)");
}

#[test]
fn test_hvfs_close_uses_atomic_claim_and_clear() {
    // TD-03: hvfs HvDmu::close 必须在单一锁内同时检查 used 并清零
    let src = read(HVFS);
    let body_start = src.find("pub fn close(&self, fd: u32) -> i32")
        .expect("HvDmu::close 必须存在");
    let body = &src[body_start..body_start + 600];
    // 锁内同时含 used 检查与清零
    assert!(body.contains("let mut fds = self.fds.lock();"),
        "hvfs close 必须拿写锁 (TD-03 原子化)");
    assert!(body.contains("fds[idx].used = false"),
        "hvfs close 必须在锁内清零 used (TD-03 原子回收)");
    // 不再调用 self.free_fd (V2 bug: 释放与检查分离, TOCTOU 窗口)
    assert!(!body.contains("self.free_fd("),
        "hvfs close 不应再调用 self.free_fd (TD-03 内联原子回收)");
}

#[test]
fn test_vfs_close_second_call_is_noop() {
    // TD-03: 静态分析 vfs_close_internal 在第二次 close 同一 fd 时不应再触发
    // pcache_invalidate_inode / inotify_notify. 由 "let snapshot = ..." 的 None 分支保证.
    let src = read(VFS_API);
    let body_start = src.find("pub fn vfs_close_internal").unwrap();
    let body = &src[body_start..];
    // 必须有 snapshot 模式 + match
    assert!(body.contains("let snapshot = {"),
        "vfs_close_internal 必须用 snapshot 模式 (TD-03 原子 claim-and-clear)");
    // snapshot 返回 None 时必须 return 0 (跳过副作用)
    assert!(body.contains("None => return 0,"),
        "TD-03: snapshot None 时必须 return 0, 跳过 pcache/inotify");
}
