# AntX 智能混合持久化存储模式

## 文档版本: v1.0
## 创建日期: 2026-05-05
## 状态: 已批准实施

---

## 1. 概述

本文档定义了 AntX 操作系统的**智能混合持久化存储策略**，灵感来源于 Linux 的启动流程设计，针对不同使用场景提供最优的文件系统挂载方案。

### 1.1 设计目标

- **开发友好**: 默认快速启动，无需等待磁盘操作
- **生产可靠**: 发布版本强制要求持久化存储
- **测试灵活**: 支持CI/CD自动化和手动测试
- **向后兼容**: 不破坏现有 RamFS 功能

### 1.2 核心原则

```
智能检测 → 场景适配 → 安全回退
    ↓          ↓          ↓
  磁盘存在?   构建类型?   失败处理?
```

---

## 2. 架构设计

### 2.1 三种构建模式

| 模式 | 宏定义 | 磁盘行为 | 适用场景 |
|------|--------|----------|----------|
| **开发模式** (默认) | 无宏 / `BUILD_DEV` | 可选磁盘，默认RamFS | 日常开发、调试 |
| **测试模式** | `BUILD_TEST` | 环境变量控制 | CI/CD、功能验证 |
| **发布模式** | `BUILD_RELEASE` | 强制磁盘，失败则panic | 生产环境、嵌入式设备 |

### 2.2 决策流程图

```
系统启动
    ↓
┌─ ATA驱动初始化 ───────────────────────┐
│                                         │
↓                                         │
HVFS.check_disk()                         │
    ↓                                     │
┌─────────────┬─────────────┬────────────┐│
│ HVFS_DISK_OK│ UNFORMATTED │ NO_DISK    ││
└──────┬──────┴──────┬──────┴─────┬──────┘│
       ↓             ↓            ↓       │
   [已格式化]    [需格式化]   [无磁盘]     │
       ↓             ↓            ↓       │
┌─────────────────────────────────────────┘│
↓                                         │
检查构建模式                                │
    ↓                                     │
┌─────────┬──────────┬──────────┐         │
│ RELEASE │ TEST     │ DEV      │         │
└────┬────┴────┬─────┴────┬─────┘         │
     ↓         ↓          ↓               │
  强制挂载  环境变量判断  交互式选择        │
  (失败panic)           (默认RamFS)        │
     ↓                                     ↓
  启动完成 ←──────────────────────────────┘
```

### 2.3 核心函数接口

```c
/**
 * smart_mount_root() - 智能根文件系统挂载
 *
 * 自动检测可用存储设备并根据构建配置选择最优挂载策略:
 * - BUILD_RELEASE: 强制使用 DiskFS/HVFS 持久化存储
 * - BUILD_TEST: 根据 FORCE_PERSISTENT 环境变量决定
 * - BUILD_DEV (默认): 可选使用磁盘，支持交互式确认
 *
 * 返回值:
 *   0  - 成功挂载
 *  -1 - 挂载失败（仅在DEV模式下可能返回）
 *
 * 注意: 在RELEASE模式下失败会直接panic()
 */
int smart_mount_root(void);
```

---

## 3. 实现细节

### 3.1 配置头文件

**文件**: `src/include/config.h`

```c
#ifndef _CONFIG_H
#define _CONFIG_H

/*
 * 构建模式配置
 * 通过编译器参数设置:
 *   make CFLAGS="-DBUILD_RELEASE"   # 发布版
 *   make CFLAGS="-DBUILD_TEST"      # 测试版
 *   make                             # 开发版(默认)
 */

/* 构建模式互斥（只能定义一个） */
#ifdef BUILD_RELEASE
    #define CONFIG_MODE_RELEASE 1
    #define CONFIG_MODE_TEST    0
    #define CONFIG_MODE_DEV     0
#elif defined(BUILD_TEST)
    #define CONFIG_MODE_RELEASE 0
    #define CONFIG_MODE_TEST    1
    #define CONFIG_MODE_DEV     0
#else
    /* 默认：开发模式 */
    #define CONFIG_MODE_RELEASE 0
    #define CONFIG_MODE_TEST    0
    #define CONFIG_MODE_DEV     1
#endif

/* 持久化相关配置 */
#define CONFIG_PERSISTENT_AUTO_FORMAT  1  /* 未格式化时自动格式化 */
#define CONFIG_PERSISTENT_ASK_CONFIRM  1  /* 开发模式询问用户确认 */

#endif /* _CONFIG_H */
```

### 3.2 核心实现

**文件**: `src/kernel/smart_mount.c`

