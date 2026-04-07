# AntX HvFS 文件系统设计

## 一、设计概述

HvFS（Hive File System，简称 HVFS）是 AntX 的专属文件系统，与 PWID 权限模型深度集成。

### 1.1 设计目标

- 现阶段简单但可靠的文件系统
- 与 PWID 权限模型原生集成
- 支持基本文件操作
- 专注 HvFS 专属设计

### 1.2 文件系统架构

> ⚠️ **暂不考虑对其他文件系统的兼容**，专注于 HvFS 的设计与实现。

```
┌─────────────────────────────────────────────────────────────┐
│                      VFS 层（虚拟文件系统）                   │
│  - 统一接口抽象                                             │
│  - 路径解析                                                 │
│  - 权限检查（PWID）                                        │
│  - 目前仅支持 HvFS                                         │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌───────────────┐
│  HvFS        │
│  (专属格式)   │
└───────────────┘
        │
        ▼
┌───────────────┐
│  磁盘驱动     │
└───────────────┘
```

## 二、HvFS 专属格式

### 2.1 磁盘布局

```
┌─────────────────────────────────────────────────────────────┐
│                   HvFS 磁盘布局                            │
├────────────┬────────────┬────────────┬────────────┬──────────┤
│   Boot     │   Super    │   Inode    │   Data     │   Log    │
│   Block    │   Block    │   Table    │   Blocks   │   Area   │
│  (1块)    │  (1块)    │  (若干块)  │  (若干块)  │  (1块)  │
└────────────┴────────────┴────────────┴────────────┴──────────┘
```

| 区域 | 大小 | 说明 |
|------|------|------|
| Boot Block | 1块 | 引导代码 |
| Super Block | 1块 | 文件系统元信息 |
| Inode Table | N块 | i 节点表 |
| Data Blocks | N块 | 数据块 |
| Log Area | 1块 | 日志区域（可选） |

### 2.2 Super Block 结构

```c
struct super_block {
    uint32_t magic;           // 魔数: 0x48564653 ("HVFS")
    uint32_t block_size;      // 块大小（字节）
    uint32_t total_blocks;    // 总块数
    uint32_t free_blocks;    // 空闲块数
    uint32_t inode_count;    // i 节点数量
    uint32_t free_inodes;    // 空闲 i 节点数量
    uint32_t first_data_block;// 第一个数据块号
    uint32_t inode_bitmap;   // i 节点位图所在块
    uint32_t block_bitmap;   // 块位图所在块
    uint32_t max_path_depth; // 最大路径深度（默认 128）
    uint32_t max_entries;    // 单级目录最大文件数（默认 65535）
    uint64_t created_time;   // 创建时间
    uint64_t modified_time;  // 修改时间
};
```

### 2.3 Inode 结构

```c
struct inode {
    uint32_t inode_num;      // i 节点号
    uint16_t mode;           // 文件类型和权限
    uint16_t uid;            // 所有者（保留，为兼容）
    uint32_t size;           // 文件大小（字节）
    uint64_t atime;          // 访问时间
    uint64_t mtime;          // 修改时间
    uint64_t ctime;          // 创建时间
    
    // PWID 权限控制（AntX 特色）
    uint64_t owner_pwid;    // 所有者 PWID
    uint64_t group_pwid;    // 组 PWID（保留）
    uint16_t pwid_perm;     // PWID 权限位
    
    // 文件数据
    uint32_t direct_blocks[12];  // 直接块指针（12个）
    uint32_t indirect_block;     // 间接块指针
    uint32_t double_indirect;   // 双重间接块
    
    // 元数据
    uint32_t link_count;     // 硬链接数
    uint32_t ref_count;     // 引用计数
};
```

### 2.4 PWID 权限位

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

### 2.5 PWID 权限检查

```c
// 文件权限检查逻辑
bool check_pwid_permission(struct inode *inode, uint64_t pwid, int access_type) {
    // 获取调用者的权限等级
    int level = get_pwid_level(pwid);
    
    // Root 权限直接通过
    if (level == PWID_LEVEL_ROOT) {
        return true;
    }
    
    // 检查所有者 PWID
    if (pwid == inode->owner_pwid) {
        return (inode->pwid_perm >> 8) & (access_type << 4);
    }
    
    // 检查组 PWID（预留）
    if (pwid == inode->group_pwid) {
        return (inode->pwid_perm >> 4) & (access_type << 4);
    }
    
    // 其他 PWID
    return inode->pwid_perm & (access_type << 4);
}
```

## 三、目录结构

### 3.1 目录项结构

```c
struct dir_entry {
    uint32_t inode;          // i 节点号
    uint16_t rec_len;       // 目录项长度
    uint8_t  name_len;      // 文件名长度
    uint8_t  file_type;     // 文件类型
    char     name[256];      // 文件名
};
```

### 3.2 预设目录

