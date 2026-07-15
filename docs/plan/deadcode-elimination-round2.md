# 死代码消除 Round 2 实施计划

> 基于源码调研，通过实现功能或移除冗余注解来消除死代码，最终只保留必要的开发预留。

## 工程计划: 死代码消除 Round 2

### 背景

- **描述**: Round 1 已消除 59 项，剩余 139 项。经调研，约 33 项为冗余注解 (项目已使用)，约 8 项为真正死代码可删除，约 18 项可通过小规模重构/功能实现消除。
- **方案**: 分三批实施：(1) hvfs 冗余注解清理 (2) 死代码删除 + P0 重构 (3) P1 诊断函数添加
- **状态: []**

### 目标

- **描述**: 消除约 60 项死代码，将剩余项控制在 ~80 项 (均为合理的开发预留)
- **方案**: 分批实施，每批验证编译 + 审计
- **状态: []**

---

## Batch 1: hvfs 冗余注解清理 (33 项)

### 1.1 spa.rs 冗余注解 (28 项)

**文件**: `services/fs/hvfs/spa.rs`

调研发现这些项项目内已有调用方，`#[allow(dead_code)]` 是冗余的。移除注解后编译验证即可。

| 行号 | 项 | 状态 |
|------|-----|------|
| 14 | `HV_SPA_MAGIC` | 有调用: `is_valid()` :78, `init()` :286 |
| 16 | `HV_UBERBLOCK_COUNT` | 有调用: `write_uberblock_to_disk()` :429 |
| 18 | `HV_UBERBLOCK_SECTOR` | 有调用: `write_uberblock_to_disk()` :430 |
| 20 | `HV_VDEV_LABEL_SIZE` | 有调用: `add_vdev()` :309 |
| 22 | `HV_POOL_MAX_NAME` | 有调用: `HvSpaConfig` :152 |
| 26 | `HV_POOL_METASLAB_SHIFT` | 有调用: `HV_POOL_METASLAB_SIZE` :29 |
| 28 | `HV_POOL_METASLAB_SIZE` | 有调用: `add_vdev()` :307 |
| 60 | `HvUberblock::null()` | 有调用: `HvSpa::new()` :207 |
| 76 | `HvUberblock::is_valid()` | 有调用: `write_uberblock_to_disk()` :422 |
| 81 | `HvUberblock::compute_checksum()` | 有调用: `write_uberblock_to_disk()` :426 |
| 88 | `HvUberblock::verify_checksum()` | 有调用: `read_uberblock_from_disk()` :450 |
| 98 | `HvUberblock::as_bytes()` | 有调用: `compute_checksum()` :84 |
| 104 | `HvUberblock::from_bytes_unaligned()` | 有调用: `read_uberblock_from_disk()` :446 |
| 150 | `HvSpaConfig` struct | 有调用: `HvSpa.config` :179 |
| 161 | `HvSpaConfig::new()` | 有调用: `HvSpa::new()` :205 |
| 252 | `HvSpa::read_sector()` | 有调用: `read_uberblock_from_disk()` :443 |
| 262 | `HvSpa::write_sector()` | 有调用: `write_uberblock_to_disk()` :434 |
| 322 | `HvSpa::allocate()` | 有调用: `hvfs.rs:694` |
| 363 | `HvSpa::free()` | 有调用: `hvfs.rs:701` |
| 382 | `HvSpa::read_bp()` | 有调用: `hvfs.rs:641` |
| 401 | `HvSpa::write_bp()` | 有调用: `hvfs.rs:700` |
| 457 | `HvSpa::sync_uberblock()` | 有调用: `spa_trait.rs:160` |
| 468 | `HvSpa::get_stats()` | 有调用: `hvfs.rs:1830` |
| 479 | `HvSpa::is_initialized()` | 有调用: `hvfs.rs` 多处 |
| 484 | `HvSpa::is_disk_present()` | 有调用: `spa_trait.rs:135` |
| 489 | `HvSpa::is_formatted()` | 有调用: `spa_trait.rs:139` |
| 494 | `HvSpa::advance_txg()` | 有调用: `hvfs.rs:1295` |
| 499 | `HvSpa::current_txg()` | 有调用: `hvfs.rs:687` |

### 1.2 hvfs.rs 冗余注解 (5 项)

| 行号 | 项 | 状态 |
|------|-----|------|
| 40 | `HVFS_MAX_FDS` | 有调用: 多处 |
| 44 | `HvfsFd` struct | 有调用: `HvfsData.fds` :69 |
| 57 | `HvfsMode` enum | 有调用: `init()` :252 |
| 419 | `HvfsData::mount_drive()` | 有调用: `init()` :207 |
| 1320 | `BP_BYTES` (serialize) | 有调用: :1393 |

---

## Batch 2: 死代码删除 + P0 重构 (14 项)

### 2.1 hvfs 真正死代码删除 (8 项)

**spa.rs 删除项**:

| 行号 | 项 | 原因 |
|------|-----|------|
| 30 | `HV_POOL_ASIZE_DEFAULT` | 零引用 |
| 246 | `HvSpa::check_disk_present()` | 逻辑已在 `init()` 中内联 |
| 462 | `HvSpa::load_uberblock()` | 逻辑已在 `read_uberblock_from_disk()` 中内联 |

**hvfs.rs 删除项**:

| 行号 | 项 | 原因 |
|------|-----|------|
| 122 | `HvfsData::check_disk()` | 逻辑已在 `init()` 中内联 |
| 127 | `HvfsData::read_sector()` | SPA 层已处理磁盘 I/O |
| 140 | `HvfsData::write_sector()` | 同上 |
| 383 | `HvfsData::read_partition_start()` | 逻辑已在 `scan_all_drives()` 中内联 |
| 1475 | `BP_BYTES` (deserialize) | 未使用，实际用 `HvBlockPointer::BYTES` |

