# HvFS 磁盘化文件系统设计文档

## 概述

HvFS (Hive File System) 是 AntX 操作系统的原生文件系统。本文档描述将 HvFS 从内存文件系统转换为磁盘持久化存储的设计方案。

## 设计目标

1. **数据持久化** - 系统重启后数据不丢失
2. **兼容现有接口** - 保持现有 API 不变
3. **简单可靠** - 适合嵌入式/教学操作系统
4. **PWID 权限集成** - 文件权限与 PWID 系统绑定

## 磁盘镜像布局

### 整体结构

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         AntX 磁盘镜像 (最小 1MB)                          │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  扇区 0-1      │ 引导扇区 (MBR + Stage2)           │ 1 KB              │
│  扇区 2-9      │ HvFS 超级块                        │ 4 KB              │
│  扇区 10-137   │ Inode 表 (128 个 inode)            │ 64 KB             │
│  扇区 138-152  │ 块位图 (管理 1024 个块)            │ 7.5 KB            │
│  扇区 153-168  │ Inode 位图 (管理 128 个 inode)     │ 8 KB              │
│  扇区 169-170  │ 日志区域 (可选)                    │ 1 KB              │
│  扇区 171+     │ 数据区 (文件内容)                  │ 剩余空间           │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 扇区分配表

| 区域 | 起始扇区 | 扇区数 | 大小 | 描述 |
|------|----------|--------|------|------|
| 引导区 | 0 | 2 | 1 KB | MBR + Stage2 引导代码 |
| 超级块 | 2 | 8 | 4 KB | 文件系统元数据 |
| Inode 表 | 10 | 128 | 64 KB | 128 个 inode (每个 512B) |
| 块位图 | 138 | 15 | 7.5 KB | 管理 1024+ 数据块 |
| Inode 位图 | 153 | 16 | 8 KB | 管理 128 个 inode |
| 日志区域 | 169 | 2 | 1 KB | 日志/恢复区域 (可选) |
| 数据区 | 171 | 变化 | ~500KB | 文件/目录数据 |

### 磁盘常量定义

```c
// hvfs_disk.h

#define HVFS_DISK_SECTOR_SIZE       512
#define HVFS_DISK_SECTORS_PER_BLOCK 1       // 1 扇区 = 1 块

#define HVFS_BOOT_SECTOR_START      0
#define HVFS_BOOT_SECTOR_COUNT      2

#define HVFS_SUPER_SECTOR_START     2
#define HVFS_SUPER_SECTOR_COUNT     8

#define HVFS_INODE_SECTOR_START     10
#define HVFS_INODE_SECTOR_COUNT     128
#define HVFS_INODES_PER_SECTOR      1       // 每个 inode 512B

#define HVFS_BLOCK_BITMAP_START     138
#define HVFS_BLOCK_BITMAP_COUNT     15

#define HVFS_INODE_BITMAP_START     153
#define HVFS_INODE_BITMAP_COUNT     16

#define HVFS_LOG_SECTOR_START       169
#define HVFS_LOG_SECTOR_COUNT       2

#define HVFS_DATA_SECTOR_START      171
```

## 数据结构设计

### 超级块 (Super Block)

```c
struct hvfs_super_block_disk {
    uint32_t magic;                    // 0x48564653 ("HVFS")
    uint32_t version;                  // 文件系统版本
    uint32_t block_size;               // 块大小 (512)
    uint32_t total_blocks;             // 总块数
    uint32_t free_blocks;              // 空闲块数
    uint32_t inode_count;              // inode 总数
    uint32_t free_inodes;              // 空闲 inode 数
    uint32_t first_data_block;         // 第一个数据块号
    
    uint32_t root_inode;               // 根目录 inode 号
    uint32_t block_bitmap_block;       // 块位图起始块
    uint32_t inode_bitmap_block;       // inode 位图起始块
    uint32_t inode_table_block;        // inode 表起始块
    
    uint64_t created_time;             // 创建时间
    uint64_t modified_time;            // 最后修改时间
    uint64_t mount_time;               // 最后挂载时间
    uint32_t mount_count;              // 挂载次数
    
    uint32_t state;                    // 文件系统状态
    uint32_t checksum;                 // 校验和
    
    uint8_t  reserved[440];            // 保留，填充到 512 字节
} __attribute__((packed));
```

