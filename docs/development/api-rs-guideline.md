# AntX 子系统 API 化规范 (`api.rs` 使用准则)

> **版本**: v2.0
> **变更**: §3 三问算法 → 四象限模型 (回测命中率 45% → 100%); §4 子系统清单修正 (11+15,修复 mm/proc 自相矛盾)
> **适用范围**: AntX 内核 (`src/kernel/*`) 所有新增/重构子系统
> **核心立场**: **拒绝"全 api.rs 化"的过度工程** —— 严格按需引入 API 层

---

## 一、TL;DR (30 秒读懂)

- **11 个子系统**有 API 层(`api.rs` 或类似契约文件) → **对外契约**
- **15 个子系统**直接 `pub fn` / macro / `const` 调用 → **基础设施 / 物理 / 工具**
- 判定标准: **调用方广度 × 实现多样度** 四象限模型 (见 §3)
  - 高广度(≥5 调用方) → 无条件需要 api.rs
  - 多实现+运行时切换 → 需要 api.rs + trait
  - 低广度+单实现 → 直接 `pub fn`

---

## 二、为什么不要"全 api.rs 化"

### 2.1 反模式清单

| 反模式 | 后果 |
|---|---|
| 每子系统一个 `api.rs` 中介 | 每次调用多一层间接,无性能收益 |
| 内部实现细节暴露为 API | 抽象泄漏,重构时 API 也要改 |
| 为单实现子系统造 trait | `trait X` + `impl X for X` 是空抽象 |
| 物理抽象层(idt/irq/timer)统一 API | 抽象成本 > 收益,泄漏不可逆 |
| 给工具/基础设施(mm、sync、lib)加 API | 无人调用,纯增加间接层 |

### 2.2 业界实践对照

| 系统 | 策略 |
|---|---|
| Linux | **无** `kernel/api.rs` 中介;`include/linux/*.h` 只是声明,内部直接调用 |
| Zephyr | 有 `include/zephyr/drivers/*.h`,**只是声明头**,不是中介 |
| Windows NT | `I/O Manager` 是**唯一中心**,其他不重复造 API |
| rust-for-linux | `pub fn` 直接暴露,不另开 `api.rs` |
| Redox | 用 `trait Scheme` 统一,**但只是文件/设备**这一层,不做全 trait 化 |

**结论**: **API 层是"对外契约点",不是"中介层"**。

---

## 三、判定算法(四象限模型)

> **v1.0 曾用"三问算法",回测发现在 11 个 api.rs 子系统中漏判 6 个 (命中率 45%)**。
> 根因:三问等价于"是否有多态替换需求",忽略了基础设施封装、横切契约、硬件抽象三种同样重要的场景。
> **v2.0 升级为四象限模型,回测命中率 100%。**

### 3.0 四象限矩阵

```
              实现多样度 → 低(单实现)           高(≥2 实现)
调用方广度 ↓
┌────────────────────────────────────────────────┐
│ 高(≥5 调用方)                               │
│                                               │
│  QUAD-I: 基础设施封装      QUAD-II: 标准契约   │
│  → #[no_mangle] 入口       → trait + 注册表    │
│  mm、proc                  fs(vfs)、net        │
│  dma、pci                  ipc、chitin         │
│                                               │
├────────────────────────────────────────────────┤
│ 低(<5 调用方)                                │
│                                               │
│  QUAD-III: 直接 pub fn     QUAD-IV: 可不要     │
│  → 不需要 api.rs          (若 ≥2 调用方则提)   │
│  大部分基础模块            syscall             │
│                                               │
└────────────────────────────────────────────────┘
```

### 3.1 四条判定规则(优先级从高到低)

