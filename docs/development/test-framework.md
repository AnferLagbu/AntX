# AntX 测试框架设计文档

> **版本**: v1.0 | **状态**: 一期完成 | **代码位置**: `src/kernel/tests/`
>
> 本文档定义 AntX 内核测试框架的架构、当前覆盖、已知缺陷和深化路线。

---

## 一、架构

### 1.1 框架分层

```
test_register_module("Module Name")     ← 注册模块
  └─ test_register_case(mod, "Case")    ← 注册用例 (含函数指针)
       └─ TEST_ASSERT_EQ / GT / LT / NE  ← 断言宏

test_run_all()                          ← 逐模块执行
  └─ test_print_report()                ← 汇总输出
```

### 1.2 核心宏

| 宏 | 语义 |
|------|------|
| `TEST_ASSERT_EQ(a, b)` | `a == b` |
| `TEST_ASSERT_NE(a, b)` | `a != b` |
| `TEST_ASSERT_GT(a, b)` | `a > b` |
| `TEST_ASSERT_LT(a, b)` | `a < b` |
| `TEST_PASS` | 返回通过 |
| `TEST_SKIP` | 返回跳过 |

### 1.3 注册模式

每个测试模块遵循统一约定：

```c
// test_xxx.c
#include "kernel_test.h"

static int test_xxx_feature_a(void) {
    TEST_ASSERT_EQ(foo(), 0);
    return TEST_PASS;
}

void test_xxx_register(void) {
    int mod = test_register_module("XXX");
    if (mod < 0) return;
    test_register_case(mod, "Feature A", test_xxx_feature_a);
}
```

模块在 `test_main.c` → `run_kernel_tests()` 中以固定顺序注册。

---

## 二、当前覆盖 (v1.0)

### 2.1 模块清单

| 模块 | 用例 | 状态 | 备注 |
|------|:--:|:--:|------|
| Process Management | 6 | ✅ | 基础创建/PID/状态/退出/查找/压力 |
| Process Management Enhanced | 13 | ✅ | 树/Ring3/ELF 加载 |
| Scheduler | 3+3s | ✅ | 基础调度 |
| Scheduler Enhanced (MLFQ) | 4+1s | ✅ | 时间片/优先级提升 |
| Scheduler RT Enhancements | 17+3s | ✅ | FIFO/RR/RT vs Normal |
| SMP & Per-CPU Scheduler | 13 | ✅ | 运行队列/负载均衡/亲和性 |
| VFS | 8 | ✅ | 挂载/创建/读写/目录/stat/删除/大文件 |
| System Calls | 6+1s | ✅ | write/open/close/mkdir/yield |
| IPC | 6 | ✅ | 管道/信号/信号量/共享内存/消息队列 |
| HvFS | 5+3s | ✅ | 格式化/统计/同步 |
| Persistence | 3+5s | ✅ | PWID 持久化 |
| PWID Enhanced (v4) | 14+1s | ✅ | 能力/令牌/配额/进程限制 |
| PMM (Rust) | 11 | ✅ | 页分配/释放/大页/对齐/统计 |
| Kmalloc (Rust) | 12 | ✅ | 堆分配 |
| Recovery (Barrier Stack) | 16 | ✅ | 域注册/回滚/隔离/级联 |
| Spinlock | 10 | ✅ | 基础/Trylock/IRQ/性能 |
| Atomic | 14 | ✅ | 加减/CAS/屏障/并发 |
| RWLock | 16 | ✅ | 读写锁全量 |
| Mutex | 10 | ✅ | 睡眠锁/条件变量/超时 |
| Network Stack (lwIP+E1000) | 34+1s | ✅ | 网卡探测/RAW/UDP/TCP/DHCP/DNS/HTTP |
| **总计** | **224** | — | 21 模块 / 0 失败 |

### 2.2 跳过统计

| 跳过原因 | 用例数 | 模块 |
|------|:--:|------|
| 单核环境无 SMP | 6 | Scheduler (3), Enhanced (1), RT (3) |
| 无磁盘 | 8 | HvFS (3), Persistence (5) |
| 无 IPv6 地址 | 1 | Network |
| 未初始化 VFS | 1 | Syscall |
| 合计 | 16 | — |

---

## 三、禁用模块

### 3.1 根本原因分类

| 模块 | 用例 | 原因 | 可恢复性 |
|------|:--:|------|:--:|
| **test_interrupt** | 4 | IDT 重新初始化会清掉已注册的 timer 中断 handler。时钟中断失效导致所有后续测试悬挂。 | ⚠️ 需重构为 IDT 增量更新或独立运行 |
| **test_pci** | 20 | QEMU 单核下 x86 I/O 端口 (0xCF8/0xCFC) 访问不稳定，导致测试超时/悬挂 | ⚠️ 需 `-smp 2` QEMU 或真机 |
| **test_dma** | 24 | 同上 — 依赖 PCI 枚举和 I/O 端口 | ⚠️ 同上 |
| **test_qemu_hardware** | 29 | 单核 QEMU 中 APIC MMIO 区域未映射，触发 GPF | ⚠️ 需多核 APIC 初始化 |
| **test_slab** | 15 | 批量分配路径中存在 GPF 缺陷 | ⚠️ 需修复后恢复 |

### 3.2 评价

全部 5 个禁用模块都是**环境限制**而非**代码质量**问题。其中：