```
/
├── bin/           # 可执行文件
├── sbin/          # 系统可执行文件
├── etc/           # 配置文件
│   ├── pwid.db   # PWID 数据库
│   └── system.conf
├── home/          # 用户目录（按 PWID 分组）
│   ├── 0xA1B2... # 以 PWID 作为目录名
│   └── 0xC3D4...
├── tmp/           # 临时文件
├── dev/           # 设备文件
├── proc/          # 进程信息（伪文件系统）
└── sys/           # 系统信息（伪文件系统）
```

### 3.3 PWID 用户目录

```
/home/
└── 0xa1b2c3d4e5f60718  # 用户 PWID 目录
    ├── documents/
    ├── downloads/
    └── .config/
```

## 四、文件操作

### 4.1 权限模型

与传统 UNIX 的 owner/group/other 不同，HvFS 使用 **PWID 权限模型**：

| 权限位 | 含义 |
|--------|------|
| r (读) | 可以读取文件内容/列出目录 |
| w (写) | 可以修改文件内容/创建删除目录项 |
| x (执行) | 可以执行文件/进入目录 |

### 4.2 权限检查流程

```
文件操作请求
      │
      ▼
┌──────────────────┐
│ 获取调用者 PWID  │
└──────────────────┘
      │
      ▼
┌──────────────────┐
│ 获取目标文件     │
│ Inode            │
└──────────────────┘
      │
      ▼
┌──────────────────┐
│ 权限匹配检查     │
└──────────────────┘
      │
   ┌──┴──┐
   ▼     ▼
  通过   拒绝
   │     │
   ▼     ▼
 执行  返回错误
```

### 4.3 特殊权限

| 权限 | 说明 |
|------|------|
| SUID | 执行时以文件所有者 PWID 运行 |
| SGID | 执行时以文件所属组 PWID 运行 |
| 粘滞位 | 仅所有者可删除目录内文件 |

## 五、文件系统操作

### 5.1 核心系统调用

| 调用 | 说明 |
|------|------|
| mount | 挂载文件系统 |
| umount | 卸载文件系统 |
| open | 打开文件 |
| close | 关闭文件 |
| read | 读取文件 |
| write | 写入文件 |
| seek | 移动文件指针 |
| stat | 获取文件状态 |
| chmod | 修改权限 |
| chown | 修改所有者 PWID |
| mkdir | 创建目录 |
| rmdir | 删除目录 |
| unlink | 删除文件 |
| rename | 重命名 |
| readdir | 读取目录 |

### 5.2 文件描述符

```c
struct file_descriptor {
    uint32_t fd;            // 文件描述符
    struct inode *inode;    // 指向 inode
    uint64_t offset;        // 当前偏移
    int flags;              // 打开标志
    uint64_t pwid;         // 打开时的 PWID
};
```

## 六、与 PWID 深度集成

### 6.1 文件 PWID 所有权

每个文件/目录都有一个 **owner_pwid** 属性，标识该文件的所有者：

```c
// 创建文件时的默认权限
void set_default_permissions(struct inode *inode, uint64_t creator_pwid) {
    inode->owner_pwid = creator_pwid;
    inode->group_pwid = 0;  // 暂不使用组
    inode->pwid_perm = 0640; // rw-r-----
}
```

### 6.2 私有文件机制

```
用户 A (PWID=0xA1B2) 的私有文件：
/home/0xa1b2c3d4e5f60718/
├── secret.txt      (仅 A 可读写)
├── shared.txt      (可分享给其他 PWID)
└── config/         (A 的配置目录)
```

### 6.3 权限继承

```c
// 创建文件/目录时继承父目录属性
void inherit_permissions(struct inode *parent, struct inode *child) {
    child->owner_pwid = parent->owner_pwid;
    // 默认权限基于 umask
    child->pwid_perm = parent->pwid_perm & ~current_umask;
}
```

## 七、存储管理

### 7.1 块分配策略

- **分配**：优先使用空闲块
- **回收**：标记为可重用
- **位图管理**：使用位图跟踪块使用

```c
// 块位图操作
uint32_t allocate_block() {
    for (int i = 0; i < sb->total_blocks; i++) {
        if (!test_bit(block_bitmap, i)) {
            set_bit(block_bitmap, i);
            sb->free_blocks--;
            return i;
        }
    }
    return 0;  // 无空闲块
}
```

### 7.2 Inode 分配

- i 节点号唯一
- 分配时查找空闲 i 节点

## 八、可扩展性（暂不考虑）

> ⚠️ **暂不考虑对其他文件系统的兼容**，HvFS 将作为 AntX 的专属文件系统持续迭代。

当前仅支持 HvFS，不支持 FAT32、ext2 等其他文件系统。

## 九、设计特点

HvFS 作为 AntX 的专属文件系统，具有以下特点：

1. **PWID 原生集成** - 文件权限直接与 PWID 绑定
2. **简洁高效** - 简单易实现的磁盘布局
3. **专属设计** - 不兼容其他文件系统，专注自身优化
4. **安全可控** - 基于 PWID 的细粒度权限控制