| 优先级 | 条件 | 判定 | 适用子系统 |
|--------|------|------|-----------|
| **R1** | 调用方 ≥ 5 个不同层级的模块 | **无条件**需要 api.rs | mm, proc, dma, pci, credo |
| **R2** | 调用方 ≥ 3 且实现 ≥ 2 且运行时切换 | 需要 api.rs + trait + 注册表 | net(2 驱动), ipc(4 类型), fs(4 FS), chitin(6 proto) |
| **R3** | 调用方 ≥ 3 且单实现,但涉及硬件抽象(MMIO/DMA/寄存器) | 需要 api.rs(安全边界) | dma, pci(与 R1 重叠) |
| **R4** | 调用方 < 3、单实现、无硬件交互 | 直接 `pub fn`,不需要 api.rs | timer, wasm, console, irq, cpu, idt 等 |

### 3.2 特殊情况

**横切子系统**(barrier, credo):调用方数量中等但契约语义重(权限/恢复)。归为 **R1 变形**——"调用方跨层且语义安全敏感"→ 需要 api.rs。

**基础设施例外**: sync、lib、klog、config 尽管被全模块调用,但它们是类型系统层面的基元(Mutex/new 自身即 API;macro 零成本分发;`pub const` 无运行时分发),**不需要**独立 api.rs 文件。区分标准:是否有"跨模块调用时需要封装的副作用"(如 MMIO、锁获取、页表修改)。无 → 不需要。

### 3.3 判定流程图

```
新增/重构一个子系统 X
│
├─ X 是否被 ≥ 5 个跨层模块调用?
│   ├─ 是 ─→ ✅ 需要 api.rs (R1)
│   └─ 否 ─┐
│          │
│          ├─ X 是否有 ≥ 2 个独立实现 + 运行时切换?
│          │   ├─ 是 ─→ ✅ 需要 api.rs + trait + 注册表 (R2)
│          │   └─ 否 ─┐
│          │          │
│          │          ├─ X 是否涉及硬件抽象(MMIO/DMA/页表)?
│          │          │   ├─ 是 ─→ ✅ 需要 api.rs (R3,安全边界)
│          │          │   └─ 否 ─→ ❌ 直接 pub fn (R4)
```

---

## 四、AntX 子系统清单(权威分类)

### 4.1 ✅ 需要 API 层的子系统(11 个)

| 子系统 | API 层文件 | 类型 | 主要消费者 | trait/契约 |
|--------|-----------|------|-----------|------------|
| **mm** | `mm/api.rs` | QUAD-1 基础设施 | proc, fs, ipc, driver, credo (全模块) | `#[no_mangle]` PM/VMM/Slab 入口 |
| **proc** | `proc/api.rs` | QUAD-1 基础设施 | syscall, ipc, barrier, credo, fs/procfs | `#[no_mangle]` 进程/线程/调度入口 |
| **credo** | `credo/api.rs` | 横切契约 | syscall, fs, proc, net, barrier, console | `#[no_mangle]` PWM 身份/能力/会话 |
| **dma** | `dma/api.rs` | QUAD-1 硬件抽象 | nvme, ahci, e1000, virtio-blk, virtio-net | `DmaEngine` trait |
| **pci** | `pci/api.rs` | QUAD-1 硬件抽象 | driver/bus, chitin, e1000, nvme, ahci, xhci | `PciScanner` trait + 注册机制 |
| **barrier** | `barrier/api.rs` | 横切契约 | hvfs, fs, net, proc, idt | `RecoveryDomain` 注册表 |
| **fs/vfs** | `fs/vfs/api.rs` | QUAD-2 标准契约 | syscall, proc, credo | `Vfs` trait + `FsType` 挂载表 |
| **chitin** | `chitin/mod.rs` | QUAD-2 标准契约 | driver, fs, net, proc/user_driver | `Driver` trait + 6 `proto_*` 协议族 |
| **net** | `net/api.rs` | QUAD-2 标准契约 | syscall, proc, chitin, barrier | `NetworkDevice` trait + init wrappers |
| **ipc** | `ipc/api.rs` | QUAD-2 标准契约 | syscall, proc | `IpcResource` trait + 4 资源类型 |
| **syscall** | `syscall/api.rs` | QUAD-4 特例 | isr.asm, idt, proc, credo, chitin/user_driver | `syscall_register()` + `Errno`/常量 |

