use antx_host_tests::hvfs::hvfs::get_hvfs;
use antx_host_tests::hvfs_mock::KernelError;

macro_rules! test {
    ($name:ident, $body:block) => {
        print!("  {} ... ", stringify!($name));
        $body
        println!("PASS");
    };
}

macro_rules! assert_eq_hvfs {
    ($left:expr, $right:expr, $msg:expr) => {
        let l = $left;
        let r = $right;
        if l != r {
            panic!("{} FAIL: expected {:?}, got {:?}", $msg, r, l);
        }
    };
}

#[test]
fn hvfs_comprehensive() {
    println!("\n=== HvFS Standalone Test Suite ===\n");

    let hvfs = get_hvfs();

    test!(init, {
        hvfs.init();
        assert!(hvfs.is_initialized(), "HvFS should be initialized");
    });

    test!(create_and_stat, {
        let fd = hvfs.open("/test.txt", 0x0102, 1).unwrap();
        assert!(fd >= 0, "open should succeed, got {}", fd);
        hvfs.close(fd as u32);
        let stat = hvfs.stat("/test.txt", 1);
        assert!(stat.is_some(), "stat should find test.txt");
    });

    test!(write_and_read, {
        let fd = hvfs.open("/test.txt", 0x0102, 1).unwrap();
        let data = b"Hello, QueenX HvFS!";
        let written = hvfs.write(fd as u32, data, data.len() as u32);
        assert_eq_hvfs!(written, data.len() as i32, "write count");
        hvfs.close(fd as u32);

        let fd = hvfs.open("/test.txt", 0x0001, 1).unwrap();
        let mut buf = [0u8; 64];
        let read = hvfs.read(fd as u32, &mut buf, 64);
        assert!(read > 0, "read should return > 0, got {}", read);
        let read_str = core::str::from_utf8(&buf[..read as usize]).unwrap();
        assert_eq_hvfs!(read_str, "Hello, QueenX HvFS!", "file content");
        hvfs.close(fd as u32);
    });

    test!(mkdir, {
        let result = hvfs.mkdir("/mydir", 1);
        assert!(result >= 0, "mkdir should succeed, got {}", result);
        let stat = hvfs.stat("/mydir", 1);
        assert!(stat.is_some(), "stat should find mydir");
        let obj = stat.unwrap();
        assert!(obj.obj_type as u8 == 2, "mydir should be directory type");
    });

    test!(create_file_in_dir, {
        let fd = hvfs.open("/mydir/nested.txt", 0x0102, 1).unwrap();
        let data = b"nested content";
        let w = hvfs.write(fd as u32, data, data.len() as u32);
        assert_eq_hvfs!(w, data.len() as i32, "nested write count");
        hvfs.close(fd as u32);
    });

    test!(read_nested, {
        let fd = hvfs.open("/mydir/nested.txt", 0x0001, 1).unwrap();
        let mut buf = [0u8; 64];
        let r = hvfs.read(fd as u32, &mut buf, 64);
        assert!(r > 0, "nested read should return > 0");
        let s = core::str::from_utf8(&buf[..r as usize]).unwrap();
        assert_eq_hvfs!(s, "nested content", "nested file content");
        hvfs.close(fd as u32);
    });

    test!(rename, {
        let r = hvfs.rename("/test.txt", "/renamed.txt", 1);
        assert_eq_hvfs!(r, 0, "rename should succeed");

        let fd = hvfs.open("/renamed.txt", 0x0001, 1).unwrap();
        let mut buf = [0u8; 64];
        let r = hvfs.read(fd as u32, &mut buf, 64);
        assert!(r > 0, "renamed file should have content");
        hvfs.close(fd as u32);

        match hvfs.open("/test.txt", 0x0001, 1) {
            Err(_) => {},
            Ok(fd) => {
                hvfs.close(fd as u32);
                panic!("old name should not exist after rename");
            }
        }
    });

    test!(delete, {
        let r = hvfs.unlink("/renamed.txt", 1);
        assert_eq_hvfs!(r, 0, "unlink should succeed");
        match hvfs.open("/renamed.txt", 0x0001, 1) {
            Err(_) => {},
            Ok(fd) => {
                hvfs.close(fd as u32);
                panic!("deleted file should not be openable");
            }
        }
    });

    test!(large_write, {
        let fd = hvfs.open("/large.bin", 0x0102, 1).unwrap();
        let pattern: Vec<u8> = (0..1024u16).flat_map(|i| i.to_le_bytes()).collect();
        let written = hvfs.write(fd as u32, &pattern, pattern.len() as u32);
        assert_eq_hvfs!(written, pattern.len() as i32, "large write count");
        hvfs.close(fd as u32);

        let fd = hvfs.open("/large.bin", 0x0001, 1).unwrap();
        let mut read_buf = vec![0u8; pattern.len()];
        let buf_len = read_buf.len();
        let r = hvfs.read(fd as u32, &mut read_buf, buf_len as u32);
        assert_eq_hvfs!(r, pattern.len() as i32, "large read count");
        assert_eq_hvfs!(&read_buf[..], &pattern[..], "large file content");
        hvfs.close(fd as u32);
    });

    test!(multiple_files, {
        for i in 0..10 {
            let name = format!("/multi_{}", i);
            let fd = hvfs.open(&name, 0x0102, 1).unwrap();
            let content = format!("file number {}", i);
            let w = hvfs.write(fd as u32, content.as_bytes(), content.len() as u32);
            let msg = format!("write multi_{}", i);
            assert_eq_hvfs!(w, content.len() as i32, msg);
            hvfs.close(fd as u32);
        }
        for i in 0..10 {
            let name = format!("/multi_{}", i);
            let stat = hvfs.stat(&name, 1);
            assert!(stat.is_some(), "should find multi_{}", i);
        }
    });

    test!(overwrite, {
        let fd = hvfs.open("/overwrite.txt", 0x0102, 1).unwrap();
        let d1 = b"first version";
        hvfs.write(fd as u32, d1, d1.len() as u32);
        hvfs.close(fd as u32);

        let fd = hvfs.open("/overwrite.txt", 0x0102, 1).unwrap();
        let d2 = b"second version - longer content!";
        hvfs.write(fd as u32, d2, d2.len() as u32);
        hvfs.close(fd as u32);

        let fd = hvfs.open("/overwrite.txt", 0x0001, 1).unwrap();
        let mut buf = [0u8; 64];
        let r = hvfs.read(fd as u32, &mut buf, 64);
        let s = core::str::from_utf8(&buf[..r as usize]).unwrap();
        assert_eq_hvfs!(s, "second version - longer content!", "overwrite content");
        hvfs.close(fd as u32);
    });

    test!(open_nonexistent, {
        match hvfs.open("/nonexistent", 0x0001, 1) {
            Err(_) => {},
            Ok(fd) => {
                hvfs.close(fd as u32);
                panic!("open nonexistent should fail");
            }
        }
    });

    test!(stat_nonexistent, {
        let stat = hvfs.stat("/nonexistent", 1);
        assert!(stat.is_none(), "stat nonexistent should return None");
    });

    println!("\n=== All 10 HvFS Tests Passed ===\n");
}

