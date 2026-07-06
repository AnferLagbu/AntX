//! ext2 只读文件系统测试

use std::fs;
use std::path::Path;

#[test]
fn test_ext2_image_exists() {
    assert!(Path::new("ext2_test.img").exists(), "ext2 测试镜像不存在");
}

#[test]
fn test_ext2_superblock_magic() {
    // 读取超级块并验证 magic number
    let data = fs::read("ext2_test.img").unwrap();
    assert!(data.len() >= 1024 + 1024, "镜像太小");

    let magic = u16::from_le_bytes([data[1024 + 56], data[1024 + 57]]);
    assert_eq!(magic, 0xEF53, "ext2 magic number 不匹配");
}

#[test]
fn test_ext2_block_size() {
    let data = fs::read("ext2_test.img").unwrap();
    let log_block_size = u32::from_le_bytes([
        data[1024 + 24],
        data[1024 + 25],
        data[1024 + 26],
        data[1024 + 27],
    ]);
    let block_size = 1024u32 << log_block_size;
    assert!(block_size >= 1024 && block_size <= 65536, "块大小无效");
}

#[test]
fn test_ext2_inode_count() {
    let data = fs::read("ext2_test.img").unwrap();
    let inode_count = u32::from_le_bytes([
        data[1024 + 0],
        data[1024 + 1],
        data[1024 + 2],
        data[1024 + 3],
    ]);
    assert!(inode_count > 0, "inode 数量为 0");
}

#[test]
fn test_ext2_block_count() {
    let data = fs::read("ext2_test.img").unwrap();
    let block_count = u32::from_le_bytes([
        data[1024 + 4],
        data[1024 + 5],
        data[1024 + 6],
        data[1024 + 7],
    ]);
    assert!(block_count > 0, "块数量为 0");
}