### Inode (索引节点)

```c
struct hvfs_inode_disk {
    uint32_t inode_num;                // inode 编号
    uint16_t mode;                     // 文件类型和权限
    uint16_t reserved;                 // 保留
    
    uint32_t size;                     // 文件大小
    uint32_t blocks;                   // 占用块数
    
    uint64_t atime;                    // 访问时间
    uint64_t mtime;                    // 修改时间
    uint64_t ctime;                    // 创建时间
    
    uint64_t owner_pwid;               // 所有者 PWID
    uint64_t group_pwid;               // 组 PWID
    uint16_t pwid_perm;                // PWID 权限位
    
    uint32_t link_count;               // 硬链接数
    
    uint32_t direct_blocks[12];        // 直接块指针
    uint32_t indirect_block;           // 一级间接块
    uint32_t double_indirect;          // 二级间接块
    
    uint8_t  flags;                    // 状态标志
    uint8_t  reserved2[23];            // 保留，填充到 512 字节
} __attribute__((packed));
```

### 目录项 (Directory Entry)

```c
struct hvfs_dir_entry_disk {
    uint32_t inode;                    // inode 编号
    uint16_t rec_len;                  // 记录长度
    uint8_t  name_len;                 // 文件名长度
    uint8_t  file_type;                // 文件类型
    char     name[HVFS_MAX_NAME];      // 文件名 (64 字节)
    uint8_t  reserved[52];             // 保留，填充到 128 字节
} __attribute__((packed));

#define HVFS_DIR_ENTRY_SIZE  128
```

## 块分配策略

### 块寻址

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         HvFS 块寻址方案                                  │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  直接块 (12 个)                                                          │
│  ├── block[0]  - block[11]  : 0 - 5.5 KB (小文件直接存储)               │
│                                                                         │
│  一级间接块                                                               │
│  ├── indirect_block         : 指向一个包含 128 个块指针的块              │
│  │                            可寻址 64 KB                              │
│                                                                         │
│  二级间接块                                                               │
│  └── double_indirect        : 指向一个包含 128 个一级间接块的块          │
│                               可寻址 8 MB                               │
│                                                                         │
│  最大文件大小: 6 KB + 64 KB + 8 MB ≈ 8.1 MB                             │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 块位图

```c
// 位图操作
#define BLOCK_IS_FREE(bitmap, block)  !((bitmap)[(block) / 8] & (1 << ((block) % 8)))
#define BLOCK_SET_USED(bitmap, block) ((bitmap)[(block) / 8] |= (1 << ((block) % 8)))
#define BLOCK_SET_FREE(bitmap, block) ((bitmap)[(block) / 8] &= ~(1 << ((block) % 8)))
```

## ATA/IDE 驱动设计

### 端口定义

```c
// ata.h

#define ATA_PRIMARY_IO        0x1F0
#define ATA_PRIMARY_CTRL      0x3F6
#define ATA_SECONDARY_IO      0x170
#define ATA_SECONDARY_CTRL    0x376

#define ATA_DATA              0x00
#define ATA_ERROR             0x01
#define ATA_FEATURES          0x01
#define ATA_SECTOR_COUNT      0x02
#define ATA_SECTOR_NUM        0x03
#define ATA_CYLINDER_LOW      0x04
#define ATA_CYLINDER_HIGH     0x05
#define ATA_DRIVE_HEAD        0x06
#define ATA_STATUS            0x07
#define ATA_COMMAND           0x07

#define ATA_CMD_READ_SECTORS  0x20
#define ATA_CMD_WRITE_SECTORS 0x30
#define ATA_CMD_IDENTIFY      0xEC

#define ATA_STATUS_BSY        0x80
#define ATA_STATUS_DRDY       0x40
#define ATA_STATUS_DRQ        0x08
#define ATA_STATUS_ERR        0x01
```

### 驱动接口

