# QX 内核演进蓝图

> 基于现有基础设施，规划创新功能与差异化能力。
> 最后更新: 2026-07-16

---

## 现状评估

QX 已完成 4 个 Phase 的架构演进，核心子系统已就绪：

| 子系统 | 成熟度 | 关键能力 |
|--------|--------|----------|
| 内存管理 | ⭐⭐⭐⭐ | 伙伴分配器 / Slab 缓存 / VMA / Demand Paging / COW / 内存压力感知调度 |
| 中断异常 | ⭐⭐⭐⭐ | IDT / Softirq / PageFault / DomainRecovery |
| 进程调度 | ⭐⭐⭐⭐ | per-CPU RunQueue / SMP IPI / COW fork |
| 同步原语 | ⭐⭐⭐⭐ | SpinLock / Mutex / RwLock / RCU |
| 文件系统 | ⭐⭐⭐⭐ | HvFS (SPA/DMU/ZAP/TXG) / ZIL / ARC / RAIDZ / Snapshot / CAS 内容寻址去重 |
| 能力系统 | ⭐⭐⭐⭐ | PWM 令牌委托 / 信任链 / 域隔离 |
| 设备模型 | ⭐⭐⭐ | Chitin 设备树 / DevTree 分层拓扑 / Compatible 匹配 |
| 网络 | ⭐⭐⭐ | lwIP 2.2.1 / DHCP/TCP/UDP/HTTP/DNS / e1000/virtio-net |
| 恢复机制 | ⭐⭐⭐⭐ | Barrier 栏栈 / UndoLog / DomainRecovery / 子系统级微重启 / 健康监控 / 持久化指纹 |

**跨架构**: x86_64 0 errors 0 warnings, AArch64 0 errors 0 warnings.
**测试**: 69 host-tests + ~125 kernel-tests, QEMU 链接 0 undefined references.

---

## 功能路线图

### Phase 5: ✅ 崩溃可恢复微重启 (Crash-Resilient Micro-Reboot) **[已完成 - 2026-07-16]**

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
2. **子系统注册** (各 init 函数调用)
   - HvFS: `recovery_register(FS_DOMAIN, &[SPA_DOMAIN])`
   - 网络: `recovery_register(NET_DOMAIN, &[])`
3. **DomainRecovery 改造** (`idt/idt.rs` / `handlers.rs`) — 按拓扑序级联恢复
4. **验证** — 循环注入 100 次 → 验证无内存泄漏

#### ✅ 完成总结 (2026-07-16)

- 新建 `kernel/barrier/recovery.rs`: `RecoveryDomain` trait + 拓扑级联恢复 + 子系统注册 API
- `CRASH_RIP` 机制: 崩溃地址 (`frame.rip`) → `locate_domain_by_addr()` 精确定位故障域
- HvFS + Net 恢复入口: `hvfs_restore()` / `net_restore()` 自动重新初始化子系统
- 调度器 `tick()` 集成心跳监控: 丢失心跳自动触发 BSR
- 启动时 `check_boot_fingerprints()` 检测连续崩溃 → Degraded 模式启动

---

### Phase 6: ✅ 内容寻址存储 (Content-Addressable Storage) **[已完成 - 2026-07-16]**

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

1. **CAS 索引** (`kernel/fs/hvfs/dedup.rs`) — `BTreeMap<[u8; 32], Vec<HvBlockPointer>>` + ref_counts
2. **写入路径改造** (`kernel/fs/hvfs/dmu.rs`) — 写入前计算 SHA256 → 查重 → ref or alloc
3. **ZIL 扩展** (`kernel/fs/hvfs/zil.rs`) — 新增 `DedupRef=12` / `DedupUnref=13`
4. **ARC 集成** — CAS 命中的块跳过 eviction

#### ✅ 完成总结 (2026-07-16)

- 新建 `kernel/fs/hvfs/dedup.rs`: `CasIndex` — BTreeMap 双重索引 `hash_to_dva` + `ref_counts`
- `sha256()` / `sha256_matches()` / `cas_aware_write()` — 写入时自动计算内容哈希并查重
- ZIL 扩展: `DedupRef=12` / `DedupUnref=13` 记录类型 + 反序列化支持
- API: `cas_lookup()`, `cas_insert()`, `cas_ref_inc()`, `cas_ref_dec()`, `cas_is_known()`, `cas_stats()`

---

### Phase 7: WASM 原生内核沙箱

**优先级**: 🥈 | **投入**: 大 | **创新度**: ⭐⭐⭐⭐⭐ | **实用价值**: 高

#### 背景