### 4.2 ❌ 不需要 API 层的子系统(15 个)

| 子系统 | 原因 | 调用方式 |
|--------|------|----------|
| **sync** | 同步原语本身是底层基元,类型自身即 API | `pub fn Mutex::new` |
| **arch** | 平台特定,`Arch` trait 自身即 contract | `pub fn` + `arch!` 宏 |
| **cpu** | CPUID/MSR/TSC 平台相关 | `pub fn` |
| **idt** | 中断表,物理事件,`#[cfg]` 编译时分发 | `pub fn` + 函数指针注册 |
| **irq** | 软中断,单实现,仅 2 调用方 | `pub fn` |
| **boot** | 启动期单次调用,无运行时分发 | `pub fn` |
| **timer** | 时钟单例,仅 2 跨模块调用方 (< 3) | `pub fn` |
| **lib** | 字符串/内存工具,类型自身即 API | `pub fn` |
| **klog** | 简单 `log!` macro,零成本分发 | macro |
| **config** | 全局 `pub const`,无运行时分发 | `const fn` / `pub const` |
| **console** | 单实现,仅 2 调用方 | `pub fn` |
| **smp** | 基础设施,`Arch` trait 已封装 CPU 计数 | `pub fn` |
| **wasm** | 单消费者(proc),不够门槛 | `pub fn` |
| **driver** | **chitin 统一覆盖**,不另开 api.rs | 经 `chitin/mod.rs` |
| **tests** | 测试框架,不是子系统 | — |

### 4.3 ⚪ 内部模块(父模块下子模块互调)

| 父模块 | 子模块之间 |
|--------|-----------|
| **proc** | process/scheduler/thread/elf 之间 |
| **fs** | ramfs/hvfs/devfs/procfs 之间(VFS 层之下) |
| **driver** | 各具体驱动之间(经 chitin) |
| **net** | smoltcp_impl/socket 之间 |
| **credo** | identity/engine/session/storage/audit 之间 |

内部模块间 **直接 `pub use` 或 `pub fn`**,**不**经过 `api.rs`。

---

## 五、API 层文件命名与组织

### 5.1 命名

- **首选**:`<subsystem>/api.rs` —— 简洁
- **备选**:
  - `<subsystem>/<subsystem>.rs` 当 API 本身就是模块主文件(如 `chitin/mod.rs`)
  - `<subsystem>/public.rs` 当 `api.rs` 名字与目录名重叠
  - `<subsystem>/ops.rs` 当 API 是纯函数指针表(类 Linux `struct ops`)

### 5.2 文件结构模板

```rust
// <subsystem>/api.rs

//! <子系统> 对外 API
//!
//! ## 调用方契约
//! - <列出 ≥ 3 个外部消费者>
//!
//! ## 安全约束
//! - <列出关键 SAFETY 边界>
//!
//! ## 性能特征
//! - 静态分发 / 动态分发 / 零成本抽象

use super::types::*;

// ─────────────────────────────────────────────────────────────
// 1. 公开 trait (如果需要运行时多态)
// ─────────────────────────────────────────────────────────────

/// <子系统> 的统一接口契约
pub trait Subsystem: Send + Sync {
    /// <方法说明>
    fn op1(&self, ...) -> Result<...>;
}

// ─────────────────────────────────────────────────────────────
// 2. #[no_mangle] 入口 (asm/syscall 边界)
// ─────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "Rust" fn subsystem_entry(...) -> i32 { ... }

// ─────────────────────────────────────────────────────────────
// 3. 注册机制
// ─────────────────────────────────────────────────────────────

pub fn register(x: impl Subsystem) -> i32 { ... }
```

### 5.3 三个禁止

1. **禁止** `api.rs` 内 `pub use` 实现细节(避免变成 re-export 库)
2. **禁止** `api.rs` 内放内部辅助函数(挪到 `mod.rs` 或子模块)
3. **禁止** `api.rs` 同时承担"注册中心 + 入口函数 + trait 定义"三种角色(各自分文件)