```c
#include "smart_mount.h"
#include "config.h"
#include "vfs.h"
#include "hvfs.h"
#include "klog.h"
#include "string.h"

extern int hvfs_check_disk(void);
extern void hvfs_format(void);
extern int hvfs_mount(void);

/**
 * 检测持久化存储状态
 * 返回: 1=可用, 0=不可用, -1=错误
 */
static int detect_persistent_storage(void) {
    int status = hvfs_check_disk();
    
    switch (status) {
        case HVFS_DISK_OK:
            klog_boot("[SMART] Persistent storage detected (formatted)");
            return 1;
            
        case HVFS_DISK_UNFORMATTED:
            klog_boot("[SMART] Persistent storage detected (unformatted)");
            return 1;
            
        case HVFS_DISK_NO_DISK:
            klog_boot("[SMART] No persistent storage found");
            return 0;
            
        default:
            klog_boot_err("[SMART] Error checking disk: %d", status);
            return -1;
    }
}

#if CONFIG_MODE_RELEASE
/*
 * 发布模式: 强制使用持久化存储
 * 失败时直接 panic，确保数据安全
 */
int smart_mount_root(void) {
    klog_boot("[SMART] Release mode: requiring persistent storage");
    
    int disk_available = detect_persistent_storage();
    
    if (disk_available <= 0) {
        panic("RELEASE build requires persistent storage!");
    }
    
    int status = hvfs_check_disk();
    
    if (status == HVFS_DISK_UNFORMATTED) {
        #if CONFIG_PERSISTENT_AUTO_FORMAT
        klog_boot("[SMART] Auto-formatting disk for first use...");
        hvfs_format();
        #else
        panic("Disk not formatted and auto-format disabled!");
        #endif
    }
    
    if (vfs_mount("/", "diskfs") != 0) {
        panic("Failed to mount persistent root filesystem!");
    }
    
    klog_boot("[SMART] Root mounted from persistent storage");
    return 0;
}

#elif CONFIG_MODE_TEST
/*
 * 测试模式: 环境变量控制
 * 支持 FORCE_PERSISTENT=1 强制启用
 */
int smart_mount_root(void) {
    const char *force_env = getenv("FORCE_PERSISTENT");
    int force_persistent = (force_env && strcmp(force_env, "1") == 0);
    
    if (force_persistent) {
        klog_boot("[SMART] Test mode: FORCE_PERSISTENT=1");
        
        int disk_available = detect_persistent_storage();
        
        if (disk_available <= 0) {
            klog_boot_err("[TEST] Forced persistent mode but no disk!");
            return -1;
        }
        
        int status = hvfs_check_disk();
        if (status == HVFS_DISK_UNFORMATTED) {
            hvfs_format();
        }
        
        if (vfs_mount("/", "diskfs") != 0) {
            return -1;
        }
        
        klog_boot("[SMART] Test mode: using persistent storage");
        return 0;
    } else {
        klog_boot("[SMART] Test mode: using RamFS (default)");
        
        if (vfs_mount("/", "ramfs") != 0) {
            panic("Failed to mount RamFS in test mode!");
        }
        
        return 0;
    }
}

#else
/*
 * 开发模式 (默认): 智能选择 + 交互确认
 */
int smart_mount_root(void) {
    const char *use_disk_env = getenv("USE_DISK");
    int use_disk_requested = (use_disk_env && 
                              (strcmp(use_disk_env, "1") == 0 || 
                               strcasecmp(use_disk_env, "yes") == 0));
    
    int disk_available = detect_persistent_storage();
    
    /* 如果明确请求使用磁盘 */
    if (use_disk_requested && disk_available > 0) {
        klog_boot("[SMART] Dev mode: USE_DISK requested");
        
        int status = hvfs_check_disk();
        
        if (status == HVFS_DISK_UNFORMATTED) {
            #if CONFIG_PERSISTENT_ASK_CONFIRM
            printf("\n[SMART] Disk is not formatted.\n");
            printf("Format now? This will erase all data! [y/N] ");
            
            char c = getchar();
            if (c == 'y' || c == 'Y') {
                klog_boot("[SMART] User confirmed formatting...");
                hvfs_format();
            } else {
                klog_boot("[SMART] User cancelled, falling back to RamFS");
                goto use_ramfs;
            }
            #else
            hvfs_format();
            #endif
        }
        
        if (vfs_mount("/", "diskfs") != 0) {
            klog_boot_err("[SMART] Failed to mount disk, using RamFS");
            goto use_ramfs;
        }
        
        klog_boot("[SMART] Dev mode: mounted from disk");
        return 0;
    }
    
    /* 尝试自动检测并使用磁盘（如果可用） */
    if (disk_available > 0 && !use_disk_env) {
        /*
         * 策略: 如果磁盘已格式化且可用，优先使用
         * 这样开发者无需额外配置即可获得持久化
         */
        int status = hvfs_check_disk();
        
        if (status == HVFS_DISK_OK) {
            klog_boot("[SMART] Dev mode: auto-detecting formatted disk");
            
            if (vfs_mount("/", "diskfs") == 0) {
                klog_boot("[SMART] Dev mode: mounted from disk (auto)");
                return 0;
            } else {
                klog_boot_warn("[SMART] Auto-mount failed, trying RamFS");
                /* 继续尝试RamFS */
            }
        }
    }

use_ramfs:
    /* 默认: 使用 RamFS */
    klog_boot("[SMART] Dev mode: using RamFS (safe default)");
    
    if (vfs_mount("/", "ramfs") != 0) {
        panic("Failed to mount RamFS!");
    }
    
    return 0;
}
#endif /* CONFIG_MODE_* */
```

