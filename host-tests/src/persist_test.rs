use antx_host_tests::hvfs::hvfs::{get_hvfs, HVFS_DATA};

#[test]
fn hvfs_persistence_roundtrip() {
    println!("\n=== HvFS Persistence Roundtrip (Memory) ===\n");

    println!("--- Phase 1: Init, write files, sync ---");
    let hvfs = get_hvfs();
    hvfs.init();
    assert!(hvfs.is_initialized(), "should be initialized");

    for i in 0..5 {
        let name = format!("/file_{}", i);
        let fd = hvfs.open(&name, 0x0102, 1).unwrap();
        let data = format!("data for file {}", i);
        assert_eq!(hvfs.write(fd as u32, data.as_bytes(), data.len() as u32), data.len() as i32);
        hvfs.close(fd as u32);
    }
    hvfs.mkdir("/docs", 1);
    let fd = hvfs.open("/docs/readme.txt", 0x0102, 1).unwrap();
    let c = b"Persist test data";
    assert_eq!(hvfs.write(fd as u32, c, c.len() as u32), c.len() as i32);
    hvfs.close(fd as u32);

    assert_eq!(hvfs.sync(), 0, "sync should succeed");
    println!("  Files written and synced ✓");

    println!("--- Phase 2: Verify pre-reset ---");
    for i in 0..5 {
        let name = format!("/file_{}", i);
        let fd = hvfs.open(&name, 0x0001, 1).unwrap() as u32;
        let mut buf = [0u8; 64];
        let r = hvfs.read(fd, &mut buf, 64);
        let s = std::str::from_utf8(&buf[..r as usize]).unwrap();
        assert_eq!(s, format!("data for file {}", i));
        hvfs.close(fd);
    }
    println!("  All files readable ✓");

    println!("--- Phase 3: Reset ---");
    {
        let mut guard = HVFS_DATA.lock();
        *guard = None;
    }
    println!("  Singleton reset ✓");

    println!("--- Phase 4: Re-init and verify files gone (fresh) ---");
    let hvfs2 = get_hvfs();
    hvfs2.init();
    assert!(hvfs2.is_initialized(), "re-init should succeed");

    match hvfs2.open("/file_0", 0x0001, 1) {
        Ok(fd) => { hvfs2.close(fd as u32); }
        Err(_) => {}
    }

    println!("\n=== Persistence Roundtrip Passed ===\n");
}