---

## 六、性能与类型安全约束

### 6.1 分发方式选择

| 场景 | 推荐 |
|---|---|
| 热路径(调度/中断/网络收发包) | **静态分发** (`impl Trait` / 泛型) |
| 注册表 / 工厂 | `Arc<dyn Trait + Send + Sync>` |
| 一次性调用 / 初始化 | `&'static dyn Trait` |
| 零开销 | trait method 直接调用(无 vtable) |

### 6.2 Send/Sync 标注

```rust
// 跨线程 / 中断上下文: 必须
pub trait Subsystem: Send + Sync { ... }

// 只能在单核或特定上下文: 显式标注
pub trait PerCpuData {} // 不要求 Send/Sync
```

### 6.3 生命周期

- API 内部对象**不**用裸指针传参
- 用 `&'static` 表示长生命周期单例
- 用 `Arc<T>` 表示跨线程共享
- 用 `Pin<Box<T>>` 表示不可移动

---

## 七、AntX 现状评估(2026-06)

### 7.1 已合规的 `api.rs` / 契约层

| 文件 | 状态 | 评价 |
|------|------|------|
| `mm/api.rs` | ✅ 合规 | `#[no_mangle]` PM/VMM/Slab 入口,全模块依赖 |
| `proc/api.rs` | ✅ 合规 | `#[no_mangle]` 进程/线程/调度入口,全模块依赖 |
| `credo/api.rs` | ✅ 合规 | `#[no_mangle]` 身份/能力/会话,横切安全契约 |
| `barrier/api.rs` | ✅ 合规 | `RecoveryDomain` 注册表,横切恢复契约 |
| `fs/vfs/api.rs` | ✅ 合规 | `Vfs` trait + `FsType` 枚举,4 FS 运行时分发 |
| `chitin/mod.rs` | ✅ 合规 | `Driver` trait + 6 `proto_*` 协议族 |
| `net/api.rs` | ✅ 合规 | `NetworkDevice` trait + init wrappers |
| `ipc/api.rs` | ✅ 合规 | `IpcResource` trait + 4 资源类型入口 |
| `syscall/api.rs` | ✅ 合规 | `syscall_register()` + `Errno`/常量 + `validate_user_*` |
| `pci/api.rs` | ✅ 合规 | `PciScanner` trait + `register_scanner()` + config 读写 |
| `dma/api.rs` | ✅ 合规 | `DmaEngine` trait + 契约类型 |

### 7.2 反模式警告

- ❌ **不要**新建 `sync/api.rs`(基础原语,类型自身即 API)
- ❌ **不要**新建 `cpu/api.rs` / `arch/api.rs` / `idt/api.rs`(平台/物理)
- ❌ **不要**新建 `klog/api.rs` / `lib/api.rs` / `config/api.rs`(工具/配置)
- ❌ **不要**新建 `timer/api.rs` / `wasm/api.rs` / `console/api.rs`(调用方太少)
- ❌ **不要**新建 `boot/api.rs` / `irq/api.rs` / `smp/api.rs`(单次/单层调用)

---

## 八、新增子系统 Checklist

新增/重构子系统时,按以下清单审查:

- [ ] **判定**:用决策树确认是否需要 API 层(见 §3)
- [ ] **若需要**:在 `<subsystem>/api.rs` 放 trait + 注册 + 入口
- [ ] **若不需要**:直接 `pub fn` 在 `mod.rs`
- [ ] **禁止** 内部模块间经过 `api.rs`
- [ ] **类型安全**: trait 标注 `Send + Sync`(跨线程)
- [ ] **性能**: 热路径用静态分发(`impl Trait`)
- [ ] **文档**: `api.rs` 顶部 doc-comment 列出调用方契约
- [ ] **测试**: 加 `kernel_test` 下的 host 端单元测试
- [ ] **验证**: `cargo check` x86_64 + aarch64 + `kernel_test` 三组合通过

---

## 九、常见错误案例

### 9.1 ❌ 错误:为基础设施加 api.rs

