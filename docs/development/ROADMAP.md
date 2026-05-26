# QX 内核演进蓝图

> 基于现有基础设施，规划创新功能与差异化能力。
> 最后更新: 2026-06-28

---

## 现状评估

QX 已完成 4 个 Phase 的架构演进，核心子系统已就绪：

| 子系统 | 成熟度 | 关键能力 |
|--------|--------|----------|
| 内存管理 | ⭐⭐⭐⭐ | 伙伴分配器 / Slab 缓存 / VMA / Demand Paging / COW |
| 中断异常 | ⭐⭐⭐⭐ | IDT / Softirq / PageFault / DomainRecovery |
| 进程调度 | ⭐⭐⭐⭐ | per-CPU RunQueue / SMP IPI / COW fork |
| 同步原语 | ⭐⭐⭐⭐ | SpinLock / Mutex / RwLock / RCU |
| 文件系统 | ⭐⭐⭐⭐ | HvFS (SPA/DMU/ZAP/TXG) / ZIL / ARC / RAIDZ / Snapshot |
| 能力系统 | ⭐⭐⭐⭐ | PWM 令牌委托 / 信任链 / 域隔离 |
| 设备模型 | ⭐⭐⭐ | Chitin 设备树 / DevTree 分层拓扑 / Compatible 匹配 |
| 网络 | ⭐⭐⭐ | lwIP 2.2.1 / DHCP/TCP/UDP/HTTP/DNS / e1000/virtio-net |
| 恢复机制 | ⭐⭐⭐ | Barrier 栏栈 / UndoLog / DomainRecovery |

**跨架构**: x86_64 0 errors 0 warnings, AArch64 0 errors 0 warnings.
**测试**: 69 host-tests + ~125 kernel-tests, QEMU 链接 0 undefined references.

---

## 功能路线图

### Phase 5: 崩溃可恢复微重启 (Crash-Resilient Micro-Reboot)

**优先级**: 🥇 | **投入**: 中 | **创新度**: ⭐⭐⭐⭐⭐ | **实用价值**: 极高

#### 背景

QX 已有 `Barrier` 栏栈 + `DomainRecovery` 机制。异常处理器可执行域级恢复而非全内核 panic。但目前 `DomainRecovery` 是单层粗粒度恢复，未将恢复域推广到子系统级。

#### 目标

将"域"的概念推广到子系统级——文件系统、网络栈、驱动各为一个 recovery domain。当一个域崩溃时，隔离并重启该域而不影响其他服务。

```
#PF in HvFS    → DomainRecovery → 重新初始化 HvFS 状态机 → 恢复运行
GPF in e1000   → DomainRecovery → 重置网卡 + 重建 lwIP netif → 网络继续
DoubleFault     → 级联恢复 → HvFS → Net → 全部重置
TripleFault     → 内核 panic (不可恢复)
```

#### 实现路径

1. **定义 `RecoveryDomain` trait** (`kernel/barrier/recovery.rs`)
   ```rust
   pub trait RecoveryDomain: Send + Sync {
       fn name(&self) -> &'static str;
       fn save_checkpoint(&self);       // 崩溃前快照
       fn restore_checkpoint(&self);    // 恢复至最后一个安全点
       fn reset(&self);                 // 硬复位
       fn dependencies(&self) -> &[DomainId];  // 拓扑依赖
   }
   ```

2. **子系统注册** (各 init 函数调用)
   - HvFS: `recovery_register(FS_DOMAIN, &[SPA_DOMAIN])`
   - 网络: `recovery_register(NET_DOMAIN, &[])`
   - 驱动: 每个驱动独立注册

3. **DomainRecovery 改造** (`idt/idt.rs` / `handlers.rs`)
   - 异常发生时确定故障域
   - 按拓扑序执行级联恢复: 子域先恢复，父域后恢复
   - 记录恢复日志到 `KLOG_RECOVERY` 缓冲区

4. **验证**
   - 注入 #PF 到 HvFS 代码路径 → 验证自动恢复
   - 循环注入 100 次 → 验证无内存泄漏

#### 涉及文件

- `kernel/barrier/` — 现有 Barrier 基础设施
- `kernel/idt/handlers.rs` — DomainRecovery 分发
- `kernel/idt/idt.rs` — handle_page_fault / handle_gpf
- `kernel/fs/hvfs/hvfs.rs` — HvFS 恢复入口
- `kernel/net/init.rs` — 网络栈恢复入口

---

### Phase 6: 内容寻址存储 (Content-Addressable Storage)

**优先级**: 🥇 | **投入**: 中 | **创新度**: ⭐⭐⭐⭐⭐ | **实用价值**: 高

#### 背景

HvFS 已有 fletcher2/4 + SHA256 checksum、ZIL intent log、ARC 自适应缓存。文件系统块索引目前按 DVA (Data Virtual Address) 寻址。

#### 目标

块索引不按 LBA 而按内容哈希。相同内容的块自动去重（dedup），ZIL replay 可验证完整性。