```c
// ata.h

void ata_init(void);

int ata_read_sector(uint8_t drive, uint32_t lba, void *buffer);
int ata_write_sector(uint8_t drive, uint32_t lba, const void *buffer);

int ata_read_sectors(uint8_t drive, uint32_t lba, uint32_t count, void *buffer);
int ata_write_sectors(uint8_t drive, uint32_t lba, uint32_t count, const void *buffer);

int ata_identify(uint8_t drive, uint16_t *identify_data);
```

### 读取扇区实现

```c
int ata_read_sector(uint8_t drive, uint32_t lba, void *buffer) {
    uint16_t io = drive ? ATA_SECONDARY_IO : ATA_PRIMARY_IO;
    
    // 等待驱动器就绪
    while (inb(io + ATA_STATUS) & ATA_STATUS_BSY);
    
    // 设置 LBA 模式
    outb(io + ATA_DRIVE_HEAD, 0xE0 | ((lba >> 24) & 0x0F));
    outb(io + ATA_SECTOR_COUNT, 1);
    outb(io + ATA_SECTOR_NUM, lba & 0xFF);
    outb(io + ATA_CYLINDER_LOW, (lba >> 8) & 0xFF);
    outb(io + ATA_CYLINDER_HIGH, (lba >> 16) & 0xFF);
    
    // 发送读取命令
    outb(io + ATA_COMMAND, ATA_CMD_READ_SECTORS);
    
    // 等待数据就绪
    while (!(inb(io + ATA_STATUS) & ATA_STATUS_DRQ));
    
    // 读取数据
    for (int i = 0; i < 256; i++) {
        ((uint16_t*)buffer)[i] = inw(io + ATA_DATA);
    }
    
    return 0;
}
```

## HvFS 磁盘操作接口

### 新增函数

```c
// hvfs.h 新增

int hvfs_disk_init(void);
int hvfs_mount(const char *device);
int hvfs_unmount(void);

int hvfs_sync(void);
int hvfs_sync_inode(struct inode *inode);
int hvfs_sync_super(void);

int hvfs_load_super(void);
int hvfs_load_inode_table(void);
int hvfs_load_block_bitmap(void);

struct inode* hvfs_load_inode(uint32_t inode_num);
int hvfs_save_inode(struct inode *inode);

uint32_t hvfs_alloc_block(void);
void hvfs_free_block(uint32_t block_num);
uint32_t hvfs_alloc_inode(void);
void hvfs_free_inode(uint32_t inode_num);
```

### 同步到磁盘

```c
int hvfs_sync(void) {
    if (!hvfs_disk_initialized) {
        return -1;
    }
    
    // 1. 同步超级块
    if (hvfs_sync_super() != 0) {
        return -1;
    }
    
    // 2. 同步所有已使用的 inode
    for (int i = 1; i < HVFS_MAX_INODES; i++) {
        if (hvfs_inode_table[i].used) {
            hvfs_sync_inode(&hvfs_inode_table[i]);
        }
    }
    
    // 3. 同步块位图
    ata_write_sectors(0, HVFS_BLOCK_BITMAP_START, 
                      HVFS_BLOCK_BITMAP_COUNT, hvfs_block_bitmap);
    
    // 4. 同步数据块
    for (int i = 0; i < HVFS_MAX_BLOCKS; i++) {
        if (!BLOCK_IS_FREE(hvfs_block_bitmap, i)) {
            ata_write_sector(0, HVFS_DATA_SECTOR_START + i, 
                            get_block(i));
        }
    }
    
    return 0;
}
```

### 从磁盘加载

```c
int hvfs_mount(const char *device) {
    (void)device;
    
    // 1. 读取超级块
    struct hvfs_super_block_disk super_disk;
    ata_read_sectors(0, HVFS_SUPER_SECTOR_START, 
                     HVFS_SUPER_SECTOR_COUNT, &super_disk);
    
    // 2. 验证魔数
    if (super_disk.magic != HVFS_MAGIC) {
        serial_puts(SERIAL_COM1, "HvFS: Invalid magic number\n");
        return -1;
    }
    
    // 3. 复制超级块信息
    hvfs_super.magic = super_disk.magic;
    hvfs_super.block_size = super_disk.block_size;
    hvfs_super.total_blocks = super_disk.total_blocks;
    // ... 其他字段
    
    // 4. 加载 inode 表
    ata_read_sectors(0, HVFS_INODE_SECTOR_START,
                     HVFS_INODE_SECTOR_COUNT, hvfs_inode_table);
    
    // 5. 加载块位图
    ata_read_sectors(0, HVFS_BLOCK_BITMAP_START,
                     HVFS_BLOCK_BITMAP_COUNT, hvfs_block_bitmap);
    
    // 6. 加载数据区到内存
    // (可选，可以按需加载)
    
    hvfs_disk_initialized = 1;
    return 0;
}
```

