---
feature: ext2-readonly
status: delivered
specs: []
plans:
  - docs/compose/plans/2026-07-06-ext2-readonly.md
branch: main
commits: ee048e0..ee048e0
---

# ext2 只读文件系统 — 最终报告

## What Was Built

实现了 ext2 文件系统的只读支持，允许 QueenX 内核挂载和读取 Linux ext2 格式的磁盘分区。该实现提供了完整的 VFS 接口集成，支持文件系统的挂载、目录列表、文件读取和元数据查询操作。

ext2 是 Linux 互操作的基础，该实现使 QueenX 能够访问现有的 ext2 磁盘分区，为未来的 Linux ABI 兼容性奠定基础。

## Architecture

### 模块结构

```
src/kernel/services/fs/ext2/
├── mod.rs           # 模块入口
├── super_block.rs   # ext2 超级块数据结构
├── block_group.rs   # 块组描述符数据结构
├── inode.rs         # inode 数据结构
├── dir.rs           # 目录项数据结构
├── bitmap.rs        # 位图操作 (只读)
├── read.rs          # 读取核心逻辑
└── mount.rs         # FileSystem trait 实现
```

### 数据结构

- **Ext2SuperBlock**: 1024 字节超级块，包含文件系统元数据（块大小、inode 数量、块组信息等）
- **Ext2BlockGroupDescriptor**: 32 字节块组描述符，描述每个块组的位图和 inode 表位置
- **Ext2Inode**: 128 字节 inode，包含文件元数据和 15 个块指针（12 直接 + 1 间接 + 1 二次间接 + 1 三次间接）
- **Ext2DirEntry**: 变长目录项，包含 inode 号、文件类型和文件名

### 核心组件

- **Ext2Fs**: 文件系统实例，管理超级块、块组描述符表和 inode 缓存
- **Ext2FileSystem**: FileSystem trait 实现，集成到 VFS 挂载系统

### 数据流

```
用户态 → VFS → Ext2FileSystem → Ext2Fs → 块设备层 → 磁盘
```

### 设计决策

1. **只读实现**: 首次实现仅支持读取，降低复杂度，确保稳定性
2. **全局单例**: 使用 `Mutex<Option<Ext2Fs>>` 管理文件系统实例，简化状态管理
3. **手动解析**: 避免 unsafe 代码，使用 `u32::from_le_bytes` 手动解析磁盘结构
4. **inode 缓存**: 简单的 Vec 缓存，避免重复读取相同的 inode

## Usage

### 挂载 ext2 文件系统

```rust
// 在 QueenX 内核中
mount -t ext2 /dev/sda /mnt
```

### 使用示例

```rust
// 列出目录
ls /mnt

// 读取文件
cat /mnt/hello.txt

// 获取文件信息
stat /mnt/hello.txt
```

### VFS 集成

ext2 已集成到 VFS 挂载系统：
- `FsType::Ext2` 变体已添加到 `FsType` 枚举
- `vfs_mount_internal` 已添加 ext2 处理分支
- 所有写入操作返回 `KernelError::ReadOnly`

## Verification

### 测试覆盖

- **host-tests**: 5 个测试用例覆盖超级块解析、块大小验证、inode 和块数量检查
- **测试镜像**: 1MB ext2 测试镜像，包含测试文件和目录

### 测试结果

```
running 5 tests
test test_ext2_image_exists ... ok
test test_ext2_superblock_magic ... ok
test test_ext2_inode_count ... ok
test test_ext2_block_size ... ok
test test_ext2_block_count ... ok

test result: ok. 5 passed; 0 failed; 0 ignored
```

### 编译验证

- x86_64 编译通过
- aarch64 编译通过
- Clippy 检查通过（0 warnings）

## Journey Log

- [lesson] services 层禁止 unsafe，需要使用手动解析替代 `core::ptr::read_unaligned`
- [lesson] PiMutex 结构体缺少字段导致编译错误，需要补全 `protocol` 和 `ceiling` 字段
- [lesson] match 语句需要覆盖所有 FsType 变体，新增枚举变体时需同步更新所有 match

## Source Materials

| File | Role | Notes |
|------|------|-------|
| `docs/compose/plans/2026-07-06-ext2-readonly.md` | 实施计划 | 12 个任务，全部完成 |
| `docs/plan/ext2-implementation.md` | 工程文档 | Phase 1 只读状态已标记 [X] |