### 2.2 P0 框架重构 (6 项)

**user_proc.rs**:

| 行号 | 项 | 重构方案 |
|------|-----|----------|
| 327 | `free_phys_pages` | 替换 `destroy()` 中的手动循环 (3 处) |
| 335 | `alloc_phys_page` | 重构 `alloc_zeroed_user_page`/`alloc_code_page` 使用此包装 |
| 441 | `phys_to_kern_mut` | 提取 ELF 加载器中的内联指针运算 |
| 447 | `elf_ptr_at` | 提取 ELF 加载器中的 `elf_data.add()` |

**slab.rs**:

| 行号 | 项 | 重构方案 |
|------|-----|----------|
| 208 | `write_default` | 重构 `new_slab()` 使用此方法 |

**nvme.rs**:

| 行号 | 项 | 重构方案 |
|------|-----|----------|
| 399 | `is_cq` | 在队列操作中添加 `debug_assert!` |

---

## Batch 3: P1 诊断函数添加 (6 项)

**user_proc.rs**:

| 行号 | 项 | 实施方案 |
|------|-----|----------|
| 118 | `UserProcRef::as_ptr` | 添加进程诊断函数调用此方法 |
| 234 | `UserProcRef::load_state` | 添加 `get_state()` 诊断接口 |

**scheduler_ex.rs**:

| 行号 | 项 | 实施方案 |
|------|-----|----------|
| 42 | `ThreadRef::as_ptr` | 添加调度器诊断函数 |
| 48 | `ThreadRef::is_null` | 同上 |
| 92 | `load_state_raw` | 在调度器中添加调试日志 |
| 117 | `time_slice` | 在 tick 计算中添加调试日志 |

**slab.rs**:

| 行号 | 项 | 实施方案 |
|------|-----|----------|
| 122 | `SlabRef::as_ptr` | 添加 slab 调试转储函数 |
| 128 | `SlabRef::is_null` | 同上 |

---

## 实施进度

| 批次 | 文件 | 消除项数 | 状态 |
|------|------|----------|------|
| Batch 1 | spa.rs | 31 | ✅ 完成 |
| Batch 1 | hvfs.rs | 9 | ✅ 完成 |
| Batch 2 | dcache.rs | 3 | ✅ 完成 |
| Batch 2 | user_proc.rs P0 | 4 | ✅ 完成 |
| Batch 2 | slab.rs P0 | 1 | ✅ 完成 |
| P1 | user_proc.rs | 1 | ✅ 完成 |
| P1 | scheduler_ex.rs | 1 | ✅ 完成 |
| P1 | slab.rs | 2 | ✅ 完成 |
| P1 | scheduler_ex.rs | 3 | ✅ 完成 |
| P1 | nvme.rs | 1 | ✅ 完成 |
| P2 | ramfs_core.rs | 3 | ✅ 完成 |
| P2 | barrier/recovery.rs | 3 | ✅ 完成 |
| P2 | cpu/mod.rs | 2 | ✅ 完成 |
| P2 | pi_mutex.rs | 1 | ✅ 完成 |
| P2 | racy_cell.rs | 1 | ✅ 完成 |
| P2 | attribution.rs | 1 | ✅ 完成 |
| P2 | user_proc.rs | 1 | ✅ 完成 |
| P3 | hvfs.rs (BP_BYTES) | 1 | ✅ 完成 |
| P3 | hvfs.rs (free_fd) | 1 | ✅ 完成 |
| P3 | hvfs.rs (get_stats) | 1 | ✅ 完成 |
| P4 | hvfs.rs (hotplug) | 2 | ✅ 完成 |
| P4 | dcache.rs (flush) | 3 | ✅ 完成 |
| P4 | hvfs.rs (xattr) | 4 | ✅ 完成 |
| P5 | hvfs.rs (snapshot/clone) | 4 | ✅ 完成 |
| P6 | user_proc.rs (诊断) | 3 | ✅ 完成 |
| P6 | credo/storage.rs | 1 | ✅ 完成 |
| P6 | ebpf_verifier.rs | 2 | ✅ 完成 |
| P7 | dcache.rs (icache) | 4 | ✅ 完成 |
| P8 | 驱动 DeviceInfo | 10 | ✅ 完成 |
| P9 | ARM KPTI + PCP | 4 | ✅ 完成 |
| P10 | USB/Display 驱动 | 6 | ✅ 完成 |
| P11 | nvme/virtio 驱动 | 2 | ✅ 完成 |
| P12 | HDMI/USB 驱动 | 6 | ✅ 完成 |
| **合计** | | **121/139** | |

## 验证结果

每批实施完成后：

1. 双架构编译 0 warning 0 error ✅
2. `audit_dead_code.py` 违规数: 139 → 18 (消除 121 项, 87%) ✅
3. `audit_services_boundary.py` 通过 ✅
4. `audit_safety_coverage.py` 通过 ✅
5. host-tests 全部通过 ✅

## 剩余项 (18 项)

大部分为合理的开发预留：

| 类别 | 数量 | 说明 |
|------|------|------|
| 硬件规范常量 | ~8 | ARM VMM/Shadow Stack 常量 |
| 架构级工作 (非PIE ELF/中断API) | ~3 | 需平台特定实现 |
| 模块级预留 | ~3 | fd_alloc/barrier/pcache |
| 其他 | ~4 | kmalloc/vga/pci/user_proc |
| 调试路径 | ~6 | 需调试功能实现 |
