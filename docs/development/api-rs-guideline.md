# AntX 子系统 API 化规范 (`api.rs` 使用准则)

> **版本**: v1.0
> **适用范围**: AntX 内核 (`src/kernel/*`) 所有新增/重构子系统
> **核心立场**: **拒绝"全 api.rs 化"的过度工程** —— 严格按需引入 API 层

---

## 一、TL;DR (30 秒读懂)

- **6-8 个子系统**有 API 层(`api.rs` 或类似契约文件) → **对外契约**
- **10+ 个子系统**直接 `pub fn` 调用 → **基础设施 / 物理 / 工具**
- 判定标准只有一个: **"是否被 3 个以上、不同层级的模块调用,且需要解耦?"**
  - 是 → 提 API
  - 否 → 直接 `pub fn`

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

## 三、判定算法(决策树)

```
新增/重构一个子系统 X
│
├─ X 的接口是否被 ≥ 3 个不同层级的模块调用?
│   ├─ 否 ─→  X 不需要 api.rs,直接 pub fn
│   └─ 是 ─┐
│          │
│          ├─ X 是否有 ≥ 2 个独立实现?
│          │   (例如 fs 下的 ramfs/hvfs/devfs/procfs)
│          │   ├─ 否 ─→  不需要 trait,函数集合即可
│          │   └─ 是 ─┐
│          │          │
│          │          ├─ 调用方是否需要在运行时切换实现?
│          │          │   (注册表 / 工厂模式)
│          │          │   ├─ 否 ─→ 泛型静态分发即可,不一定 api.rs
│          │          │   └─ 是 ─→ ✅ 提 api.rs + trait
│          │
│          └─ X 是否会被未来新模块/驱动扩展?
│              (例如 driver 未来要加新设备类型)
│              ├─ 否 ─→ 不需要 api.rs
│              └─ 是 ─→ ✅ 提 api.rs
```

### 3.1 简化为"三个问题"

1. **多实现?** (≥ 2 个)
2. **运行时切换?** (注册表 / 工厂)
3. **可扩展?** (未来会加新实现)

**三问中两问为"是"** → 需要 API 层  
**三问中两问为"否"** → 直接 `pub fn`

---

## 四、AntX 子系统清单(权威分类)

### 4.1 ✅ 需要 API 层的子系统(6-8 个)

| 子系统 | API 层文件 | 主要消费者 | 现有 trait/契约 |
|---|---|---|---|
| **fs** | `fs/vfs/api.rs` | syscall, proc, 用户态 | `FsType` 枚举 + 挂载表 |
| **driver** | (由 chitin 统一) | chitin, syscall | `Driver` trait (`driver/framework.rs`) |
| **chitin** | `chitin/mod.rs` | driver, fs | `ChitinDevice`, `proto_*` 协议族 |
| **net** | `net/init.rs` | syscall, proc | socket API, `smoltcp` trait |
| **ipc** | `ipc/mod.rs` | proc, syscall | `pipe/shm/msgq` 模块级 API |
| **syscall** | `syscall/mod.rs` | 用户态, asm stub | `SyscallHandler` 函数指针表 |
| **barrier** | `barrier/api.rs` | 各可恢复子系统 | `RecoveryDomain` 注册表 |
| **credo** | `credo/api.rs` | syscall, proc | 能力 / DID 句柄 |
| **pci** | `pci/mod.rs` | driver | `PciDevice` 句柄 |
| **dma** | `dma/mod.rs` | driver | `DmaEngine` trait |

### 4.2 ❌ 不需要 API 层的子系统(10+ 个)

| 子系统 | 原因 | 调用方式 |
|---|---|---|
| **mm** | 太基础,被所有依赖;`Allocator` 在 `no_std` 已最优 | `pub fn kmalloc/page_alloc` |
| **sync** | 同步原语本身是底层基元 | `pub fn Mutex::new` |
| **arch** | 平台特定,无统一语义 | `pub fn` + `arch!` 宏 |
| **cpu** | CPUID/MSR/TSC 平台相关 | `pub fn` |
| **idt** | 中断表,物理事件 | `pub fn` + 函数指针注册 |
| **irq** | 软中断,单实现 | `pub fn` |
| **timer** | 时钟单例,基础 | `pub fn` |
| **lib** | 字符串/内存工具 | `pub fn` |
| **klog** | 简单 `log!` 宏 | macro |
| **config** | 静态配置常量 | `const fn` / `pub const` |
| **console** | 单实现,直接调用 | `pub fn` |
| **credo (内部)** | 子模块间 | `pub use` |