QX 有完整 VMA + Demand Paging + COW + PWM 能力系统。这恰好是 WASM 运行时的核心依赖——线性内存隔离 + 能力校验。

#### 目标

内核直接执行 WASM 字节码作为"内核小程序"——比 eBPF 更强表达力，比内核模块更安全。

#### 实现路径

1. **WASM 解释器** (`kernel/wasm/`) — 引入 `wasmi` (no_std 兼容)
2. **系统调用桥接** (`kernel/wasm/syscalls.rs`) — WASM import → kernel syscall dispatch
3. **资源配额** — 每实例 VMA 数、RSS、gas metering
4. **验证** — fibonacci.wasm + 恶意 WASM → SEGV → 内核继续运行

---

### Phase 8: ✅ 内存压力感知调度 (Memory-Pressure-Aware Scheduler) **[已完成 - 2026-07-16]**

**优先级**: 🥈 | **投入**: 小 | **创新度**: ⭐⭐⭐⭐ | **实用价值**: 极高

#### 背景

VMA 跟踪每进程 RSS，PMM 伙伴系统有 free_pages 统计。目前 OOM 是事后反应——进程死掉才知道内存不够。

#### 目标

在 OOM 发生**之前**，调度器根据内存压力动态调整进程优先级，实现预测式内存管理。

```
PMM.free_pages < THRESHOLD_WARNING (256 页 / ~10%)
  → scheduler.memory_pressure = WARNING → 通知进程释放 page cache

PMM.free_pages < THRESHOLD_CRITICAL (64 页 / ~3%)
  → scheduler.memory_pressure = CRITICAL → 降优先级 + 阻塞 mmap

PMM.free_pages < THRESHOLD_EMERGENCY (16 页 / <1%)
  → scheduler.memory_pressure = EMERGENCY → SIGTERM → 5s 未释放 → SIGKILL
```

#### 实现路径

1. **压力检测** (`kernel/mm/pressure.rs`) — 4 级压力枚举 + `update_pressure(free, total)`
2. **OOMD 内核线程** (`kernel/proc/oomd.rs`) — 每 100 tick 周期性检查
3. **调度器集成** (`kernel/proc/scheduler.rs`) — `tick()` 中调用 OOMD

#### ✅ 完成总结 (2026-07-16)

- 新建 `kernel/mm/pressure.rs`: 4 级压力检测 `Normal → Warning → Critical → Emergency`，`AtomicU64` 可配置阈值
- 新建 `kernel/proc/oomd.rs`: OOM 守护 — 每 100 tick 检查，Emergency 时 500 tick 超时后 kill
- 调度器 `tick()` 集成: 与 barrier 同循环，零额外开销
- 可配置阈值: `set_thresholds(warn, crit, emer)` + `disable()` 紧急关闭

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
```

#### 实现路径

1. **CompositeDriver trait** (`kernel/chitin/composite.rs`)
2. **RAID0/RAID1/Mirror** 作为内置组合驱动
3. **DeviceTree walk** 检测 `compatible` 并加载组合驱动

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

---

## 实施优先级矩阵

| 优先级 | Phase | 功能 | 状态 | 投入 | 创新度 |
|--------|-------|------|------|------|--------|
| **1** | 8 | 内存压力感知调度 | ✅ 已完成 | 小 (3天) | ⭐⭐⭐⭐ |
| **2** | 5 | 微重启崩溃恢复 | ✅ 已完成 | 中 (7天) | ⭐⭐⭐⭐⭐ |
| **3** | 6 | 内容寻址存储 | ✅ 已完成 | 中 (10天) | ⭐⭐⭐⭐⭐ |
| **4** | 9 | 可组合虚拟设备 | ⏳ 未开始 | 中 (7天) | ⭐⭐⭐⭐ |
| **5** | 10 | 用户态驱动 | ⏳ 未开始 | 大 (14天) | ⭐⭐⭐ |
| **6** | 7 | WASM 内核沙箱 | ⏳ 未开始 | 大 (21天) | ⭐⭐⭐⭐⭐ |

### 推荐执行顺序

```
Phase 8 (✅) → Phase 5 (✅) → Phase 6 (✅) → Phase 9 → Phase 10 → Phase 7
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
| 5 | 崩溃可恢复微重启 (Micro-Reboot) | ✅ |
| 6 | 内容寻址存储 (CAS Dedup) | ✅ |
| 8 | 内存压力感知调度 (Pressure-Aware) | ✅ |
| — | 稳健性修复 (P0/P1) + 栏栈深化 (4 防御 + 5 深化) | ✅ |