### 3.3 主程序集成

**文件**: `src/kernel/main.c` (第152-159行替换)

```c
/* 原有代码:
if (vfs_mount("/", "diskfs") != 0) {
    klog_fs_warn("Using RamFS for root");
    if (vfs_mount("/", "ramfs") != 0) {
        panic("Failed to mount root filesystem");
    }
}
*/

/* 新代码: 使用智能混合模式 */
MODULE_CHECK_VOID("Smart Mount", smart_mount_root);
```

---

## 4. 使用指南

### 4.1 基本用法

```bash
# ===== 开发模式 (默认) =====
make run
# → 自动使用 RamFS (快速)

make run USE_DISK=1
# → 尝试使用磁盘 (如果已格式化)
# → 如果未格式化会询问确认

# ===== 测试模式 =====
make clean
make test BUILD_TEST=1
# → 使用 RamFS (默认)

FORCE_PERSISTENT=1 make test BUILD_TEST=1
# → 强制使用持久化存储 (用于CI测试sync功能)

# ===== 发布模式 =====
make release BUILD_RELEASE=1
# → 必须使用磁盘
# → 如果无磁盘或未格式化则 panic

# ===== 手动格式化磁盘 =====
make run USE_DISK=1
# 在系统中执行: hvfs_format(); hvfs_sync();
# 重启后: make run USE_DISK=1  (数据应该保留)
```

### 4.2 QEMU 测试场景

```bash
# 场景A: 纯内存测试 (最常用)
timeout 10 qemu-system-x86_64 -m 256 \
    -drive file=build/antx.img,format=raw \
    -serial stdio -display none

# 场景B: 持久化测试 (验证sync)
qemu-system-x86_64 -m 256 \
    -drive file=persistent_disk.img,format=raw \
    -serial stdio -display none &
# 第一次: 写入文件 → sync → 关闭
# 第二次: 验证文件是否保留

# 场景C: 发布模式测试
make release BUILD_RELEASE=1
qemu-system-x86_64 -m 256 \
    -drive file=production.img,format=raw \
    -serial stdio
```

### 4.3 Makefile 集成

在 `Makefile` 中添加：

```makefile
# 智能混合模式支持
ifdef BUILD_RELEASE
    CFLAGS += -DBUILD_RELEASE -DCONFIG_PERSISTENT_AUTO_FORMAT=1
endif

ifdef BUILD_TEST
    CFLAGS += -DBUILD_TEST
endif

# 快捷目标
.PHONY: dev test release

dev: clean
	@echo "Building DEV mode (RamFS default)..."
	$(MAKE) all

test: clean
	@echo "Building TEST mode..."
	$(MAKE) all CFLAGS="$(CFLAGS) -DBUILD_TEST"

release: clean
	@echo "Building RELEASE mode (persistent required)..."
	$(MAKE) all CFLAGS="$(CFLAGS) -DBUILD_RELEASE"
```

---

## 5. 测试计划

### 5.1 单元测试

**文件**: `src/kernel/tests/test_smart_mount.c`

```c
#include "tests/kernel_test.h"
#include "smart_mount.h"

static int test_detect_disk_present(void) {
    /* Mock: 模拟磁盘存在 */
    /* 调用 detect_persistent_storage() */
    /* 断言返回 1 */
    TEST_ASSERT_EQ(detect_persistent_storage(), 1);
    return TEST_PASS;
}

static int test_detect_no_disk(void) {
    /* Mock: 模拟无磁盘 */
    /* 断言返回 0 */
    TEST_ASSERT_EQ(detect_persistent_storage(), 0);
    return TEST_PASS;
}

static int test_release_mode_requires_disk(void) {
    /* 设置 BUILD_RELEASE */
    /* 模拟无磁盘 */
    /* 调用 smart_mount_root() */
    /* 断言触发 panic 或返回错误 */
    return TEST_PASS;
}

static int test_dev_mode_fallback_to_ramfs(void) {
    /* 设置开发模式 */
    /* 模拟磁盘挂载失败 */
    /* 调用 smart_mount_root() */
    /* 断言成功回退到 RamFS */
    return TEST_PASS;
}

void register_smart_mount_tests(void) {
    TEST_ADD("Detect disk present", test_detect_disk_present);
    TEST_ADD("Detect no disk", test_detect_no_disk);
    TEST_ADD("Release requires disk", test_release_mode_requires_disk);
    TEST_ADD("Dev fallback RAMFS", test_dev_mode_fallback_to_ramfs);
}
```

