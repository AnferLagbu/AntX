# HvFS 磁盘持久化文件系统

> **最后更新**: 2026-05-06 | **实现状态**: ✅ 已完成基础功能

## 一、概述

HvFS (Hive File System) 是 AntX 的原生文件系统。

**当前能力**:
- ✅ 内存文件系统 (完整可用)
- ✅ 磁盘持久化 (ATA PIO)
- ✅ 自动格式化/挂载 (通过 Smart Mount)
- ✅ Sync 机制 (脏页追踪+写回)

## 二、磁盘布局

```
┌─────────────────────────────────────────────────────────┐
│                 AntX 磁盘镜像 (最小 1MB)              │
├─────────────────────────────────────────────────────────┤
│ 扇区 0-1    │ 引导区 (MBR)                    │ 1KB │
│ 扇区 2-9    │ 超级块                          │ 4KB │
│ 扇区 10-137 │ Inode 表 (128个)               │ 64KB │
│ 扇区 138-152│ Block Bitmap                   │ 7.5KB│
│ 扇区 153-168│ Inode Bitmap                  │ 8KB  │
│ 扇区 169+   │ 数据区                         │ ~500KB│
└─────────────────────────────────────────────────────────┘
```

## 三、关键数据结构

### Super Block
```rust
struct HvFsSuperBlock {
    magic: u32,           // HVFS_MAGIC
    version: u32,         // HVFS_VERSION
    total_inodes: u32,    // 最大inode数 (128)
    total_blocks: u32,    // 最大数据块数 (1024)
    used_inodes: u32,
    used_blocks: u32,
    // ... 其他元数据
}
```

### Inode
```rust
struct HvInode {
    inode_num: u32,
    file_type: u8,        // FT_FILE / FT_DIR
    size: u64,
    pwid: u64,            // 所有者标识
    atime: u64, mtime: u64, ctime: u64,
    permissions: u8,
    block_pointers: [u32; 8],  // 直接+间接块指针
}
```

## 四、持久化机制

### 4.1 写入流程
```
hvfs_write(fd, data, count)
    ↓
修改 block_cache (标记 dirty)
    ↓ (调用 sync 时)
write_block_to_disk()  → ata_write_sector() × 8
write_inode_table()   → 写入 inode 区域
write_bitmaps()       → 写入位图区域
write_superblock()    → 写入超级块
```

### 4.2 读取流程
```
hvfs_open(path, flags, pwid)
    ↓
resolve_path()        → 解析路径获取 inode_num
read_inode_from_cache_or_disk()  → 加载 inode 元数据
返回 FileHandle
```

### 4.3 Sync 触发条件
- 显式调用 `hvfs_sync()`
- Smart Mount 在特定模式下自动 sync
- 定时器触发 (未来实现)

## 五、与 Smart Mount 集成

**位置**: `src/kernel/smart_mount.c` 调用 HvFS API

```c
int detect_persistent_storage(void) {
    int status = hvfs_check_disk();  // 检测 ATA 磁盘
    
    switch (status) {
        case HVFS_DISK_OK:       return 1;  // 已格式化
        case HVFS_DISK_UNFORMATTED: return 1;  // 需格式化
        case HVFS_DISK_NO_DISK:   return 0;  // 无磁盘
        default: return -1;
    }
}
```

## 六、FFI 导出函数 (Rust→C)

**文件**: `src/fs/vfs/ffi.rs`

```rust
#[no_mangle]
pub extern "C" fn hvfs_init() { ... }

#[no_mangle]
pub extern "C" fn hvfs_format() -> i32 { ... }

#[no_mangle]
pub extern "C" fn hvfs_check_disk() -> i32 { ... }  // [v2.0 新增]

#[no_mangle]
pub extern "C" fn hvfs_mount() -> i32 { ... }

#[no_mangle]
pub extern "C" fn hvfs_open(...) -> i32 { ... }

// ... 其他文件操作函数
```

## 七、已知限制

1. **单分区支持**: 当前只支持一个 HvFS 分区
2. **无日志**: 未实现 WAL/journaling (崩溃恢复靠 fsck)
3. **无碎片整理**: 删除后空间不回收 (需手动 format)
4. **性能**: PIO 模式，非 DMA (适合教学场景)

## 八、测试验证

```bash
# 编译
make all

# QEMU 测试 (15秒超时)
timeout 15 qemu-system-x86_64 \
    -m 256 -drive file=build/antx.img,format=raw \
    -serial stdio -display none -no-reboot

# 检查日志中的 [HVFS] 和 [SMART] 标记
```

---

**相关文档**: 
- [smart-persistent-storage.md](./smart-persistent-storage.md) - Smart Mount 设计
- [kernel-architecture.md](./kernel-architecture.md) - 整体架构
- [ai-autonomous-development-spec.md](../ai-autonomous-development-spec.md) - 开发规范