## 启动流程集成

### 当前启动流程 (2026-04-19 更新)

```
kernel_main()
    │
    ├── ata_init()           // 初始化 ATA 驱动
    │
    ├── vfs_init()           // 初始化 VFS 层
    │   ├── ramfs_init()     // 注册 RamFS
    │   ├── diskfs_init()    // 注册 DiskFS
    │   ├── devfs_init()     // 注册 DevFS
    │   └── procfs_init()    // 注册 ProcFS
    │
    ├── VFS 挂载
    │   ├── 尝试: vfs_mount("/", "diskfs")
    │   │   ├── 成功 → 使用磁盘文件系统 (持久化)
    │   │   └── 失败 → 回退 RamFS (内存, 关机丢失)
    │   └── ...
    │
    ├── 检测安装状态
    │   ├── 存在 /.antx_installed → 跳过安装向导
    │   └── 不存在 → 运行 install_guide_run()
    │       ├── Step 1: 设置 root 密码 (pwid_create_original_root)
    │       ├── Step 2: 配置主机名 (/etc/hostname)
    │       └── Step 3: 完成安装 (sys_fs_sync + 创建标记)
    │
    └── 进入 Shell (antxsh, Ring 3 用户态)
```

### 磁盘挂载逻辑 ([diskfs.c](file:///home/anfer/Code/C/AntX/src/fs/diskfs.c))

```c
static int diskfs_mount(const char *path) {
    int status = hvfs_check_disk();
    
    switch (status) {
        case HVFS_DISK_OK:
            // 找到有效的 HvFS 文件系统，直接挂载
            hvfs_mount();
            break;
            
        case HVFS_DISK_NO_DISK:
            // 无磁盘设备，返回错误（上层回退到 RamFS）
            return -1;
            
        case HVFS_DISK_UNFORMATTED:
            // 磁盘未格式化，自动格式化并初始化
            hvfs_format_disk();
            hvfs_format();
            hvfs_sync();
            break;
    }
}
```

## 安装向导集成

### 安装向导实现状态 (2026-04-19 验证完成)

AntX 提供两个版本的安装向导：