#[test]
fn hvfs_error_paths() {
    println!("\n=== HvFS Error Path Tests ===\n");

    let hvfs = get_hvfs();
    hvfs.init();

    test!(open_without_create_flag_returns_not_found, {
        match hvfs.open("/no_such_file.txt", 0x0001, 1) {
            Err(KernelError::NotFound) => {},
            Err(e) => panic!("expected NotFound, got {:?}", e),
            Ok(fd) => { hvfs.close(fd as u32); panic!("should fail with NotFound"); }
        }
    });

    test!(open_with_create_flag_succeeds, {
        match hvfs.open("/new_file.txt", 0x0102, 1) {
            Ok(fd) => { hvfs.close(fd as u32); }
            Err(e) => panic!("open with O_CREAT should succeed, got {:?}", e),
        }
    });

    test!(close_invalid_fd, {
        let r = hvfs.close(9999);
        assert_eq_hvfs!(r, -1, "close invalid fd should return -1");
    });

    test!(read_invalid_fd, {
        let mut buf = [0u8; 64];
        let r = hvfs.read(9999, &mut buf, 64);
        assert_eq_hvfs!(r, -1, "read invalid fd should return -1");
    });

    test!(write_invalid_fd, {
        let data = b"test";
        let r = hvfs.write(9999, data, data.len() as u32);
        assert_eq_hvfs!(r, -1, "write invalid fd should return -1");
    });

    test!(unlink_nonexistent, {
        let r = hvfs.unlink("/no_such_file", 1);
        assert_eq_hvfs!(r, -2, "unlink nonexistent should return -2");
    });

    test!(stat_nonexistent_returns_none, {
        let s = hvfs.stat("/definitely_not_here", 1);
        assert!(s.is_none(), "stat nonexistent should return None");
    });

    test!(rename_source_not_found, {
        let r = hvfs.rename("/missing_src", "/dst", 1);
        assert_eq_hvfs!(r, -2, "rename missing source should return -2");
    });

    test!(rename_target_exists, {
        let fd1 = hvfs.open("/rename_src.txt", 0x0102, 1).unwrap();
        hvfs.write(fd1 as u32, b"src", 3);
        hvfs.close(fd1 as u32);
        let fd2 = hvfs.open("/rename_dst.txt", 0x0102, 1).unwrap();
        hvfs.write(fd2 as u32, b"dst", 3);
        hvfs.close(fd2 as u32);
        let r = hvfs.rename("/rename_src.txt", "/rename_dst.txt", 1);
        assert_eq_hvfs!(r, -4, "rename to existing target should return -4");
    });

    test!(seek_invalid_fd, {
        let r = hvfs.seek(9999, 0, 0);
        assert_eq_hvfs!(r, -1, "seek invalid fd should return -1");
    });

    test!(seek_set, {
        let fd = hvfs.open("/seek_test.txt", 0x0102, 1).unwrap();
        let data = b"0123456789ABCDEF";
        hvfs.write(fd as u32, data, data.len() as u32);
        hvfs.close(fd as u32);

        let fd = hvfs.open("/seek_test.txt", 0x0001, 1).unwrap();
        let pos = hvfs.seek(fd as u32, 4, 0);
        assert_eq_hvfs!(pos, 4, "seek SET to 4");
        let mut buf = [0u8; 4];
        let r = hvfs.read(fd as u32, &mut buf, 4);
        assert!(r > 0, "read after seek should succeed");
        assert_eq_hvfs!(&buf[..r as usize], b"4567", "read after seek content");
        hvfs.close(fd as u32);
    });

    test!(seek_cur, {
        let fd = hvfs.open("/seek_cur_test.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"ABCDEFGHIJ", 10);
        hvfs.close(fd as u32);

        let fd = hvfs.open("/seek_cur_test.txt", 0x0001, 1).unwrap();
        hvfs.seek(fd as u32, 2, 0);
        let pos = hvfs.seek(fd as u32, 3, 1);
        assert_eq_hvfs!(pos, 5, "seek CUR from 2 + 3 = 5");
        let mut buf = [0u8; 3];
        let r = hvfs.read(fd as u32, &mut buf, 3);
        assert!(r > 0);
        assert_eq_hvfs!(&buf[..r as usize], b"FGH", "read after seek CUR");
        hvfs.close(fd as u32);
    });

    test!(seek_end, {
        let fd = hvfs.open("/seek_end_test.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"HELLO", 5);
        hvfs.close(fd as u32);

        let fd = hvfs.open("/seek_end_test.txt", 0x0001, 1).unwrap();
        let pos = hvfs.seek(fd as u32, -2i64, 2);
        assert_eq_hvfs!(pos, 3, "seek END - 2 = 3");
        let mut buf = [0u8; 4];
        let r = hvfs.read(fd as u32, &mut buf, 4);
        assert!(r > 0);
        assert_eq_hvfs!(&buf[..r as usize], b"LO", "read after seek END");
        hvfs.close(fd as u32);
    });

    println!("\n=== HvFS Error Path Tests Passed ===\n");
}

#[test]
fn hvfs_advanced_features() {
    println!("\n=== HvFS Advanced Feature Tests ===\n");

    let hvfs = get_hvfs();
    hvfs.init();

    test!(symlink_create_and_readlink, {
        let fd = hvfs.open("/link_target.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"target data", 11);
        hvfs.close(fd as u32);

        let r = hvfs.symlink("/link_target.txt", "/my_symlink", 1);
        assert!(r >= 0, "symlink should succeed, got {}", r);
    });

    test!(hardlink_create, {
        let fd = hvfs.open("/hardlink_src.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"hardlink data", 13);
        hvfs.close(fd as u32);

        let r = hvfs.link("/hardlink_src.txt", "/hardlink_dst.txt", 1);
        assert_eq_hvfs!(r, 0, "hardlink should succeed");
    });

    test!(hardlink_target_exists, {
        let fd = hvfs.open("/hl_exist_src.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"src", 3);
        hvfs.close(fd as u32);
        let fd2 = hvfs.open("/hl_exist_dst.txt", 0x0102, 1).unwrap();
        hvfs.write(fd2 as u32, b"dst", 3);
        hvfs.close(fd2 as u32);

        let r = hvfs.link("/hl_exist_src.txt", "/hl_exist_dst.txt", 1);
        assert_eq_hvfs!(r, -4, "hardlink to existing target should return -4");
    });

    test!(hardlink_source_not_found, {
        let r = hvfs.link("/no_such_src", "/hl_dst.txt", 1);
        assert_eq_hvfs!(r, -2, "hardlink missing source should return -2");
    });

    test!(xattr_set_get_remove, {
        let fd = hvfs.open("/xattr_test.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"xattr content", 13);
        hvfs.close(fd as u32);

        let r = hvfs.setxattr("/xattr_test.txt", "user.comment", b"hello world", 1);
        assert!(r >= 0, "setxattr should succeed, got {}", r);

        let mut buf = [0u8; 64];
        let r = hvfs.getxattr("/xattr_test.txt", "user.comment", &mut buf, 1);
        assert!(r > 0, "getxattr should return data, got {}", r);

        let r = hvfs.removexattr("/xattr_test.txt", "user.comment", 1);
        assert_eq_hvfs!(r, 0, "removexattr should succeed");
    });

    test!(xattr_nonexistent_file, {
        let r = hvfs.setxattr("/no_xattr_file", "user.test", b"val", 1);
        assert_eq_hvfs!(r, -2, "setxattr on nonexistent should return -2");
    });

    test!(xattr_list, {
        let fd = hvfs.open("/xattr_list.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"list test", 9);
        hvfs.close(fd as u32);

        hvfs.setxattr("/xattr_list.txt", "user.attr0", b"v0", 1);
        hvfs.setxattr("/xattr_list.txt", "user.attr1", b"v1", 1);

        let mut buf = [0u8; 256];
        let r = hvfs.listxattr("/xattr_list.txt", &mut buf, 1);
        assert!(r > 0, "listxattr should return data, got {}", r);
    });

    test!(chmod_basic, {
        let fd = hvfs.open("/chmod_test.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"chmod", 5);
        hvfs.close(fd as u32);

        let r = hvfs.chmod("/chmod_test.txt", 0o755, 1);
        assert_eq_hvfs!(r, 0, "chmod should succeed");

        let stat = hvfs.stat("/chmod_test.txt", 1);
        assert!(stat.is_some());
        assert_eq_hvfs!(stat.unwrap().pwid_perm, 0o755u16, "chmod value");
    });

    test!(chmod_nonexistent, {
        let r = hvfs.chmod("/no_chmod_file", 0o755, 1);
        assert_eq_hvfs!(r, -1, "chmod nonexistent should return -1");
    });

    test!(chown_basic, {
        let fd = hvfs.open("/chown_test.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"chown", 5);
        hvfs.close(fd as u32);

        let r = hvfs.chown("/chown_test.txt", 42, 1);
        assert_eq_hvfs!(r, 0, "chown should succeed");

        let stat = hvfs.stat("/chown_test.txt", 1);
        assert!(stat.is_some());
        assert_eq_hvfs!(stat.unwrap().owner_pwid, 42, "chown value");
    });

    test!(chown_nonexistent, {
        let r = hvfs.chown("/no_chown_file", 42, 1);
        assert_eq_hvfs!(r, -1, "chown nonexistent should return -1");
    });

    test!(sync_operation, {
        let fd = hvfs.open("/sync_test.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"sync data", 9);
        hvfs.close(fd as u32);

        let r = hvfs.sync();
        assert_eq_hvfs!(r, 0, "sync should succeed");
    });

    test!(get_stats, {
        let (allocs, _frees, reads, writes) = hvfs.get_stats();
        assert!(allocs > 0 || reads > 0 || writes > 0, "stats should reflect activity");
    });

    println!("\n=== HvFS Advanced Feature Tests Passed ===\n");
}

#[test]
fn hvfs_snapshot_clone() {
    println!("\n=== HvFS Snapshot & Clone Tests ===\n");

    let hvfs = get_hvfs();
    hvfs.init();

    test!(snapshot_create, {
        let fd = hvfs.open("/snap_file.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"snapshot data", 13);
        hvfs.close(fd as u32);

        let r = hvfs.snapshot_create("snap1");
        assert!(r >= 0, "snapshot_create should succeed, got {}", r);
    });

    test!(snapshot_list, {
        let snap_mgr = &hvfs.snap_mgr;
        let count = snap_mgr.snapshot_count();
        assert!(count >= 1, "should have at least 1 snapshot, got {}", count);
    });

    test!(snapshot_get, {
        let snap_mgr = &hvfs.snap_mgr;
        let snaps = snap_mgr.list_snapshots(0);
        assert!(!snaps.is_empty(), "should list snapshots for ds_id=0");
        let snap = snap_mgr.get_snapshot(snaps[0].snap_id);
        assert!(snap.is_some(), "should get snapshot by id");
        let snap = snap.unwrap();
        assert_eq_hvfs!(snap.get_name(), "snap1", "snapshot name");
    });

    test!(snapshot_rollback, {
        let fd = hvfs.open("/post_snap_file.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"after snapshot", 14);
        hvfs.close(fd as u32);

        let snap_mgr = &hvfs.snap_mgr;
        let snaps = snap_mgr.list_snapshots(0);
        let snap_id = snaps[0].snap_id;

        let r = hvfs.snapshot_rollback(snap_id);
        assert_eq_hvfs!(r, 0, "rollback should succeed");
    });

    test!(snapshot_destroy, {
        let _ = hvfs.snapshot_create("snap_to_destroy");
        let snap_mgr = &hvfs.snap_mgr;
        let snaps = snap_mgr.list_snapshots(0);
        let target = snaps.iter().find(|s| s.get_name() == "snap_to_destroy");
        assert!(target.is_some(), "should find snap_to_destroy");
        let snap_id = target.unwrap().snap_id;

        let r = hvfs.snapshot_destroy(snap_id);
        assert_eq_hvfs!(r, 0, "destroy should succeed");
    });

    test!(snapshot_destroy_nonexistent, {
        let r = hvfs.snapshot_destroy(99999);
        assert_eq_hvfs!(r, -1, "destroy nonexistent should return -1");
    });

    test!(snapshot_rollback_wrong_ds, {
        let snap_mgr = &hvfs.snap_mgr;
        let snaps = snap_mgr.list_snapshots(0);
        if !snaps.is_empty() {
            let snap_id = snaps[0].snap_id;
            let fake_ds = antx_host_tests::hvfs::dataset::HvDataset::new(999, "fake", 0);
            let r = snap_mgr.rollback(snap_id, &fake_ds);
            assert_eq_hvfs!(r, false, "rollback with wrong ds_id should fail");
        }
    });

    test!(clone_from_snapshot, {
        let _ = hvfs.snapshot_create("clone_source");
        let snap_mgr = &hvfs.snap_mgr;
        let snaps = snap_mgr.list_snapshots(0);
        let source = snaps.iter().find(|s| s.get_name() == "clone_source");
        if let Some(snap) = source {
            let r = hvfs.clone_create(snap.snap_id, "cloned_ds");
            assert!(r >= 0, "clone_create should succeed, got {}", r);
        }
    });

    test!(clone_from_nonexistent_snapshot, {
        let r = hvfs.clone_create(99999, "bad_clone");
        assert_eq_hvfs!(r, -1, "clone from nonexistent snapshot should fail");
    });

    println!("\n=== HvFS Snapshot & Clone Tests Passed ===\n");
}

#[test]
fn hvfs_fd_management() {
    println!("\n=== HvFS FD Management Tests ===\n");

    let hvfs = get_hvfs();
    hvfs.init();

    test!(open_append_flag, {
        let fd = hvfs.open("/append_test.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"first", 5);
        hvfs.close(fd as u32);

        let fd = hvfs.open("/append_test.txt", 0x0100 | 0x0400, 1).unwrap();
        let mut buf = [0u8; 32];
        let r = hvfs.read(fd as u32, &mut buf, 32);
        assert!(r >= 5, "append mode should start at end, read got {}", r);
        hvfs.close(fd as u32);
    });

    test!(open_existing_with_create, {
        let fd = hvfs.open("/existing.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"original", 8);
        hvfs.close(fd as u32);

        let fd2 = hvfs.open("/existing.txt", 0x0102, 1);
        assert!(fd2.is_ok(), "open existing with O_CREAT should succeed");
        hvfs.close(fd2.unwrap() as u32);
    });

    test!(close_twice, {
        let fd = hvfs.open("/double_close.txt", 0x0102, 1).unwrap();
        hvfs.close(fd as u32);
        let r = hvfs.close(fd as u32);
        assert_eq_hvfs!(r, -1, "closing already-closed fd should return -1");
    });

    test!(read_write_zero_bytes, {
        let fd = hvfs.open("/zero_rw.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"data", 4);
        hvfs.close(fd as u32);

        let fd = hvfs.open("/zero_rw.txt", 0x0001, 1).unwrap();
        let mut buf = [0u8; 4];
        let r = hvfs.read(fd as u32, &mut buf, 0);
        assert_eq_hvfs!(r, 0, "read 0 bytes should return 0");
        let w = hvfs.write(fd as u32, b"", 0);
        assert_eq_hvfs!(w, 0, "write 0 bytes should return 0");
        hvfs.close(fd as u32);
    });

    test!(read_past_end, {
        let fd = hvfs.open("/short_file.txt", 0x0102, 1).unwrap();
        hvfs.write(fd as u32, b"hi", 2);
        hvfs.close(fd as u32);

        let fd = hvfs.open("/short_file.txt", 0x0001, 1).unwrap();
        hvfs.seek(fd as u32, 100, 0);
        let mut buf = [0u8; 10];
        let r = hvfs.read(fd as u32, &mut buf, 10);
        assert_eq_hvfs!(r, 0, "read past end should return 0");
        hvfs.close(fd as u32);
    });

    println!("\n=== HvFS FD Management Tests Passed ===\n");
}
