use antx_host_tests::hvfs::hvfs::get_hvfs;

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