```
写 /data/config.json (4KB)
  → SHA256(content) = 0xDEAD...BEEF
  → 查询 CAS 索引: 已存在 (refcount=3)
  → refcount++, 不写新块
  → ZIL 记录: {obj_id=5, offset=0, hash=0xDEAD...BEEF, op=RefInc}
```

#### 实现路径

1. **CAS 索引** (`kernel/fs/hvfs/dedup.rs`)
   ```rust
   pub struct CasIndex {
       hash_to_dva: BTreeMap<[u8; 32], Vec<HvBlockPointer>>,
       ref_counts: BTreeMap<[u8; 32], u64>,
   }
   ```

2. **写入路径改造** (`kernel/fs/hvfs/dmu.rs`)
   - `dmu_write()` 前计算 SHA256
   - 查 CAS 索引 → 命中则 ref++，未命中则正常分配
   - 空闲块回收时 ref-- → refcount=0 时真正释放

3. **ZIL 扩展** (`kernel/fs/hvfs/zil.rs`)
   - 新增 `HvZilRecordType::DedupRef` / `DedupUnref`
   - replay 时验证内容哈希与记录一致

4. **ARC 集成** — CAS 命中的块跳过 eviction

5. **验证**
   - 写入 100 个 4KB 随机块中 50 个重复 → 物理占用 = 50 块
   - ZIL replay 后 dedup 引用计数一致

#### 涉及文件

- `kernel/fs/hvfs/dmu.rs` — 写路径
- `kernel/fs/hvfs/dedup.rs` — 新建，CAS 索引
- `kernel/fs/hvfs/zil.rs` — 扩展 record type
- `kernel/fs/hvfs/bp.rs` — BlockPointer 增加 hash 字段
- `kernel/fs/hvfs/arc.rs` — CAS 感知缓存

---

### Phase 7: WASM 原生内核沙箱

**优先级**: 🥈 | **投入**: 大 | **创新度**: ⭐⭐⭐⭐⭐ | **实用价值**: 高

#### 背景

QX 有完整 VMA + Demand Paging + COW + PWM 能力系统。这恰好是 WASM 运行时的核心依赖——线性内存隔离 + 能力校验。

#### 目标

内核直接执行 WASM 字节码作为"内核小程序"——比 eBPF 更强表达力，比内核模块更安全。

```
用户程序 → .wasm binary → 内核 WASM 解释器 → 安全执行
                              │
                              ├── VMA: 每实例独立地址空间
                              ├── PWM: cap_net_raw, cap_fs_write...
                              ├── PageFault → SignalSegv (不 panic)
                              └── COW: fork wasm实例 = 零拷贝
```

#### 实现路径

1. **WASM 解释器** (`kernel/wasm/`)
   - 引入 `wasmi` (no_std 兼容的 WASM 解释器)
   - 限制指令集: 无浮点（可选）、无 bulk-memory、无 SIMD
   - 线性内存通过 `VMA::Anonymous` 映射

2. **系统调用桥接** (`kernel/wasm/syscalls.rs`)
   - WASM import → 内核 syscall dispatch
   - 每个 syscall 前校验 PWM capability
   - 指针通过 `copy_from_user` 安全传递

3. **资源配额**
   - 每实例限制: 最大 VMA 数、最大 RSS、最大执行指令数（gas metering）
   - 超限 → `PfResult::SignalSegv` → 终止 WASM 实例

4. **验证**
   - 加载 fibonacci.wasm → 计算结果
   - 恶意 WASM (无限循环 / 越界写) → gas 超限 / SEGV → 内核继续运行

#### 涉及文件

- `kernel/wasm/` — 新建模块
- `kernel/mm/vma.rs` — WASM 线性内存 VMA
- `kernel/mm/page_fault.rs` — WASM #PF 处理
- `kernel/syscall/mod.rs` — dispatch 桥接

---

### Phase 8: 内存压力感知调度 (Memory-Pressure-Aware Scheduler)

**优先级**: 🥈 | **投入**: 小 | **创新度**: ⭐⭐⭐⭐ | **实用价值**: 极高

#### 背景

VMA 跟踪每进程 RSS，PMM 伙伴系统有 free_pages 统计。目前 OOM 是事后反应——进程死掉才知道内存不够。

#### 目标

在 OOM 发生**之前**，调度器根据内存压力动态调整进程优先级，实现预测式内存管理。

```
PMM.free_pages < THRESHOLD_WARNING (25%)
  → scheduler.memory_pressure = WARNING
  → 通知进程释放 page cache

PMM.free_pages < THRESHOLD_CRITICAL (10%)
  → scheduler.memory_pressure = CRITICAL
  → Top-3 RSS 进程降优先级
  → 阻塞新 mmap() 调用

PMM.free_pages < THRESHOLD_EMERGENCY (3%)
  → scheduler.memory_pressure = EMERGENCY
  → 发送 SIGTERM 给最大 RSS 进程
  → 如果 5s 内未释放 → SIGKILL
```

#### 实现路径

1. **压力检测** (`kernel/mm/pressure.rs`)
   ```rust
   pub enum MemoryPressure { Normal, Warning, Critical, Emergency }
   pub fn current_pressure() -> MemoryPressure;
   ```