| 版本 | 文件 | 运行模式 | 用途 |
|------|------|----------|------|
| 内核态版本 | [install_guide.c](file:///home/anfer/Code/C/AntX/src/kernel/install_guide.c) | Ring 0 | 调试/开发用 |
| 用户态版本 | [user_install.c](file:///home/anfer/Code/C/AntX/src/user/install/user_install.c) | Ring 3 | 生产环境使用 |

### 安装向导三步流程

```
┌─────────────────────────────────────────────────────────────┐
│              AntX Installation Wizard                       │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Step 1: Root Account Setup                                 │
│  ├── 输入 root 密码 (最少 4 字符)                            │
│  ├── 确认密码                                               │
│  └── pwid_create_original_root() → 创建原始 Root PWID        │
│                                                             │
│  Step 2: System Configuration                               │
│  ├── 输入主机名 (默认: localhost)                             │
│  ├── sys_sethostname() → 设置内核主机名                      │
│  └── 写入 /etc/hostname                                     │
│                                                             │
│  Step 3: Finalizing Installation                            │
│  ├── sys_fs_sync() → 同步文件系统到磁盘                     │
│  ├── 创建 /.antx_installed 标记文件                          │
│  └── sys_fs_sync() → 再次同步确保持久化                     │
│                                                             │
│  结果: 安装完成，下次启动跳过安装向导                         │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 安装标记检测

```c
// 检查是否需要运行安装向导
int install_guide_check_needed(void) {
    int fd = sys_fs_open("/.antx_installed", HVFS_O_RDONLY, 0);
    if (fd >= 0) {
        sys_fs_close(fd);
        return 0;  // 已安装，不需要再次运行
    }
    return 1;  // 未安装，需要运行安装向导
}
```

### 格式化磁盘

```c
int hvfs_format_disk(void) {
    // 1. 初始化超级块
    struct hvfs_super_block_disk super_disk = {0};
    super_disk.magic = HVFS_MAGIC;
    super_disk.version = HVFS_VERSION;
    super_disk.block_size = HVFS_BLOCK_SIZE;
    super_disk.total_blocks = total_sectors - HVFS_DATA_SECTOR_START;
    super_disk.free_blocks = super_disk.total_blocks;
    super_disk.inode_count = HVFS_MAX_INODES;
    super_disk.free_inodes = HVFS_MAX_INODES - 1;
    super_disk.first_data_block = 0;
    super_disk.root_inode = 1;
    super_disk.created_time = get_time();
    
    // 2. 写入超级块
    ata_write_sectors(0, HVFS_SUPER_SECTOR_START, 
                      HVFS_SUPER_SECTOR_COUNT, &super_disk);
    
    // 3. 初始化 inode 表
    struct hvfs_inode_disk inode_disk = {0};
    for (int i = 0; i < HVFS_MAX_INODES; i++) {
        ata_write_sector(0, HVFS_INODE_SECTOR_START + i, &inode_disk);
    }
    
    // 4. 初始化位图
    uint8_t bitmap[HVFS_BLOCK_BITMAP_COUNT * 512] = {0};
    ata_write_sectors(0, HVFS_BLOCK_BITMAP_START, 
                      HVFS_BLOCK_BITMAP_COUNT, bitmap);
    
    // 5. 创建根目录
    hvfs_create_root_directory();
    
    return 0;
}
```

## 缓存策略

### 简单缓存设计

```c
#define HVFS_CACHE_SIZE  16

struct hvfs_cache_entry {
    uint32_t block_num;
    uint8_t  data[HVFS_BLOCK_SIZE];
    uint8_t  dirty;
    uint8_t  valid;
};

static struct hvfs_cache_entry hvfs_cache[HVFS_CACHE_SIZE];

uint8_t* hvfs_get_block(uint32_t block_num) {
    // 1. 查找缓存
    for (int i = 0; i < HVFS_CACHE_SIZE; i++) {
        if (hvfs_cache[i].valid && 
            hvfs_cache[i].block_num == block_num) {
            return hvfs_cache[i].data;
        }
    }
    
    // 2. 缓存未命中，加载块
    int victim = find_cache_victim();
    if (hvfs_cache[victim].dirty) {
        // 写回脏块
        ata_write_sector(0, HVFS_DATA_SECTOR_START + 
                        hvfs_cache[victim].block_num, 
                        hvfs_cache[victim].data);
    }
    
    // 加载新块
    ata_read_sector(0, HVFS_DATA_SECTOR_START + block_num,
                   hvfs_cache[victim].data);
    hvfs_cache[victim].block_num = block_num;
    hvfs_cache[victim].valid = 1;
    hvfs_cache[victim].dirty = 0;
    
    return hvfs_cache[victim].data;
}
```

## 错误处理

### 错误码

```c
#define HVFS_OK              0
#define HVFS_ERR_IO         -1
#define HVFS_ERR_NOSPC      -2
#define HVFS_ERR_NOENT      -3
#define HVFS_ERR_EXIST      -4
#define HVFS_ERR_PERM       -5
#define HVFS_ERR_CORRUPT    -6
```

### 文件系统检查

```c
int hvfs_fsck(void) {
    int errors = 0;
    
    // 1. 检查超级块
    if (hvfs_super.magic != HVFS_MAGIC) {
        serial_puts(SERIAL_COM1, "fsck: Invalid super block magic\n");
        return -1;
    }
    
    // 2. 检查 inode 一致性
    for (int i = 1; i < HVFS_MAX_INODES; i++) {
        if (hvfs_inode_table[i].used) {
            // 检查块指针有效性
            for (int j = 0; j < 12; j++) {
                uint32_t block = hvfs_inode_table[i].direct_blocks[j];
                if (block && !BLOCK_IS_USED(hvfs_block_bitmap, block)) {
                    serial_puts(SERIAL_COM1, "fsck: Block bitmap mismatch\n");
                    errors++;
                }
            }
        }
    }
    
    // 3. 检查块位图一致性
    // ...
    
    return errors;
}
```

## Makefile 更新

```makefile
# 磁盘镜像生成
DISK_IMAGE = build/antx.img

$(DISK_IMAGE): build/kernel.bin
    @echo "Creating disk image..."
    @dd if=/dev/zero of=$@ bs=1M count=2
    @dd if=build/boot.bin of=$@ conv=notrunc
    @dd if=build/kernel.bin of=$@ bs=512 seek=2 conv=notrunc

disk: $(DISK_IMAGE)

# 运行带磁盘的 QEMU
run-disk: $(DISK_IMAGE)
    qemu-system-x86_64 -drive file=$(DISK_IMAGE),format=raw -serial stdio
```

## 测试计划

| 测试项 | 描述 | 预期结果 |
|--------|------|----------|
| 磁盘检测 | 检测未格式化磁盘 | 返回 UNFORMATTED |
| 格式化 | 格式化空白磁盘 | 创建有效 HvFS |
| 挂载 | 挂载已格式化磁盘 | 成功加载元数据 |
| 文件创建 | 创建文件并写入 | 数据持久化 |
| 重启读取 | 重启后读取文件 | 数据完整 |
| 同步 | 手动同步到磁盘 | 无错误 |
| fsck | 文件系统检查 | 无错误 |

## 实现优先级

1. **P0 - 必需**
   - ATA 驱动基础读写
   - 超级块加载/保存
   - Inode 表加载/保存
   - 基本挂载功能

2. **P1 - 重要**
   - 块位图管理
   - 数据块读写
   - 同步机制

3. **P2 - 增强**
   - 缓存层
   - fsck 工具
   - 性能优化

## 参考资料

- ATA/IDE 规范
- ext2 文件系统设计
- FAT 文件系统设计

## 预设目录结构

根据 hivefs.md 设计，格式化后创建以下目录结构：

```
/
├── bin/           # 可执行文件
├── sbin/          # 系统可执行文件
├── etc/           # 配置文件
│   ├── pwid.db    # PWID 数据库
│   └── system.conf
├── home/          # 用户目录（按 PWID 分组）
│   ├── 0xA1B2...  # 以 PWID 作为目录名
│   └── 0xC3D4...
├── tmp/           # 临时文件
├── dev/           # 设备文件
├── proc/          # 进程信息（伪文件系统）
└── sys/           # 系统信息（伪文件系统）
```

### PWID 用户目录

```
/home/
└── 0xa1b2c3d4e5f60718  # 用户 PWID 目录
    ├── documents/
    ├── downloads/
    └── .config/
```

### 初始化目录创建

```c
int hvfs_create_default_directories(void) {
    // 创建标准目录
    hvfs_mkdir("/bin", 0);
    hvfs_mkdir("/sbin", 0);
    hvfs_mkdir("/etc", 0);
    hvfs_mkdir("/home", 0);
    hvfs_mkdir("/tmp", 0);
    hvfs_mkdir("/dev", 0);
    hvfs_mkdir("/proc", 0);
    hvfs_mkdir("/sys", 0);
    
    // 创建 PWID 数据库文件
    int fd = hvfs_open("/etc/pwid.db", HVFS_O_CREAT | HVFS_O_WRONLY, 0);
    hvfs_close(fd);
    
    return 0;
}
```

## PWID 权限集成

### 权限位定义

```
┌────────────────────────────────────────────────────────────┐
│                  PWID 权限位 (16位)                        │
├────────────────────────────────────────────────────────────┤
│  [15-12]  │ [11-8] │ [7-4] │ [3-0]                       │
│  Special   │ Owner  │ Group │ Other                       │
├────────────────────────────────────────────────────────────┤
│  Special:                                                  │
│    0x1000: 粘滞位                                           │
│    0x2000: SGID                                            │
│    0x4000: SUID                                            │
│                                                            │
│  Owner/Group/Other:                                        │
│    bit 2: 读 (r)                                           │
│    bit 1: 写 (w)                                           │
│    bit 0: 执行 (x)                                         │
└────────────────────────────────────────────────────────────┘
```

### 权限检查

```c
bool check_pwid_permission(struct inode *inode, uint64_t pwid, int access_type) {
    int level = get_pwid_level(pwid);
    
    if (level == PWID_LEVEL_ROOT) {
        return true;
    }
    
    if (pwid == inode->owner_pwid) {
        return (inode->pwid_perm >> 8) & (access_type << 4);
    }
    
    if (pwid == inode->group_pwid) {
        return (inode->pwid_perm >> 4) & (access_type << 4);
    }
    
    return inode->pwid_perm & (access_type << 4);
}
```

### 权限继承

```c
void inherit_permissions(struct inode *parent, struct inode *child, uint64_t creator_pwid) {
    child->owner_pwid = creator_pwid;
    child->group_pwid = parent->group_pwid;
    child->pwid_perm = parent->pwid_perm & ~current_umask;
}

void set_default_permissions(struct inode *inode, uint64_t creator_pwid) {
    inode->owner_pwid = creator_pwid;
    inode->group_pwid = 0;
    inode->pwid_perm = 0640;
}
```

## 日志区域 (可选)

### 日志结构

```c
#define HVFS_LOG_MAGIC      0x4C4F4731  // "LOG1"
#define HVFS_LOG_ENTRY_SIZE 64

struct hvfs_log_header {
    uint32_t magic;
    uint32_t sequence;
    uint32_t checksum;
    uint32_t entry_count;
};

struct hvfs_log_entry {
    uint64_t timestamp;
    uint32_t operation;      // 操作类型
    uint32_t inode_num;
    uint32_t block_num;
    uint32_t data_len;
    uint8_t  data[40];       // 操作数据
};
```

### 日志操作类型

| 操作 | 值 | 描述 |
|------|-----|------|
| LOG_OP_CREATE | 1 | 创建文件/目录 |
| LOG_OP_DELETE | 2 | 删除文件/目录 |
| LOG_OP_WRITE | 3 | 写入数据 |
| LOG_OP_RENAME | 4 | 重命名 |
| LOG_OP_CHMOD | 5 | 修改权限 |
| LOG_OP_CHOWN | 6 | 修改所有者 |

### 日志恢复

```c
int hvfs_replay_log(void) {
    struct hvfs_log_header header;
    ata_read_sector(0, HVFS_LOG_SECTOR_START, &header);
    
    if (header.magic != HVFS_LOG_MAGIC) {
        return 0;  // 无日志
    }
    
    struct hvfs_log_entry entry;
    for (uint32_t i = 0; i < header.entry_count; i++) {
        // 读取并重放日志条目
        // ...
    }
    
    return 0;
}
```

## 私有文件机制

用户可以创建私有文件，仅自己可访问：

```c
// 创建私有文件
int create_private_file(const char *name, uint64_t pwid) {
    char path[128];
    snprintf(path, sizeof(path), "/home/%016lx/%s", pwid, name);
    
    int fd = hvfs_open(path, HVFS_O_CREAT | HVFS_O_WRONLY, 0);
    if (fd < 0) return fd;
    
    struct inode *inode = hvfs_get_inode(fd);
    inode->owner_pwid = pwid;
    inode->pwid_perm = 0600;  // 仅所有者可读写
    
    hvfs_close(fd);
    return 0;
}
```

## 与 hivefs.md 的对应关系

| hivefs.md 设计 | hvfs-disk.md 实现 | 状态 |
|----------------|-------------------|------|
| Boot Block | 引导扇区 (扇区 0-1) | ✅ |
| Super Block | 超级块 (扇区 2-9) | ✅ |
| Inode Table | Inode 表 (扇区 10-137) | ✅ |
| Data Blocks | 数据区 (扇区 171+) | ✅ |
| Log Area | 日志区域 (扇区 169-170) | ✅ |
| PWID 权限位 | pwid_perm 字段 | ✅ |
| 预设目录结构 | hvfs_create_default_directories() | ✅ |
| 权限继承 | inherit_permissions() | ✅ |
| 私有文件机制 | create_private_file() | ✅ |
