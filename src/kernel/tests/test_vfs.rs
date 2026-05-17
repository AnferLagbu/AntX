use crate::kernel::fs::vfs::vfs::VfsManager;
use crate::kernel::fs::vfs::types::*;
use crate::kernel::tests::{runner, TestResult};
use super::check;

fn test_fstype_from_name() -> TestResult {
    let ramfs = FsType::from_name("ramfs");
    check!(ramfs == FsType::RamFs, "ramfs should be RamFs");

    let hvfs = FsType::from_name("hvfs");
    check!(hvfs == FsType::HvFs, "hvfs should be HvFs");

    let unknown = FsType::from_name("ext4");
    check!(unknown == FsType::Unknown, "ext4 should be Unknown");
    TestResult::Pass
}

fn test_fstype_as_str() -> TestResult {
    check!(FsType::RamFs.as_str() == "ramfs", "RamFs as_str mismatch");
    check!(FsType::HvFs.as_str() == "hvfs", "HvFs as_str mismatch");
    check!(FsType::Unknown.as_str() == "unknown", "Unknown as_str mismatch");
    TestResult::Pass
}

fn test_vfs_file_type() -> TestResult {
    check!(VfsFileType::from_u8(0) == VfsFileType::File, "0 should be File");
    check!(VfsFileType::from_u8(1) == VfsFileType::Dir, "1 should be Dir");
    check!(VfsFileType::from_u8(2) == VfsFileType::Dev, "2 should be Dev");
    check!(VfsFileType::from_u8(3) == VfsFileType::Symlink, "3 should be Symlink");
    check!(VfsFileType::from_u8(99) == VfsFileType::File, "invalid should fallback to File");

    check!(VfsFileType::Dir.as_u8() == 1, "Dir as_u8 should be 1");
    TestResult::Pass
}

fn test_vfs_seek_whence() -> TestResult {
    check!(VfsSeekWhence::from_u32(0) == VfsSeekWhence::Set, "0 should be Set");
    check!(VfsSeekWhence::from_u32(1) == VfsSeekWhence::Cur, "1 should be Cur");
    check!(VfsSeekWhence::from_u32(2) == VfsSeekWhence::End, "2 should be End");
    check!(VfsSeekWhence::from_u32(99) == VfsSeekWhence::Set, "invalid should fallback to Set");
    TestResult::Pass
}

fn test_vfs_mount_unmount() -> TestResult {
    let mgr = VfsManager::new();
    let result = mgr.mount("/", "ramfs");
    check!(result.is_ok(), "mount / should succeed");

    let dup = mgr.mount("/", "ramfs");
    check!(dup.is_err(), "duplicate mount should fail");

    let found = mgr.find_mount("/");
    check!(found.is_some(), "should find / mount");

    let unmount_result = mgr.unmount("/");
    check!(unmount_result.is_ok(), "unmount / should succeed");

    let not_found = mgr.find_mount("/");
    check!(not_found.is_none(), "should not find / after unmount");
    TestResult::Pass
}

fn test_vfs_resolve_mount() -> TestResult {
    let mgr = VfsManager::new();
    let _ = mgr.mount("/", "ramfs");
    let _ = mgr.mount("/home", "hvfs");

    let root = mgr.resolve_mount("/");
    check!(root.is_some(), "should resolve /");
    let (_idx, fs_type) = root.unwrap();
    check!(fs_type == FsType::RamFs, "/ should be RamFs");

    let home = mgr.resolve_mount("/home/user/file.txt");
    check!(home.is_some(), "should resolve /home/user/file.txt");
    let (_, home_fs) = home.unwrap();
    check!(home_fs == FsType::HvFs, "/home should be HvFs");

    let rel = mgr.get_relative_path("/home/user/file.txt", home.unwrap().0);
    check!(rel == "user/file.txt", "relative path mismatch");
    TestResult::Pass
}

fn test_vfs_fd_alloc_free() -> TestResult {
    let mgr = VfsManager::new();
    let fd1 = mgr.alloc_fd();
    check!(fd1.is_some(), "first alloc should succeed");

    let fd2 = mgr.alloc_fd();
    check!(fd2.is_some(), "second alloc should succeed");
    check!(fd1.unwrap() != fd2.unwrap(), "fds should be different");

    mgr.free_fd(fd1.unwrap());
    let fd3 = mgr.alloc_fd();
    check!(fd3.is_some(), "alloc after free should succeed");
    TestResult::Pass
}

fn test_vfs_dirent() -> TestResult {
    let mut dirent = VfsDirent::new();
    dirent.set_name("test.txt");
    let name = dirent.get_name();
    check!(name == "test.txt", "dirent name mismatch");
    TestResult::Pass
}

fn test_vfs_cwd() -> TestResult {
    let mgr = VfsManager::new();
    mgr.set_cwd("/home/user");
    let cwd = mgr.get_cwd();
    check!(cwd == "/home/user", "cwd mismatch");
    TestResult::Pass
}

fn test_vfs_snapshot_restore() -> TestResult {
    let mgr = VfsManager::new();
    let _ = mgr.mount("/", "ramfs");
    mgr.capture_snapshot();

    let _ = mgr.unmount("/");
    check!(mgr.find_mount("/").is_none(), "mount should be gone after unmount");

    mgr.restore_from_snapshot();
    let found = mgr.find_mount("/");
    check!(found.is_some(), "mount should be restored after snapshot restore");
    TestResult::Pass
}

pub fn register_vfs_tests() {
    let r = runner();
    r.register("vfs::types", "fstype_from_name", test_fstype_from_name);
    r.register("vfs::types", "fstype_as_str", test_fstype_as_str);
    r.register("vfs::types", "file_type", test_vfs_file_type);
    r.register("vfs::types", "seek_whence", test_vfs_seek_whence);
    r.register("vfs::mgr", "mount_unmount", test_vfs_mount_unmount);
    r.register("vfs::mgr", "resolve_mount", test_vfs_resolve_mount);
    r.register("vfs::mgr", "fd_alloc_free", test_vfs_fd_alloc_free);
    r.register("vfs::types", "dirent", test_vfs_dirent);
    r.register("vfs::mgr", "cwd", test_vfs_cwd);
    r.register("vfs::mgr", "snapshot_restore", test_vfs_snapshot_restore);
}