2. **调度器集成** (`kernel/proc/scheduler_ex.rs`)
   - 新增 `on_memory_pressure()` 回调
   - Emergency 时暂停非关键进程的调度

3. **OOMD 内核线程** (`kernel/proc/oomd.rs`)
   - 周期性检查压力级别
   - 渐进式回收: page cache → 降优先级 → SIGTERM → SIGKILL

4. **验证**
   - 分配内存直至耗尽 → 验证 Emergency 触发而非直接 panic

#### 涉及文件

- `kernel/mm/pmm.rs` — free_pages 统计
- `kernel/mm/vma.rs` — RSS 统计
- `kernel/proc/scheduler_ex.rs` — 压力回调
- `kernel/proc/oomd.rs` — 新建

---

### Phase 9: Chitin 可组合虚拟设备

**优先级**: 🥉 | **投入**: 中 | **创新度**: ⭐⭐⭐⭐ | **实用价值**: 中

#### 背景

Chitin 设备树已有分层拓扑 + compatible 驱动匹配。`ChitinNode` 有 `children: Vec<NodeId>`。

#### 目标

设备树节点可以是"虚拟设备"——由多个物理设备组合而成。

```
ChitinNode("virt-raid0")
  ├── children: [nvme0, nvme1]
  ├── compatible: "qx,raid0"
  └── props: { stripe_size: 64KB }

virt-raid0 的 Driver::read() → stripe across nvme0 + nvme1
virt-mirror 的 Driver::write() → mirror to nvme0 + nvme1
```

#### 实现路径

1. **CompositeDriver trait** (`kernel/chitin/composite.rs`)
2. **RAID0/RAID1/Mirror** 作为内置组合驱动
3. **DeviceTree walk** 检测 `compatible` 并加载组合驱动

#### 涉及文件

- `kernel/chitin/devtree.rs` — 扩展 walk 逻辑
- `kernel/chitin/composite.rs` — 新建
- `kernel/driver/block/` — 组合块设备

---

### Phase 10: 用户态驱动框架 (User-Space Driver)

**优先级**: 🥉 | **投入**: 大 | **创新度**: ⭐⭐⭐ | **实用价值**: 高

#### 背景

VMA 用户态映射 / COW / Demand Paging / PWM 能力 / Chitin 设备树 — 安全用户态驱动的全部基础设施已就绪。

#### 目标

Chitin 设备树节点可直接映射到用户态进程，实现安全用户态驱动。

#### 实现路径

1. ChitinNode 增加 `user_mapped: Option<Pid>` 字段
2. `devtree_bind_device` 支持用户态绑定
3. 中断转发: IDT → `chitin_forward_irq(node_id)` → 信号给进程
4. PWM 新增 `CAP_DRIVER_*` 能力系列

#### 涉及文件

- `kernel/chitin/` — 用户绑定
- `kernel/idt/` — 中断转发
- `kernel/mm/vma.rs` — IOMMU 映射

---

## 实施优先级矩阵

| 优先级 | Phase | 功能 | 投入 | 创新度 | 依赖 | 预计提交 |
|--------|-------|------|------|--------|------|----------|
| **1** | 8 | 内存压力感知调度 | 小 (3天) | ⭐⭐⭐⭐ | 无 | 1-2 commits |
| **2** | 5 | 微重启崩溃恢复 | 中 (7天) | ⭐⭐⭐⭐⭐ | Barrier 现有 | 5-10 commits |
| **3** | 6 | 内容寻址存储 | 中 (10天) | ⭐⭐⭐⭐⭐ | SHA256/checksum 现有 | 8-15 commits |
| **4** | 9 | 可组合虚拟设备 | 中 (7天) | ⭐⭐⭐⭐ | DevTree 现有 | 5-10 commits |
| **5** | 10 | 用户态驱动 | 大 (14天) | ⭐⭐⭐ | Phase 5+9 | 10-20 commits |
| **6** | 7 | WASM 内核沙箱 | 大 (21天) | ⭐⭐⭐⭐⭐ | Phase 8 | 20-40 commits |

### 推荐执行顺序

```
Phase 8 (小投入快速收益) → Phase 5 (高创新) → Phase 6 (高差异化)
    → Phase 9 (中创新) → Phase 10 (大盘) → Phase 7 (终极形态)
```

---

## 附录: 已完成 Phase

| Phase | 内容 | 状态 |
|-------|------|------|
| 1 | 伙伴系统 + Softirq + SMP RunQueue | ✅ |
| 2 | VMA + Chitin 设备树 + RCU | ✅ |
| 3 | IPC 动态扩容 + Slab kmalloc + ZIL WAL | ✅ |
| 4a | Demand Paging + #PF Handler 集成 | ✅ |
| 4c | mmap/munmap/mprotect + VMA 集成 | ✅ |
| 4d | COW fork() + ELF64 加载器 | ✅ |
| — | 稳健性修复 (P0/P1) + 测试框架更新 | ✅ |