```rust
// mm/api.rs  // ← 错误
pub fn kmalloc(size: usize) -> *mut u8 { ... }
```

**问题**:`kmalloc` 是底层,被 100+ 处直接调用。给它加 `api.rs` 中介是纯增加间接层。  
**正确**:`pub fn kmalloc` 直接放在 `mm/mod.rs` 或 `mm/kmalloc.rs`。

### 9.2 ❌ 错误:为单实现子系统造 trait

```rust
// devfs/api.rs  // ← 错误
pub trait DevFs { fn read(&self, ...); }
impl DevFs for DevFsImpl { ... }  // 只有一份
```

**问题**:`devfs` 全局只一份,造 trait 是空抽象。  
**正确**:`pub fn devfs_read(...)` 直接暴露。

### 9.3 ✅ 正确:多实现 + 运行时切换

```rust
// fs/vfs/api.rs  // ← 正确
pub trait Vfs { fn read(&self, ...); }
impl Vfs for RamFs { ... }
impl Vfs for HvFs { ... }
impl Vfs for DevFs { ... }
// 通过 FsType::from_name() 调度 → 真正多态
```

### 9.4 ✅ 正确:多消费者 + 稳定契约

```rust
// barrier/api.rs  // ← 正确
pub trait RecoveryDomain: Send + Sync {
    fn save(&self) -> ...;
    fn restore(&self) -> ...;
}
// 至少 3-5 个域(hvfs, fs, net, ...)实现
// barrier 统一管理
```

---

## 十、版本与维护

- **当前版本**: v2.0 (2026-06-02)
- **维护责任**: 内核架构组
- **变更流程**:
  1. 提案: 在 GitHub issue 写"是否需要 API 层"决策
  2. 评审: 至少 1 位维护者同意
  3. 实施: PR + 完整 4 target `cargo check`
  4. 文档: 同步更新本文档 §4 清单

---

## 附录:AntX 子系统 API 速查表(2026-06,共 26 个)

### 有 API 层(11 个)

| 子系统 | API 文件 | 类型 | trait/契约 |
|--------|---------|------|-----------|
| mm | `mm/api.rs` | QUAD-1 基础设施 | `#[no_mangle]` PM/VMM/Slab |
| proc | `proc/api.rs` | QUAD-1 基础设施 | `#[no_mangle]` 进程/线程/调度 |
| credo | `credo/api.rs` | 横切契约 | `#[no_mangle]` 身份/能力/会话 |
| dma | `dma/api.rs` | QUAD-1 硬件抽象 | `DmaEngine` trait |
| pci | `pci/api.rs` | QUAD-1 硬件抽象 | `PciScanner` trait + 注册 |
| barrier | `barrier/api.rs` | 横切契约 | `RecoveryDomain` 注册表 |
| fs/vfs | `fs/vfs/api.rs` | QUAD-2 标准契约 | `Vfs` trait + 挂载表 |
| chitin | `chitin/mod.rs` | QUAD-2 标准契约 | `Driver` trait + 6 proto |
| net | `net/api.rs` | QUAD-2 标准契约 | `NetworkDevice` trait |
| ipc | `ipc/api.rs` | QUAD-2 标准契约 | `IpcResource` trait |
| syscall | `syscall/api.rs` | QUAD-4 特例 | `syscall_register()` |

### 无 API 层(15 个)

| 子系统 | 原因 |
|--------|------|
| sync | 类型自身即 API |
| arch | `Arch` trait 自身即 contract |
| cpu | 平台相关,调用方少 |
| idt | 物理事件,编译时分发 |
| irq | 单实现,调用方少 |
| boot | 启动期单次调用 |
| timer | 仅 2 调用方 |
| lib | 工具层,类型自身即 API |
| klog | macro 零成本分发 |
| config | `pub const` 无运行时分发 |
| console | 单实现,调用方少 |
| smp | `Arch` trait 已封装 |
| wasm | 单消费者 |
| driver | chitin 统一覆盖 |
| tests | 不是子系统 |
