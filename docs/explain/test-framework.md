# 测试框架

> AntX的四级测试策略与框架使用

---

## 🎯 测试策略

AntX采用四级测试策略，确保代码质量和系统稳定性：

```
┌─────────────────────────────────────────┐
│  4. 混沌测试 (Chaos Test)              │  ← 故障注入与恢复
├─────────────────────────────────────────┤
│  3. 压力测试 (Stress Test)             │  ← 高负载、边界条件
├─────────────────────────────────────────┤
│  2. 集成测试 (Integration Test)        │  ← 子系统间交互
├─────────────────────────────────────────┤
│  1. 单元测试 (Unit Test)               │  ← 单个函数/模块
└─────────────────────────────────────────┘
```

---

## 🧪 1. 单元测试

### 概述

- **测试范围**: 单个函数、结构、模块
- **测试数量**: 256个
- **测试覆盖率**: 90%+
- **执行时间**: < 2分钟

### 测试框架

**Rust测试框架**:

```rust
// src/kernel/tests/mod.rs
pub struct TestRunner;

impl TestRunner {
    pub fn run_all() -> TestResult {
        let mut passed = 0;
        let mut failed = 0;
        
        for test in TEST_REGISTRY.iter() {
            match test.run() {
                Ok(_) => passed += 1,
                Err(_) => failed += 1,
            }
        }
        
        TestResult { passed, failed }
    }
}
```

**测试宏**:

```rust
// 定义测试
#[test_case]
fn test_memory_allocation() {
    let ptr = kmalloc(1024);
    assert!(!ptr.is_null());
    kfree(ptr);
}

// 使用check宏
check!(result == expected, "description");
```

### 运行单元测试

```bash
make test-unit
```

**输出示例**:

```
╔══════════════════════════════════════════════╗
║     Building & Running Unit Tests             ║
╚══════════════════════════════════════════════╝

[1/256] memory::pmm::alloc_free...PASS
[2/256] memory::pmm::alloc_contiguous...PASS
[3/256] memory::vmm::map_page...PASS
...
[256/256] barrier::parallel::compute_layers...PASS

========================================
  RESULT: ALL 256 TESTS PASSED (0 skipped)
========================================
```

---

## 🔗 2. 集成测试

### 概述

- **测试范围**: 子系统间交互
- **测试数量**: 7个测试套件
- **执行时间**: < 1分钟

### 测试套件

| 套件 | 说明 | 测试内容 |
|------|------|---------|
| Boot Sequence | 启动序列 | 内核启动和初始化 |
| Memory Subsystem | 内存子系统 | PMM + VMM + kmalloc集成 |
| Filesystem Mount | 文件系统挂载 | VFS + RamFS + DevFS + ProcFS |
| Process & Scheduler | 进程与调度 | 进程管理器 + 调度器集成 |
| Security Subsystem | 安全子系统 | PWID + Session集成 |
| Barrier Subsystem | 栏栈子系统 | 故障恢复框架 |
| No Unresolved Panics | 无未解决Panic | 所有panic已恢复 |

### 运行集成测试

```bash
make test-integration
```

**输出示例**:

```
╔══════════════════════════════════════════════════════════╗
║     Integration Tests                                   ║
╚══════════════════════════════════════════════════════════╝

[PASS] Boot Sequence: Kernel boot and initialization
[PASS] Memory Subsystem: PMM + VMM + kmalloc integration
[PASS] Filesystem Mount: VFS + RamFS + DevFS + ProcFS mounting
[PASS] Process & Scheduler: Process manager + scheduler integration
[PASS] Security Subsystem: PWID + Session integration
[PASS] Barrier Subsystem: Fault recovery framework
[PASS] No Unresolved Panics: All panics should be recovered

============================================================
Integration Tests: 7 passed, 0 failed
============================================================
```

---

## 💪 3. 压力测试

### 概述

- **测试范围**: 高负载、边界条件
- **测试数量**: 5个测试场景
- **执行时间**: ~3分钟

### 测试场景

| 场景 | 说明 | 参数 |
|------|------|------|
| Memory Pressure | 内存压力 | 128MB |
| Low Memory Boot | 低内存启动 | 64MB |
| Extended Stability | 扩展稳定性 | 60秒 |
| SMP Stability | SMP稳定性 | 2核心，30秒 |
| Rapid Reboot Cycle | 快速重启循环 | 3次重启 |

### 运行压力测试

```bash
make test-stress
```

**输出示例**:

```
╔══════════════════════════════════════════════════════════╗
║     Stress Tests                                        ║
╚══════════════════════════════════════════════════════════╝

[Memory Pressure (128MB)]
  [PASS] Kernel handled 128MB memory (subsystems: 4/10 OK)

[Low Memory Boot (64MB)]
  [PASS] Booted with 64MB (subsystems: 4/10 OK)

[Extended Stability (60s)]
  [PASS] Stable for 60 seconds

[SMP Stability (2 cores)]
  [PASS] 2-core SMP stable for 30 seconds

[Rapid Reboot Cycle]
  [PASS] 3 consecutive boots all stable

============================================================
Stress Tests: 5 passed, 0 failed
============================================================
```