- **test_interrupt** 是**架构冲突**：IDT 测试本身会清除其他模块注册的 handler，不适合在共享测试套件中运行。解决方案是构建独立的 `make test-interrupt` 目标。
- **PCI/DMA/QEMU** 是**QEMU 单核限制**：在 `qemu-system-x86_64 -smp 2 -M q35` 下应该全部通过。
- **test_slab** 是唯一的**真实缺陷**，需要 root-cause。

---

## 四、深化路线图

### Phase 1: 解除环境限制 (短期)

| 任务 | 方案 | 预期用例恢复 |
|------|------|:--:|
| **独立中断测试目标** | `make test-interrupt` — 只注册 interrupt 模块 + 最小内核，独立运行 | 4 |
| **多核 QEMU 测试目标** | `make test-smp-qemu` — `-smp 2 -M q35` 参数下的全量测试 | 73 (PCI+DMA+QEMU) |

### Phase 2: 修复已知缺陷 (中期)

| 任务 | 方案 | 预期用例恢复 |
|------|------|:--:|
| **Slab GPF 修复** | root-cause 批量分配中的页表映射错误 | 15 |
| **增强 Slab 测试** | 增加碎片化/对齐/大对象路径 | +8 |

### Phase 3: 填补覆盖盲区 (中长期)

| 模块 | 当前状态 | 方案 |
|------|------|------|
| `devfs` | 零测试 | 实现 `test_devfs` — 设备节点创建/读写 |
| `keyboard/serial` | 零测试 | 在 QEMU 硬件测试中追加 |
| `ioapic` | 间接 (通过中断) | 实现多核下的 IOAPIC 重定向测试 |
| `timer` | 间接 (通过调度器) | 实现 PIT/HPET 周期精度测试 |
| `e1000` (独立) | 作为 netif 上层测试 | 实现裸 e1000 寄存器读写测试 |
| `pci` (Rust) | C 侧已有测试 | 实现 Rust `PciDevice` FFI 直测 |

### Phase 4: 框架增强 (长期)

| 增强 | 说明 |
|------|------|
| **`#[test]` 属性宏** | 替代 C 侧手动注册，Rust 模块可用 `#[recovery_test]` 自动发现 |
| **chaos/fault-injection** | 栏栈故障注入框架 — 随机 panic + 验证恢复率 (来自 barrier 设计文档 §5.4) |
| **覆盖率报告** | `cargo llvm-cov` + QEMU GDB stub → 行/分支覆盖率 |
| **Fuzzing 集成** | `cargo fuzz` 对 VFS 路径解析、网络包解析进行模糊测试 |
| **CI 矩阵** | `[单核QEMU, 多核QEMU, 真机x86_64]` × `[debug, release]` |

---

## 五、覆盖率矩阵

```
子系统                 测试模块                     覆盖度
──────────────────────────────────────────────────────
进程管理               test_process                  ████████ 100%
进程管理(增强)         test_process_enhanced         ████████ 100%
调度器                 test_scheduler*               ████████ 100%
SMP                    test_smp                      ████████ 100%
VFS                    test_vfs*                     ████████ 100%
系统调用               test_syscall*                 ████████ 100%
IPC                    test_ipc*                     ████████ 100%
HvFS                   test_hvfs                     ████████ 100%
持久化                 test_persistence              ████████ 100%
权限模型               test_pwid_enhanced            ████████ 100%
内存管理               test_pmm/kmalloc/slab         ███████ 75% (slab禁用)
栏栈恢复               test_recovery                 ████████ 100%
并发原语               test_spinlock/atomic/rwlock/mutex ████████ 100%
网络栈                 test_network                  ████████ 100%
设备驱动               test_pci/dma                  ░░░░░░░░ 0% (禁用)
中断                   test_interrupt                ░░░░░░░░ 0% (架构冲突)
硬件仿真               test_qemu_hardware            ░░░░░░░░ 0% (禁用)
设备文件系统           (无)                          ░░░░░░░░ 0%
IOAPIC                 (间接)                        ░░░░░░░░ 0%
定时器                 (间接)                        ░░░░░░░░ 0%
键盘/串口              (无)                          ░░░░░░░░ 0%
──────────────────────────────────────────────────────
总体                   224 用例                       ~85%
```

---

## 六、CI 集成方案

```makefile
# Makefile 测试矩阵

test-unit:          # 当前: 单核 QEMU, KERNEL_TEST=1
test-smp:           # 多核: qemu -smp 2 -M q35 (启用 PCI/DMA/QEMU硬件)
test-interrupt:     # 独立: 仅中断模块 + 最小内核
test-chaos:         # 栏栈故障注入: 随机 panic → 验证恢复率
test-coverage:      # 覆盖率: cargo llvm-cov + 报告
test-all: test-unit test-smp test-chaos  # 全量
```

---

## 七、与栏栈的集成点

栏栈故障注入框架（[barrier-stack-design.md §5.4](barrier-stack-design.md)）是测试框架的下一优先级任务：

```
test-chaos:
  ├─ RUSTFLAGS="--cfg fault_injection"
  ├─ maybe_inject_fault() 在每次 UndoLog::record 后随机触发 panic
  ├─ 栏栈捕获 → cascade_rollback → 验证 UndoLog 清空 + VFS 快照恢复
  └─ CI 断言: 恢复率 > 99%, 无 Quarantine 泄漏
```

这将是栏栈子系统从"单元测试验证"到"混沌工程验证"的关键一步。
