# ext2 文件系统实现工程文档

> 附属文档: [naming-implementation.md](naming-implementation.md) §六 Linux 兼容
> 基于 2026-07-05 源码调研, 规划 QueenX ext2 文件系统实现路径.

## 背景

QueenX 当前有 5 种文件系统 (ramfs/hvfs/devfs/procfs/initramfs), 缺少传统磁盘文件系统. ext2 是 Linux 互操作的基础, 也是 linuxulator 路线 B 的关键组件.

### 现有 VFS 接口

QueenX VFS 采用 **FsType 枚举 + 分发** 模式:

```rust
// services/fs/vfs_types.rs
pub enum FsType {
    RamFs,
    HvFs,
    DevFs,
    Unknown,
}
```

挂载流程: `vfs_mount_internal` → `FsType::from_name` → 分发到对应 FS 的 `init()`.

### 参考实现

Asterinas ext2: 10K 行, 23 个文件, 完整实现 (super_block/inode/block_group/dir/xattr).

### 目标

- 实现 ext2 文件系统核心 (只读优先, 读写后续)
- 集成到 QueenX VFS
- 支持 mount -t ext2 /dev/sdX /mnt
- host-tests 验证

## 架构设计

### 模块结构

```text
services/fs/ext2/
├── mod.rs              模块入口 + FsType 注册
├── super_block.rs      超级块解析 (磁盘布局 + 内存表示)
├── block_group.rs      块组描述符
├── inode.rs            inode 操作 (读/写/属性)
├── dir.rs              目录项操作 (lookup/readdir)
├── bitmap.rs           位图管理 (inode/block 分配)
├── mount.rs            挂载/卸载逻辑
└── read.rs             数据读取 (直接/间接/双重间接块)
```

### 核心数据结构

```rust
// super_block.rs — 磁盘布局 (1024 字节偏移)
#[repr(C)]
pub struct RawSuperBlock {
    pub s_inodes_count: u32,
    pub s_blocks_count: u32,
    pub s_r_blocks_count: u32,
    pub s_free_blocks_count: u32,
    pub s_free_inodes_count: u32,
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,  // log2(block_size) - 10
    pub s_blocks_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_magic: u16,           // 0xEF53
    // ...
}

// inode.rs — 磁盘布局 (128 字节)
#[repr(C)]
pub struct RawInode {
    pub i_mode: u16,
    pub i_uid: u16,
    pub i_size: u32,
    pub i_atime: u32,
    pub i_ctime: u32,
    pub i_mtime: u32,
    pub i_dtime: u32,
    pub i_gid: u16,
    pub i_links_count: u16,
    pub i_blocks: u32,
    pub i_flags: u32,
    pub i_block: [u32; 15],  // 12 direct + 1 indirect + 1 double + 1 triple
    // ...
}
```

### 与 QueenX VFS 集成

1. `FsType` 枚举新增 `Ext2` 变体
2. `FsType::from_name("ext2")` 返回 `FsType::Ext2`
3. `vfs_mount_internal` 添加 `FsType::Ext2` 分发分支
4. 通过块设备层 `block::read_sectors` / `block::write_sectors` 访问磁盘

## 实施步骤

### Phase 1: 只读 ext2 (2 周)

| 步骤 | 内容 | 行数 |
|------|------|------|
| 1 | FsType 枚举新增 Ext2 | ~10 |
| 2 | super_block.rs: 解析超级块 | ~200 |
| 3 | block_group.rs: 块组描述符 | ~100 |
| 4 | inode.rs: inode 读取 + 属性 | ~300 |
| 5 | dir.rs: 目录项 lookup/readdir | ~200 |
| 6 | bitmap.rs: inode/block 位图 | ~150 |
| 7 | read.rs: 数据块读取 (直接/间接) | ~200 |
| 8 | mount.rs: 挂载逻辑 | ~100 |
| 9 | host-tests: 5+ 测试 | ~200 |

**验收**: mount -t ext2 /dev/sda /mnt → ls/cat/read 正确.

### Phase 2: 读写 ext2 (2 周)

| 步骤 | 内容 | 行数 |
|------|------|------|
| 1 | inode 写入 + 分配 | ~300 |
| 2 | 目录项创建/删除 | ~200 |
| 3 | 块分配/释放 | ~200 |
| 4 | 超级块更新 | ~100 |
| 5 | fsync/sync | ~100 |
| 6 | host-tests: 10+ 测试 | ~300 |

**验收**: touch/mkdir/rm/write → mount 后数据持久化.

## 依赖

- 块设备层: `framework::driver::block` (read_sectors/write_sectors)
- VFS: `services::fs::vfs_types` (FsType 枚举)
- 内存: `framework::mm` (页分配)

## 参考

- Asterinas ext2: `tmp/asterinas-main/kernel/src/fs/fs_impls/ext2/` (10K 行)
- Linux ext2 spec: `ext2.doc`
- QueenX ramfs: `services/fs/ramfs_core.rs` (参考 VFS 集成模式)