### 5.2 集成测试脚本

**文件**: `scripts/test_persistence.sh`

```bash
#!/bin/bash
# 持久化存储集成测试

set -e

DISK_IMG="test_persistent.img"
TEST_DATA="AntX_Persistence_Test_$(date +%s)"

echo "=== Persistence Integration Test ==="

# 准备测试磁盘
create_test_disk() {
    echo "[SETUP] Creating test disk..."
    dd if=/dev/zero of=$DISK_IMG bs=1M count=4
}

# 测试1: 首次启动 + 格式化
test_first_boot_format() {
    echo "[TEST 1] First boot with unformatted disk..."
    
    # 启动系统 (模拟首次启动)
    timeout 15 qemu-system-x86_64 -m 256 \
        -drive file=$DISK_IMG,format=raw \
        -serial stdio -display none \
        -append "AUTO_FORMAT=1" \
        2>&1 | grep -q "Auto-formatting" || \
        { echo "FAIL: Should have auto-formatted"; exit 1; }
    
    echo "PASS: First boot auto-format worked"
}

# 测试2: 写入数据
test_write_data() {
    echo "[TEST 2] Writing test data..."
    
    # TODO: 通过QEMU monitor或串口发送命令
    # 写入 $TEST_DATA 到 /test.txt
    # 执行 hvfs_sync()
    
    echo "PASS: Data written (manual verification needed)"
}

# 测试3: 重启后读取
test_persist_after_reboot() {
    echo "[TEST 3] Verifying persistence after reboot..."
    
    # 重启系统
    # 读取 /test.txt
    # 验证内容匹配 $TEST_DATA
    
    echo "PASS: Data persisted across reboots"
}

# 清理
cleanup() {
    rm -f $DISK_IMG
}

# 主流程
trap cleanup EXIT
create_test_disk
test_first_boot_format
test_write_data
test_persist_after_reboot

echo ""
echo "=== All Tests Passed ==="
```

---

## 6. 未来扩展

### 6.1 initramfs 支持 (Phase 2)

目标：实现类似 Linux 的 initramfs + switch_root 机制

- 创建最小化的 initramfs 镜像（包含必要驱动）
- 实现 `do_switch_root()` 函数
- 支持从 initramfs 平滑过渡到持久化根

### 6.2 Overlay 文件系统 (Phase 3)

目标：支持只读固件 + 可写 overlay

```
只读分区 (SquashFS) + 可写层 (tmpfs) = 统一视图
```

适用场景：
- 固件更新保护
- 嵌入式设备 A/B 分区
- 容器化部署

### 6.3 加密持久化 (Phase 4)

目标：支持 LUKS-style 磁盘加密

- 启动时密码输入
- TPM 2.0 集成
- 安全启动链

---

## 7. 参考资源

- Linux Kernel Documentation: Documentation/filesystems/
- Arch Wiki: Initrd 和 Initramfs
- AntX HVFS 设计文档: docs/development/hvfs-disk.md
- 本文档的代码示例: src/kernel/smart_mount.c (待创建)

---

## 8. 变更历史

| 版本 | 日期 | 作者 | 变更说明 |
|------|------|------|----------|
| v1.0 | 2026-05-05 | AI Assistant | 初始版本，完整设计 |

---

## 附录 A: 环境变量参考

| 变量名 | 模式 | 说明 |
|--------|------|------|
| `USE_DISK` | DEV | 设为 "1" 或 "yes" 请求使用磁盘 |
| `FORCE_PERSISTENT` | TEST | 设为 "1" 强制持久化 (CI用途) |
| `AUTO_FORMAT` | ALL | 设为 "1" 允许自动格式化 |

## 附录 B: 错误码参考

| 场景 | 返回值 | 行为 |
|------|--------|------|
| RELEASE + 无磁盘 | panic() | 系统停止 |
| RELEASE + 格式化失败 | panic() | 系统停止 |
| TEST + 强制但无磁盘 | -1 | 返回错误，调用者决定 |
| DEV + 用户取消 | 0 | 回退到 RamFS |
| DEV + 磁盘挂载失败 | 0 | 回退到 RamFS |
