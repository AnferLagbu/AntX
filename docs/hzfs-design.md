# HzFS 三特征融合设计文档

> **版本**: 1.0  
> **状态**: 设计阶段  
> **作者**: Anfer  
> **日期**: 2025-05-15  

---

## 摘要

HzFS 是 AntX 操作系统的原生文件系统，融合了三项核心特征：

| 特征 | 来源 | 目标 |
|------|------|------|
| **PWID 安全标签** (特色1) | AntX 原生安全模型 | 数据安全 |
| **混合存储引擎** (特色2) | ZFS + ext4 优势融合 | 性能与效率 |
| **弹性恢复体系** (特色3) | Barrier Stack + ZIL + COW Snap | 故障保护 |

本文档对三项特征的设计、交互、冲突与解决方案进行深度分析。

---

## 目录

1. [架构总览](#1-架构总览)
2. [特色1: PWID 安全标签深度集成](#2-特色1-pwid-安全标签深度集成)
3. [特色2: 混合存储引擎](#3-特色2-混合存储引擎)
4. [特色3: 弹性恢复体系](#4-特色3-弹性恢复体系)
5. [特征正交性证明](#5-特征正交性证明)
6. [关键路径竞态分析](#6-关键路径竞态分析)
7. [域注册与依赖图设计](#7-域注册与依赖图设计)
8. [延迟分配 + TXG + Barrier 三元协同](#8-延迟分配--txg--barrier-三元协同)
9. [四层防御与文件系统降级](#9-四层防御与文件系统降级)
10. [ZIL 与 Barrier 握手协议](#10-zil-与-barrier-握手协议)
11. [UndoLog 容量规划](#11-undolog-容量规划)
12. [实施路线图](#12-实施路线图)
13. [测试矩阵](#13-测试矩阵)

---

## 1. 架构总览

```
┌─────────────────────────────────────────────────────────────────┐
│                      VFS / FFI 层                               │
│               hzfs_open/read/write/mkdir/sync                   │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌───────────────────────┐  ┌───────────────────────────────┐  │
│  │  HzFS Dataset 管理     │  │  PWID 安全层 (特色1)          │  │
│  │  ┌───────┐ ┌───────┐  │  │  ┌─────────────────────────┐  │  │
│  │  │ root  │ │ /home │  │  │  │ owner_pwid              │  │  │
│  │  └───┬───┘ └───┬───┘  │  │  │ sensitivity 标签         │  │  │
│  │      └─────┬─────┘    │  │  │ pwid_perm               │  │  │
│  │           ▼           │  │  │ ACE + 信任链             │  │  │
│  │  DMU Object Set       │  │  └─────────────────────────┘  │  │
│  └───────────┬───────────┘  └───────────────────────────────┘  │
│              │                                                   │
│  ┌───────────▼───────────────────────────────────────────────┐  │
│  │  混合存储引擎 (特色2)                                      │  │
│  │  ┌──────────────────┐  ┌──────────────────────────────┐  │  │
│  │  │ Extent BP 树      │  │ HTree ZAP 目录               │  │  │
│  │  │ - ZFS: COW+Checksum│  │ - ext4: O(log n) 查找        │  │  │
│  │  │ - ext4: 连续区间   │  │ - ZFS: 属性存储              │  │  │
│  │  └──────────────────┘  └──────────────────────────────┘  │  │
│  │  ┌──────────────────┐  ┌──────────────────────────────┐  │  │
│  │  │ 延迟分配器        │  │ Inline Data (小文件)         │  │  │
│  │  │ - 批量连续分配    │  │ - ≤56B 存 Object 内         │  │  │
│  │  └──────────────────┘  └──────────────────────────────┘  │  │
│  └───────────┬───────────────────────────────────────────────┘  │
│              │                                                   │
│  ┌───────────▼───────────────────────────────────────────────┐  │
│  │  弹性恢复体系 (特色3)                                      │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │  │
│  │  │ Barrier Stack │  │  ZIL Intent  │  │  COW Snap    │    │  │
│  │  │ (内存态/μs)   │  │  Log (持久/ms)│  │  (用户态/s)   │    │  │
│  │  │ 域ID=6       │  │  写前日志    │  │  即时快照     │    │  │
│  │  │ 字段级回滚   │  │  崩溃重放    │  │  在线恢复     │    │  │
│  │  └──────────────┘  └──────────────┘  └──────────────┘    │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## 2. 特色1: PWID 安全标签深度集成

### 2.1 设计目标

将 AntX 的 PWID (Privilege Work ID) 安全模型深度嵌入文件系统的每个层面，使数据安全成为架构的内在属性而非附加层。

### 2.2 集成层次

```
用户态系统调用
    │
    ▼ pwid = current_pwid()
┌─────────────────────────────────────────┐
│  VFS 层                                 │
│  check_permission(obj, pwid, cap)       │
│  ┌───────────────────────────────────┐  │
│  │ 五层检查:                          │  │
│  │ L0: Root 旁路 (level==0xFF 拒绝)   │  │
│  │ L1: Sensitivity 敏感度标签         │  │
│  │ L2: ACE 访问控制条目               │  │
│  │ L3: Capability 能力掩码            │  │
│  │ L4: Trust chain 信任链             │  │
│  └───────────────────────────────────┘  │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│  DMU Object 层                          │
│  ┌────────────────────────────────────┐ │
│  │ HzDmuObject {                      │ │
│  │   owner_pwid: u64   ← 所有者身份    │ │
│  │   group_pwid: u64   ← 组身份       │ │
│  │   sensitivity: u8   ← 敏感度标签    │ │
│  │   pwid_perm: u16    ← PWID 权限位  │ │
│  │ }                                  │ │
│  └────────────────────────────────────┘ │
│  COW 时自动继承父对象的安全标签          │
└─────────────────────────────────────────┘
```

### 2.3 与 Barrier Stack 的 PWID 复用

Barrier Stack 本身使用 PWID 标签进行域间能力管理：

```
HzFS 域 (ID=6):
  dom_cap_mask (能力掩码):
    CAP_FS_WRITE (1<<0)   ← PWID 写入能力
    CAP_NET_SEND (1<<1)   ← 预留
    CAP_PROC_CREATE (1<<2) ← 预留

降级时:
  failures=3 → 剥夺 CAP_FS_WRITE → HzFS 只读
  failures=4 → 剥夺全部能力 → 隔离
```

---

## 3. 特色2: 混合存储引擎

### 3.1 设计哲学

从 ZFS 继承**数据完整性**（COW、Checksum、快照），从 ext4 继承**性能优化**（Extent、HTree、延迟分配、Inline Data）。

### 3.2 Extent Block Pointer

```rust
// 传统 ZFS 固定 BP (128B) — 大文件需要 3 级间接树
// 1GB 文件 ≈ 250,000 个 BP ≈ ~39,000 个间接块

// HzFS Extent BP — 单条目描述连续区间
#[repr(C)]
pub struct HzExtentEntry {
    pub lba_start: u64,      // 起始 LBA
    pub block_count: u32,    // 连续块数 (最大 4GB)
    pub flags: u8,           // 压缩/加密标志
    pub comp_size: u32,      // 压缩后大小
}

// 同一 1GB 文件: 单 ExtentEntry
// Metadata 开销: 39,000 块 → 1 个条目 (250,000x 缩减)
```

**双模式**：小文件 (< 64KB) 使用固定 BP（低开销），大文件 (> 64KB) 使用 Extent BP（高效）。

### 3.3 HTree 目录索引

```
当前线性 ZAP:                        HTree ZAP:
┌──────────────────┐            ┌──────────┐
│ entry[0] = "aaa" │            │  root    │  ← 1 个间接块
│ entry[1] = "abc" │            │ hash=0x00│──→ ┌──────────┐
│ ...              │            └──────────┘    │ level 1  │
│ entry[N] = "zzz" │                            │ 0x00─0x1F│──→ ...
│ O(n) 搜索        │                            │ 0x20─0x3F│──→ ...
└──────────────────┘                            └──────────┘
                                                  O(log n) 搜索
10 万文件 = 10 万次比较 (1ms)                    10 万文件 = ~3 次比较 (30ns)
```

### 3.4 延迟分配

```
传统 COW 路径:                     延迟分配路径:
write(buf)                         write(buf)
  ├─ SPA::allocate() ← 立即分配       ├─ 标记 dirty
  ├─ SPA::write_bp()                  └─ 返回 (未分配块, 不写盘)
  └─ COW 更新 BP                      
                                     TXG sync (批量):
                                       ├─ 收集所有待分配区间
                                       ├─ 排序 + 合并相邻区间
                                       │   [0..4096] + [4096..8192]
                                       │       → [0..8192] (Extent)
                                       ├─ SPA::allocate_contiguous()
                                       └─ SPA::write_bp() 批量写入

结果: 碎片化                         结果: 连续布局, 更高吞吐
```

### 3.5 Inline Data (小文件)

```
传统:                                Inline:
┌──────────────────┐                 ┌──────────────────────────────┐
│ HzDmuObject      │ 256B           │ HzDmuObject + Inline Data     │
│ bp ───→ 4KB 块   │                 │ data[0..56] = "hello.ts\0    │
│ size=10          │ (浪费 4086B)    │   ...exec /bin/sh"           │
└──────────────────┘                 │ flags |= INLINE               │
                                     │ bp = null (无需分配块)         │
 100 万小文件 = 4GB wasted           └──────────────────────────────┘
                                     100 万小文件 = 0 wasted
```

---

## 4. 特色3: 弹性恢复体系

### 4.1 三层恢复矩阵

```
┌─────────────────────────────────────────────────────────────────┐
│                        故障覆盖矩阵                              │
│                                                                 │
│          │  Barrier Stack  │  ZIL Intent Log  │  COW Snapshot   │
│ ─────────┼─────────────────┼──────────────────┼─────────────────│
│ 恢复层   │  内存态          │  持久化            │  用户态          │
│ 恢复延迟 │  μs 级           │  ms 级             │  秒级           │
│ 粒度     │  字段级 (8B)     │  记录级 (KB)       │  快照级 (MB-GB)  │
│ 触发条件 │  panic / 逻辑错  │  断电 / 硬崩溃      │  用户请求        │
│ 回滚方向 │  后滚 (undo)     │  前滚 (redo)        │  全量替换        │
│ 持久化   │  否 (纯内存)     │  是 (同步写盘)       │  是 (COW)       │
│ ─────────┼─────────────────┼──────────────────┼─────────────────│
│ 适用场景 │  指针非法访问    │  系统崩溃            │  误删除         │
│          │  ARC 内部错     │  写入中断            │  数据损坏       │
│          │  TXG 状态机异常  │  磁盘部分写入        │  版本回退       │
└─────────────────────────────────────────────────────────────────┘
```

### 4.2 Barrier Stack 集成

HzFS 注册为 Barrier 域 (ID=6)，利用 AntX 栏栈的微秒级字段回滚。

**捕获内容** (capture_cb):
```
HzfsBarrierSnapshot {
    arc_mru_len, arc_mfu_len, arc_p      ← ARC 自适应参数
    fd_count, next_fd                     ← 打开文件状态
    open_txg_id, quiescing_txg_id         ← TXG 状态
    dirty_bp_count                        ← 脏块计数
    alloc_count, free_count               ← 空间统计
    dataset_count, root_ds_id             ← Dataset 状态
    zil_pending_count, zil_committed_seq  ← ZIL 状态
}
```

**关键设计**: 只捕获控制结构元数据，不捕获数据块内容。数据块由 ZIL/TXG 持久化机制保护。

**回滚回调** (rollback_cb):
```
hzfs_barrier_rollback_cb():
  1. ARC: 驱逐低于回滚代际的条目
  2. TXG: 丢弃未提交事务
  3. ZIL: 重放已提交但未同步的记录
  4. FD 表: 修复悬挂引用
```

### 4.3 域依赖关系

```
HzFS (ID=6)
  depends_on[0] = Some(2)  ← VFS
  depends_on[1] = None
  ...

回滚顺序:
  VFS panic → cascade_rollback(2) → BFS:
    1. VFS 回滚 (ID=2)
    2. BFS 传播到 HzFS (ID=6, 因为 VFS 的 depended_by 包含 6)
    3. HzFS 回滚
```

### 4.4 四层防御与降级

| 连续失败 | 防御动作 | HzFS 行为 |
|----------|---------|----------|
| 1-2 | 正常回滚 | 正常服务 |
| 3 | 剥夺 CAP_FS_WRITE | 降级为只读 |
| 4 | 剥夺 CAP_FS_WRITE \| CAP_NET_SEND \| CAP_PROC_CREATE | 深度降级 |
| ≥5 | 永久隔离 | 域隔离，不可恢复 |

**自愈路径**:
```
降级后连续 1 个 barrier 周期 (100 ticks) 无 panic → mark_recovered()
  → consecutive_failures = 0
  → dom_cap_mask = original_cap_mask
  → state = Active
```

---

## 5. 特征正交性证明

### 5.1 独立性分析

| 特征 | 作用对象 | 修改频率 | 内存占用 | 磁盘持久化 |
|------|---------|---------|---------|-----------|
| PWID | 元数据字段 | 低频(创建/打开) | O(objects) | 是(DMU字段) |
| 混合引擎 | 块寻址/目录索引 | 中频(分配/写入) | O(extents) | 是(BP树) |
| 弹性恢复 | 控制结构+磁盘块 | 高频(每tick) | O(entries×256) | Barrier:否 ZIL:是 |

三个特征作用于 HzFS 的不同层次，彼此间无逻辑依赖。

### 5.2 唯一冲突: BP 指针回滚与磁盘块生命周期

**场景**: `write()` 执行 COW 过程中发生 panic

```rust
// 时间轴
t=0: write() 开始
t=1:   obj.cow_bp(new_bp, txg)         ← BP 指针改变
t=2:   UndoLog::record(obj.bp, old_bp)  ← 记录撤销
t=3:   === BARRIER PUSH ===            ← capture_snapshot
t=4:   SPA::write_bp(new_bp, data)     ← 新数据写入磁盘
t=5:   SPA::free(old_bp)               ← 旧块标记为释放
t=6:   === PANIC ===                   ← int 0x82
```

**问题**: 回滚 `obj.bp → old_bp` 后，old_bp 的 LBA 已在 t=5 被 free，下次 allocate 可能覆盖。

**解决方案**: UndoLog 必须记录配对操作：
- t=2: `record(obj.bp, old_bp)` — 恢复指针
- t=5: `record(metaslab_bitmap_word, old_word)` — 恢复位图

同时，free 操作必须在 TXG committed 之后延迟执行，而非立即执行。

---

## 6. 关键路径竞态分析

### 6.1 write() 路径的安全状态机

```
状态A: 初始状态
  obj.bp = old_bp, obj.size = old_size
  Metaslab: old_bp 已分配

状态B: 分配新块后
  new_bp = spa.allocate()
  spa.write_bp(new_bp, data)
  UndoLog::record(obj.bp, old_bp)    ← 防护点1
  UndoLog::record(obj.size, old_size) ← 防护点2

状态C: 指针替换后
  obj.bp = new_bp
  obj.size = new_size
  // 此时 panic → UndoLog 恢复状态B

状态D: TXG 提交后
  txg.commit()
  // 此时 panic → 不需要回滚, 磁盘已持久化

状态E: 延迟释放后
  spa.free_deferred(old_bp)  // 仅在 TXG committed 后
  UndoLog::record(metaslab_bitmap, old_bitmap) ← 防护点3
```

### 6.2 各状态 panic 的结果

| Panic 状态 | Barrier 行为 | 磁盘状态 | 一致性 |
|-----------|-------------|---------|--------|
| A→B 之间 | 无 undo，状态A | new_bp 孤儿(泄漏) | ✅ mount scrub 回收 |
| B→C 之间 | undo: bp+size 回滚 | new_bp 孤儿 | ✅ scrub 回收 |
| C→D 之间 | undo: bp+size 回滚 | old_bp 已提交, new_bp 待 GC | ✅ ZIL replay 确认 |
| D→E 之间 | 无 undo 需要 | 已持久化 | ✅ 完全一致 |
| E 之后 | 无 undo 需要 | 已持久化 | ✅ 完全一致 |

---

## 7. 域注册与依赖图设计

### 7.1 域 ID 分配

| ID | 域 | 注册方式 |
|----|-----|---------|
| 1 | 系统保留 | — |
| 2 | VFS | VfsManager::init() 内注册 |
| 3 | PMM | pmm_register_barrier_domain() |
| 4 | PROC | proc_register_barrier_domain() |
| 5 | NET | net_register_barrier_domain() |
| **6** | **HzFS** | **hzfs_register_barrier_domain()** (新增) |

### 7.2 依赖图设计

```
                PMM (ID=3)
                  ↑
         隐式依赖 (kmalloc)
                  │
    ┌─────────────┼─────────────┐
    │             │             │
  VFS (ID=2)  PROC (ID=4)   NET (ID=5)
    ↑             │             │
    │ depends_on  │             │
    │             │             │
  HzFS (ID=6) ←  ┘             │
    │                           │
    └─────── depends_on ────────┘  (可选: 网络文件系统场景)
```

**设计决策**:
- HzFS 显式依赖 VFS (ID=2): 因为 HzFS 的 FD 表通过 VFS 路由
- HzFS 不显式依赖 PMM: 如果 PMM panic，HzFS 无法在无内存时回滚
- 但 VFS panic 会 BFS 传播到 HzFS，间接覆盖了依赖关系

**循环依赖风险**: VFS 和 HzFS 互相需要对方有效。解决方案：
- 不建立循环依赖
- HzFS 的 rollback_cb 显式调用 `vfs_barrier_restore()`
- 或将两者合并为单一"文件系统域"

### 7.3 BFS 回滚顺序

```
cascade_rollback(domain_id=6):  // HzFS 触发
  1. HzFS 回滚 (先回滚自身)
  2. 遍历 HzFS 的 depended_by:
     - 如果有域 A 依赖 HzFS，加入队列
     - 但当前 VFS 不依赖 HzFS (depended_by 为空)
  → 仅 HzFS 回滚

cascade_rollback(domain_id=2):  // VFS 触发
  1. VFS 回滚
  2. 遍历 VFS 的 depended_by:
     - HzFS (ID=6) 依赖 VFS
     → 加入队列
  3. HzFS 回滚
  → VFS → HzFS 级联回滚 ✓
```

---

## 8. 延迟分配 + TXG + Barrier 三元协同

### 8.1 完整写入路径

```
write(buf, offset, count):
  │
  ├─ 1. UNDO: record(obj.size, old_size)
  │      obj.size = max(obj.size, offset+count)
  │      obj.dirty = true
  │      // 暂不分配块 (ext4 优化)
  │
  └─ 2. 返回成功 (未写盘, μs 级)

TXG sync (每 100 ticks):
  │
  ├─ 3. 收集所有 dirty objects
  │
  ├─ 4. 延迟分配区间合并:
  │      ranges = merge_sort(all_dirty_ranges)
  │      例: [0..4096, 4096..8192, 12288..16384]
  │        → [0..8192, 12288..16384]
  │
  ├─ 5. UNDO: record(extent_tree_root, old_root)  ← 回滚点
  │
  ├─ 6. 连续区间分配:
  │      for range in ranges:
  │          bp = spa.allocate_contiguous(range.size)
  │          spa.write_bp(bp, data[range])
  │          UNDO: record(metaslab_bitmap_word, old_word)
  │
  ├─ 7. 更新 Extent 树:
  │      extent_tree.replace(old_root, new_root)
  │
  ├─ 8. ZIL 记录:
  │      zil.add_record(Write, txg, obj_id, offset, count)
  │
  └─ 9. TXG 提交:
         uberblock.write_to_disk()

TXG committed 回调:
  │
  └─ 10. 延迟释放旧块:
         for old_extent in old_extent_tree:
             spa.free_deferred(old_extent)
```

### 8.2 各阶段 panic 分析

| Panic 时机 | Barrier 恢复 | 后果 | 处理 |
|-----------|-------------|------|------|
| 步骤1-2 | undo obj.size | 无其他副作用 | ✅ 直接恢复 |
| 步骤3-5 | undo extent_tree | 新块已分配但无引用 | scrub 回收 |
| 步骤6-7 | undo bitmap_word | 位图恢复, 数据块无效 | ✅ 完全恢复 |
| 步骤7-9 | undo extent_tree + bitmap | 需保留旧 extent | ZIL replay 确认 |
| 步骤9-10 | 无需回滚 | 已完全持久化 | ✅ 完全一致 |

### 8.3 孤儿块回收

`mount_disk()` 时执行快速 scrub：扫描所有 Metaslab 位图，遍历所有 Dataset 的 Extent 树，标记未被任何树引用的块为空闲。

```
fast_scrub():
  for metaslab in spa.metaslabs:
      for block in metaslab.allocated_blocks:
          if !any_extent_tree_references(block):
              metaslab.free(block)
```

---

## 9. 四层防御与文件系统降级

### 9.1 降级状态机

```
                    Active (正常)
                      │
     ┌────────────────┼────────────────┐
     │ panic #1       │ panic #2       │ panic #3
     ▼                ▼                ▼
  Active            Active           Degraded
  (继续)            (继续)           CAP_FS_WRITE 移除
                                       │
                                panic #4
                                       ▼
                                   Degraded
                                   CAP_FS_WRITE|NET|PROC 移除
                                       │
                                panic #5
                                       ▼
                                   Quarantined
                                   永久隔离
```

### 9.2 降级语义

```rust
// 写入操作入口检查
fn write(&self, ...) -> i32 {
    if !self.has_capability(CAP_FS_WRITE) {
        return -EROFS; // Read-only file system
    }
    // ...
}
```

降级模式下允许的操作:
- `read()` — 从 ARC 或磁盘读取
- `stat()` — 查询元数据
- `snapshot_create()` — 创建只读快照
- `clone_create()` — 从快照创建可写克隆

降级模式下禁止:
- `write()` / `truncate()` — 返回 EROFS
- `unlink()` / `mkdir()` / `rmdir()` — 返回 EROFS
- `format_disk()` — 返回 EROFS

### 9.3 自愈条件

```rust
// 恢复条件: 降级后连续 1 个 barrier 周期无 panic
fn tick() {
    for domain in domains {
        if domain.is_degraded() && tick - domain.last_rollback_time >= BARRIER_INTERVAL {
            domain.mark_recovered();
            // state = Active, cap_mask 恢复, failures = 0
        }
    }
}
```

### 9.4 确定性重放检测

```rust
// 四层防御 - 第二层
let prev_fp = self.last_crash_fingerprint.swap(crash_fingerprint);
if crash_fingerprint != 0 && prev_fp == crash_fingerprint {
    // 相同 fingerprint 出现两次 → 确定性故障, 直接隔离
    self.consecutive_failures.store(MAX_CONSECUTIVE_FAILURES);
    self.state.store(DomainState::Quarantined);
    return false; // 拒绝回滚, 避免无限循环
}
```

---

## 10. ZIL 与 Barrier 握手协议

### 10.1 故障分类与恢复路径

| 故障类型 | 恢复路径 | 说明 |
|----------|---------|------|
| 逻辑 panic (指针访问) | Barrier UndoLog | 内存回滚，无需 ZIL |
| write() 返回后 panic | Barrier + ZIL | 可能需重放 ZIL |
| 断电/硬重启 | ZIL replay | Barrier 在内存中丢失 |
| TXG syncing 中 panic | Barrier + ZIL | 部分 TXG 需丢弃 |
| 连续 panic (≥5) | 域隔离 | HzFS 降级为只读 |

### 10.2 重启恢复流程

```
mount_disk():
  1. 从磁盘加载 uberblock → txg = N, root_bp = X
  2. 扫描 ZIL records → 找到 txg > N 的记录
     ├─ Write(txg=N+1, obj_id=A, offset=0, count=4096)
     ├─ Create(txg=N+1, parent=0, "new_file")
     └─ ...
  3. ZIL replay → 逐条重做
     ├─ 验证 checksum
     ├─ 重新分配块 (可能分配新 LBA)
     ├─ 应用写入
     └─ 标记为 replayed
  4. 初始化 Barrier 域
     ├─ barrier_generation = 0 (全新)
     ├─ capture_cb 绑定
     └─ 此时不能回滚到 "断电前的 Barrier 状态"
         (因为 Barrier 是纯内存, 断电后已丢失)
  5. HzFS 进入 Active 状态
```

### 10.3 ZIL 毒记录处理

```
ZIL 重放中遇到格式错误的记录:
  → 触发 panic
  → Barrier 接收 panic
  → crash_fingerprint = hash("ZIL replay error at record N")
  → 如果同一个 ZIL 记录连续两次触发相同 panic
  → 四层防御第二层: 确定性重放检测 → 域隔离
  → 避免无限重启循环
```

---

## 11. UndoLog 容量规划

### 11.1 容量分析

| 操作 | Undo 条目数 | 100 tick 内最坏次数 | 最坏条目数 |
|------|-----------|-------------------|-----------|
| write() | 2 (bp + size) | 200 | 400 |
| create_file() | 3 (obj_id + obj + zap) | 20 | 60 |
| unlink() | 3 (obj + zap + bp) | 20 | 60 |
| txg transition | 2 (state + current) | 1 | 2 |
| ARC eviction | 1 (p值) | 100 | 100 |
| Metaslab alloc | 1 (bitmap word) | 100 | 100 |
| **合计** | | | **722** |

### 11.2 缓解策略

**策略1**: 增大 MAX_UNDO_ENTRIES → 1024 (40KB 开销)

**策略2**: HzFS 域使用更小的 barrier_interval (50 ticks)，减少每代际 mutation

**策略3**: emergency_compact 保留最近 4 代际，丢失更早代际时触发全量 `restore_from_snapshot`

**推荐**: 策略1 + 策略2 组合。MAX_UNDO_ENTRIES=1024, barrier_interval=50。

---

## 12. 实施路线图

### Phase A: Barrier 集成基础 (~475 行)

| 文件 | 内容 | 行数 |
|------|------|------|
| hzfs.rs | register_barrier_domain + capture_cb + rollback_cb | 250 |
| hzfs.rs | check_capability 包装器 | 50 |
| arc.rs | undo record on p adjustment | 30 |
| txg.rs | undo record on state transition | 40 |
| spa.rs | undo record on alloc/free bitmap | 40 |
| dataset.rs | undo record on zap modification | 30 |
| zil.rs | undo record on committed_seq | 20 |
| lib.rs | 调用 hzfs_register_barrier_domain() | 10 |
| mod.rs | 公开 register 函数 | 5 |

### Phase B: 混合引擎增强 (~880 行)

| 文件 | 内容 | 行数 |
|------|------|------|
| txg.rs | 延迟分配队列 + 区间合并 | 150 |
| spa.rs | allocate_contiguous + free_deferred | 80 |
| bp.rs | ExtentEntry + 双模式 BP | 200 |
| zap.rs | HTree 目录索引 | 300 |
| hzfs.rs | scrub 孤儿块扫描入口 | 150 |

### Phase C: 深度测试 (~500 行)

| 测试 | 内容 | 行数 |
|------|------|------|
| test_barrier_hzfs.c | Barrier 域 6 回滚测试 | 150 |
| test_delayed_alloc.c | 延迟分配 + TXG 一致性 | 150 |
| test_scrub.c | 孤儿块回收 | 100 |
| test_zil_replay.c | ZIL 重放 + Barrier 握手 | 100 |

---

## 13. 测试矩阵

### 13.1 功能测试

| 测试场景 | 验证目标 |
|----------|---------|
| write 后立即 panic | UndoLog 回滚 obj.size, bp 不变 |
| TXG sync 中 panic | extent_tree 回滚, 位图恢复 |
| 连续 panic ×5 | 域隔离, HzFS 降级为只读 |
| 降级后 read | 返回正确数据 |
| 降级恢复 | 连续 50 ticks 无 panic → 恢复 |
| 断电重启 | ZIL replay → 无 Barrier 介入 |
| VFS panic 级联 | VFS 回滚 → BFS → HzFS 回滚 → FD 修复 |
| 确定性重放 | 相同 fingerprint → 直接隔离 |
| undo log 满 | emergency_compact → 保留最近 4 代际 |
| 孤儿块回收 | mount 后 scrub → 释放无引用块 |

### 13.2 压力测试

| 测试 | 参数 | 验证 |
|------|------|------|
| 200 并发 write × 100 tick | undo 条目 > 1024 | emergency_compact 触发 |
| 1000 文件目录 | HTree 深度 < 4 | O(log n) 性能 |
| 1GB 连续写入 | 单 Extent 条目 | 250,000x metadata 缩减 |
| 50 次连续 panic | 自愈循环 | 降级 → 恢复 → 正常 |

---

## 附录 A: Barrier Stack API 速查

| 函数 | 签名 | 说明 |
|------|------|------|
| `recovery_domain_register` | `fn(id: u64) -> i32` | 注册恢复域 |
| `recovery_domain_set_cbs` | `fn(id, capture, rollback) -> i32` | 设置回调 |
| `recovery_undo_record` | `fn(id, ptr, old_val) -> i32` | 记录撤销条目 |
| `recovery_domain_add_dep` | `fn(id, dep_id) -> i32` | 添加域依赖 |
| `recovery_test_rollback` | `fn(id, fingerprint) -> i32` | 触发测试回滚 |
| `recovery_barrier_maintenance` | `fn()` | 调度器 tick 驱动 |

## 附录 B: HzFS FFI API

| 函数 | 说明 |
|------|------|
| `hzfs_init()` | 初始化 HzFS + 注册 Barrier 域 |
| `hzfs_mount()` | 挂载 |
| `hzfs_format()` | 格式化磁盘 |
| `hzfs_open(path, flags, pwid)` | 打开文件 |
| `hzfs_close(fd)` | 关闭 |
| `hzfs_read(fd, buf, count)` | 读取 |
| `hzfs_write(fd, buf, count)` | 写入 |
| `hzfs_mkdir(path, pwid)` | 创建目录 |
| `hzfs_unlink(path, pwid)` | 删除文件 |
| `hzfs_sync()` | 同步到磁盘 |
| `hzfs_snapshot_create(name)` | 创建快照 |
| `hzfs_is_initialized()` | 查询初始化状态 |

## 附录 C: 相关源文件索引

| 文件 | 说明 |
|------|------|
| [src/kernel/fs/hzfs/hzfs.rs](file:///home/anfer/Code/AntX/src/kernel/fs/hzfs/hzfs.rs) | HzFS 主模块 |
| [src/kernel/fs/hzfs/spa.rs](file:///home/anfer/Code/AntX/src/kernel/fs/hzfs/spa.rs) | SPA 存储池 |
| [src/kernel/fs/hzfs/txg.rs](file:///home/anfer/Code/AntX/src/kernel/fs/hzfs/txg.rs) | TXG 事务组 |
| [src/kernel/fs/hzfs/arc.rs](file:///home/anfer/Code/AntX/src/kernel/fs/hzfs/arc.rs) | ARC 缓存 |
| [src/kernel/fs/hzfs/dmu.rs](file:///home/anfer/Code/AntX/src/kernel/fs/hzfs/dmu.rs) | DMU Object |
| [src/kernel/fs/hzfs/zap.rs](file:///home/anfer/Code/AntX/src/kernel/fs/hzfs/zap.rs) | ZAP 属性 |
| [src/kernel/fs/hzfs/zil.rs](file:///home/anfer/Code/AntX/src/kernel/fs/hzfs/zil.rs) | ZIL 日志 |
| [src/kernel/fs/hzfs/snapshot.rs](file:///home/anfer/Code/AntX/src/kernel/fs/hzfs/snapshot.rs) | 快照管理 |
| [src/kernel/barrier/types.rs](file:///home/anfer/Code/AntX/src/kernel/barrier/types.rs) | Barrier 类型 |
| [src/kernel/barrier/domain.rs](file:///home/anfer/Code/AntX/src/kernel/barrier/domain.rs) | 恢复域 |
| [src/kernel/barrier/undo_log.rs](file:///home/anfer/Code/AntX/src/kernel/barrier/undo_log.rs) | 撤销日志 |
| [src/kernel/barrier/manager.rs](file:///home/anfer/Code/AntX/src/kernel/barrier/manager.rs) | 恢复管理器 |
| [src/kernel/barrier/ffi.rs](file:///home/anfer/Code/AntX/src/kernel/barrier/ffi.rs) | Barrier FFI |
