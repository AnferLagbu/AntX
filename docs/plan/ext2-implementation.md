# ext2 文件系统实现

> QueenX 缺少传统磁盘文件系统. ext2 是 Linux 互操作的基础, 也是直接 ABI 兼容的关键组件.

## 工程计划: ext2 只读实现

### 背景
- **ext2 缺失导致无法挂载 Linux 磁盘**
  - 描述: QueenX 当前有 ramfs/hvfs/devfs/procfs/initramfs 5 种 FS, 无传统磁盘 FS
  - 方案: 实现 ext2 只读支持, 支持 mount -t ext2 /dev/sdX /mnt
  - 状态: []

- **VFS 接口已就绪**
  - 描述: FsType 枚举 + mount 分发模式可直接扩展
  - 方案: 新增 FsType::Ext2 变体, 在 vfs_mount_internal 添加分发
  - 状态: []

- **块设备层已就绪**
  - 描述: framework::driver::block 提供 read_sectors/write_sectors
  - 方案: ext2 通过块设备层访问磁盘, 无需直接操作硬件
  - 状态: []

### 目标
- **只读 ext2**
  - 描述: mount + ls + cat + read 正确
  - 方案: 解析 super_block/inode/block_group, 实现数据块读取
  - 状态: []

- **host-tests 验证**
  - 描述: 5+ 测试覆盖挂载/读取/目录
  - 方案: 在 host-tests/ 中创建 ext2 测试镜像 + 测试用例
  - 状态: []

### 方案
- **模块结构**
  - 描述: 8 个模块组织
  - 方案: services/fs/ext2/ 下: mod.rs + super_block.rs + block_group.rs + inode.rs + dir.rs + bitmap.rs + read.rs + mount.rs
  - 状态: []

- **数据结构**
  - 描述: ext2 磁盘布局 (super_block 1024字节偏移, inode 128字节, block_group 32字节)
  - 方案: 参考 Asterinas ext2 super_block.rs/inode/mod.rs, 适配 QueenX 块设备层
  - 状态: []

- **参考实现**
  - 描述: Asterinas ext2 10K 行, 23 个文件
  - 方案: 借鉴数据结构设计, 用 QueenX VFS 接口重新实现, 约 2K 行
  - 状态: []

### 工作量
- **Phase 1 只读**
  - 描述: 预计 2 周
  - 方案: super_block + inode + dir + bitmap + read + mount + tests
  - 状态: []

## 工程计划: ext2 读写实现

### 背景
- **只读完成后需支持写入**
  - 描述: touch/mkdir/rm/write 需要 ext2 写入能力
  - 方案: 在只读基础上添加 inode 分配/释放、块分配/释放、超级块更新
  - 状态: []

### 目标
- **读写 ext2**
  - 描述: mount + write + sync → 重新 mount 数据持久化
  - 方案: 实现 ext2 写入路径
  - 状态: []

- **host-tests 验证**
  - 描述: 10+ 测试覆盖写入/删除/同步
  - 方案: 验证数据持久化正确性
  - 状态: []

### 工作量
- **Phase 2 读写**
  - 描述: 预计 2 周
  - 方案: inode 写入 + 目录项创建删除 + 块分配释放 + fsync
  - 状态: []