---

## 🌀 4. 混沌测试

### 概述

- **测试范围**: 故障注入与恢复
- **故障率**: 可配置（默认5%）
- **执行时间**: < 2分钟

### 故障注入类型

- 内存分配失败
- 设备I/O错误
- 中断风暴
- 随机Panic

### 运行混沌测试

```bash
# 默认故障率（5%）
make test-chaos

# 自定义故障率（10%）
make test-chaos FAULT_RATE=100
```

**输出示例**:

```
╔══════════════════════════════════════════════════════════╗
║     Chaos/Fault Injection Tests (fault_injection=on)   ║
║     FAULT_RATE=50/1000                        ║
╚══════════════════════════════════════════════════════════╝

Fault Injection:
  Injections triggered:  100
  Barrier captures:      95
  Undo rollbacks:        90
  Domain recoveries:     85
  Domain rollbacks:      85
  Quarantined domains:   5

Stability:
  Kernel panics:         0
  Triple faults:         0

Recovery Rate: 95%

============================================================
  Chaos Test: PASSED (Recovery rate >= 90%)
============================================================
```

---

## 📊 测试覆盖率

### 当前覆盖率

| 子系统 | 行覆盖率 | 分支覆盖率 | 函数覆盖率 |
|--------|---------|-----------|-----------|
| 内存管理 | 95% | 92% | 98% |
| 进程管理 | 90% | 88% | 95% |
| 文件系统 | 92% | 90% | 96% |
| 安全子系统 | 88% | 85% | 92% |
| 栏栈恢复 | 95% | 93% | 97% |
| 驱动框架 | 75% | 70% | 80% |
| 网络栈 | 70% | 65% | 75% |
| **总体** | **90%** | **87%** | **93%** |

### 生成覆盖率报告

```bash
# 生成详细覆盖率报告
make coverage

# 查看报告
open coverage/index.html
```

---

## 🔧 测试工具

### 测试宏

```rust
// 基本断言
check!(condition, "description");
check_eq!(actual, expected, "description");

// 性能测试
bench!("name", || {
    // 测试代码
}, 1000);  // 运行1000次

// 故障注入
inject_fault!(FaultType::MemoryAlloc);
```

### 测试桩

```c
// src/kernel/tests/test_hw_stubs.c
int ata_read_sector(u8 disk, u32 sector, u8 *buf) {
    // 模拟磁盘读取
    memset(buf, 0, 512);
    return 0;
}
```

### Mock对象

```rust
struct MockDevice {
    read_count: AtomicUsize,
    write_count: AtomicUsize,
}

impl Driver for MockDevice {
    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.read_count.fetch_add(1, Ordering::SeqCst);
        Ok(buf.len())
    }
}
```

---

## 📝 编写测试

### 单元测试示例

```rust
// src/kernel/tests/memory.rs
use crate::kernel::mem::*;

#[test_case]
fn test_pmm_alloc_free() {
    let page = pmm_alloc(1);
    assert!(!page.is_null());
    
    pmm_free(page, 1);
    // 验证页面已释放
}

#[test_case]
fn test_pmm_alloc_contiguous() {
    let pages = pmm_alloc(10);
    assert!(!pages.is_null());
    
    // 验证连续性
    for i in 0..10 {
        let page = unsafe { pages.add(i) };
        assert!(is_page_free(page));
    }
    
    pmm_free(pages, 10);
}
```

### 集成测试示例

```rust
// tests/integration/filesystem.rs
fn test_vfs_mount_unmount() {
    // 1. 挂载RamFS
    assert!(vfs_mount("/", FsType::RamFs).is_ok());
    
    // 2. 创建文件
    let fd = vfs_open("/test.txt", O_CREAT | O_WRONLY);
    assert!(fd >= 0);
    
    // 3. 写入数据
    let data = b"Hello, AntX!";
    assert_eq!(vfs_write(fd, data), data.len());
    
    // 4. 关闭文件
    assert!(vfs_close(fd).is_ok());
    
    // 5. 卸载文件系统
    assert!(vfs_unmount("/").is_ok());
}
```

---

## 📈 持续集成

### GitHub Actions

```yaml
name: Test

on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      
      - name: Install dependencies
        run: |
          sudo apt-get install nasm qemu-system-x86
          rustup default nightly
          rustup target add x86_64-unknown-none
      
      - name: Run tests
        run: make test-all
```

---

## 🐛 调试测试失败

### 查看失败日志

```bash
# 查看最新的测试日志
cat tests/reports/unit_test_*.log | grep FAIL

# 查看混沌测试日志
cat tests/reports/chaos_test_*.log
```

### 单独运行失败的测试

```bash
# 运行特定测试
make test-unit TEST_FILTER=test_memory_allocation
```

---

## 📚 最佳实践

1. **测试先行**: 编写代码前先写测试（TDD）
2. **小步提交**: 每次提交都确保测试通过
3. **覆盖边界**: 测试边界条件和异常情况
4. **性能测试**: 对关键路径进行性能测试
5. **定期回归**: 定期运行完整测试套件

---

**最后更新**: 2026-05-18