### 4.3 ⚪ 内部模块(父模块下子模块互调)

| 父模块 | 子模块之间 |
|---|---|
| **proc** | process/scheduler/thread/elf 之间 |
| **fs** | ramfs/hvfs/devfs/procfs 之间(VFS 层之下) |
| **driver** | 各具体驱动之间(经 chitin) |
| **net** | smoltcp_impl/socket 之间 |

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

### 7.1 已有合规的 `api.rs` / 契约层

| 文件 | 状态 | 评价 |
|---|---|---|
| `fs/vfs/api.rs` | ✅ 合规 | 暴露 vfs_* 给 syscall,合理 |
| `barrier/api.rs` | ✅ 合规 | 暴露 recovery_* 给可恢复域,合理 |
| `credo/api.rs` | ✅ 合规 | 暴露身份/能力给 syscall,合理 |
| `chitin/mod.rs` | ✅ 合规 | 完整设备协议族,合理 |
| `driver/framework.rs` | ✅ 合规 | `Driver` trait 合理 |
| `pci/mod.rs` | ⚠️ 需检查 | 暴露 `PciDevice` 句柄? |
| `net/init.rs` | ⚠️ 需检查 | 是否需独立 api.rs? |
| `ipc/mod.rs` | ⚠️ 需检查 | 是否需独立 api.rs? |

### 7.2 反模式警告

- ❌ **不要**新建 `mm/api.rs`(已是 `pub fn` 集合,无外部消费者)
- ❌ **不要**新建 `sync/api.rs`(基础原语,无人需要抽象)
- ❌ **不要**新建 `cpu/api.rs` / `arch/api.rs` / `idt/api.rs`(平台/物理)
- ❌ **不要**新建 `klog/api.rs` / `lib/api.rs` / `config/api.rs`(工具/配置)

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

- **当前版本**: v1.0 (2026-06-02)
- **维护责任**: 内核架构组
- **变更流程**:
  1. 提案: 在 GitHub issue 写"是否需要 API 层"决策
  2. 评审: 至少 1 位维护者同意
  3. 实施: PR + 完整 4 target `cargo check`
  4. 文档: 同步更新本文档 §4 清单

---

## 附录:AntX 子系统 API 速查表

| 子系统 | 路径 | API 类型 | 状态 |
|---|---|---|---|
| fs | `fs/vfs/api.rs` | 函数集合 + FsType | ✅ |
| chitin | `chitin/mod.rs` | trait + 协议族 | ✅ |
| driver | `driver/framework.rs` | `Driver` trait | ✅ |
| net | `net/init.rs` | 函数集合 | ⚠️ 评估 |
| ipc | `ipc/mod.rs` | 模块级 fn | ⚠️ 评估 |
| syscall | `syscall/mod.rs` | 函数指针表 | ✅ |
| barrier | `barrier/api.rs` | `RecoveryDomain` | ✅ |
| credo | `credo/api.rs` | 句柄 + 能力 | ✅ |
| pci | `pci/mod.rs` | 句柄 | ⚠️ 评估 |
| dma | `dma/mod.rs` | `DmaEngine` trait | ✅ |
| mm | `mm/*.rs` | `pub fn` | ✅ (无需 api.rs) |
| sync | `sync/*.rs` | `pub fn` | ✅ (无需 api.rs) |
| arch | `arch/*.rs` | `pub fn` + `arch!` | ✅ (无需 api.rs) |
| cpu | `cpu/*.rs` | `pub fn` | ✅ (无需 api.rs) |
| idt | `idt/*.rs` | 函数指针 | ✅ (无需 api.rs) |
| irq | `irq/mod.rs` | `pub fn` | ✅ (无需 api.rs) |
| timer | `timer/*.rs` | `pub fn` | ✅ (无需 api.rs) |
| lib | `lib/*.rs` | `pub fn` | ✅ (无需 api.rs) |
| klog | `klog/mod.rs` | macro | ✅ (无需 api.rs) |
| config | `config/*.rs` | `const` | ✅ (无需 api.rs) |
| console | `console/*.rs` | `pub fn` | ✅ (无需 api.rs) |
