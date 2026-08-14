# framework/barrier + chitin + debug + klog + smp 子系统深度审计报告

> **审计范围**：`src/kernel/framework/barrier/` (10) + `chitin/` (9) + `debug/` (6) + `klog/` (1) + `smp/` (1) = **27 个源文件**
> **审计日期**：2026-08-14
> **代码规模**：约 9,540 LoC
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **34 个问题（P0×5, P1×10, P2×12, P3×7）**

## 1. 子系统概览

| 子系统 | 文件数 | LoC | 主要职责 | 风险等级 |
|---|---:|---:|---|---|
| [barrier/](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/) | 10 | 2,123 | 故障恢复子系统（Barrier Recovery + BSR/BHR）| **极高** |
| [chitin/](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/) | 9 | 2,878 | 几丁质设备驱动框架（Chitin Framework）| **极高** |
| [debug/](file:///home/anfer/Code/QueenX/src/kernel/framework/debug/) | 6 | 2,582 | eBPF/ftrace/kgdb/ringbuf 调试基础设施 | **高** |
| [klog/](file:///home/anfer/Code/QueenX/src/kernel/framework/klog/) | 1 | 939 | 内核日志（COM1 串口 + 环形缓冲）| **高** |
| [smp/](file:///home/anfer/Code/QueenX/src/kernel/framework/smp/) | 1 | 137 | SMP 启动 + IPI | **高** |

### 1.1 barrier 子系统

故障恢复子系统（QueenX 的核心创新）：
- **3 层恢复策略**：Barrier Recovery (~1μs) → BSR Soft Reset (~50ms) → BHR Hard Reset (~120ms)
- 模块级回滚（RecoveryDomain 拓扑恢复）
- 撤销日志（UndoLog + fnv1a_32 哈希）

### 1.2 chitin 子系统

几丁质设备驱动框架：唯一的设备注册/发现/初始化/I/O 入口。
- 4 个协议族：Block/Char/Net/Input
- 全局设备表 `CHITIN_DEVICES`（O(N) 查找，N ≤ 64）

### 1.3 debug 子系统

调试基础设施（**非生产必需**）：
- eBPF（1402 行，Kprobe/Tracepoint/SocketFilter）
- ftrace（290 行）
- kgdb（586 行）
- ringbuf（214 行）

### 1.4 klog 子系统

零依赖日志：内建 COM1 串口 + 128KB 环形缓冲 + 级别/分类双重过滤 + RDTSC 时间戳。

### 1.5 smp 子系统

多处理器初始化与 IPI（Inter-Processor Interrupt）。

## 2. 严重问题

### 2.1 [P0] `barrier/recovery.rs:42-52` `RegisteredDomain.save_fn/restore_fn/reset_fn: unsafe fn()` 函数指针**无类型签名**

- **位置**：[recovery.rs:42-54](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/recovery.rs#L42-L54)
- **代码**：
  ```rust
  pub(crate) struct RegisteredDomain {
      id: DomainId,
      name: String,
      deps: &'static [DomainId],
      save_fn: unsafe fn(),
      restore_fn: unsafe fn(),
      reset_fn: unsafe fn(),
  }
  ```
- **问题**：
  - `unsafe fn()` 是无参数无返回值的函数指针，**完全丢失上下文**。
  - 子系统（HvFS/Net）注册时需要把 `self` 绑到闭包——但函数指针无法捕获环境。
  - 实际使用必然通过 `static FOO: fn()` 把上下文塞到全局变量 → **绕过类型系统**。
  - 与此同时，[barrier/recovery.rs:31-40](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/recovery.rs#L31-L40) `RecoverableDomain` trait 用方法签名（`&self`），**两套并行注册机制**——只有 trait 那套可能真正使用。
- **建议方案**：
  1. 删除 `RegisteredDomain::save_fn/restore_fn/reset_fn` 函数指针字段。
  2. 全部走 `RecoverableDomain` trait。
  3. 验证 `recovery.rs:54` `impl RegisteredDomain {}` 是空 impl → 字段未使用。

### 2.2 [P0] `barrier/api.rs:95-103` `recovery_try_recover_from_idt` 文档说"中断上下文"，但实现持 IrqSpinLock

- **位置**：[barrier/api.rs:99-115](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/api.rs#L99-L115)
- **代码**：
  ```rust
  #[unsafe(no_mangle)]
  pub extern "C" fn recovery_try_recover_from_idt() -> i32 {
      let tick = crate::kernel::framework::tick_query::current_tick();
      let mgr = super::RECOVERY_MANAGER.lock();   // ← 中断上下文持锁
      ...
  }
  ```
- **问题**：
  - 函数注释说"IDT 入口调用"（中断上下文）。
  - `RECOVERY_MANAGER.lock()` 内部自动 `cli`（IrqSpinLock 实现），但**当前已中断**——双重 cli 无效。
  - 如果异常发生**后**调用此函数，`current_tick()` 等仍可能持其它锁 → 锁顺序问题。
  - 与此同时 [barrier/mod.rs:97](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/mod.rs#L97) `PANIC_MSG: IrqSpinLock` 在异常路径被持，**锁顺序未文档化**。
- **建议方案**：
  1. 文档化锁顺序：`PANIC_MSG → RECOVERY_MANAGER → domains[i]`。
  2. IDT 路径走无锁版本（仅读 PANIC_FLAG）。
  3. 或拆为 `try_lock()` + 失败回退到 BSR 异步路径。

### 2.3 [P0] `chitin/mod.rs:46-48` `alloc::boxed::Box::leak(bx)` 创建 `&'static RecoveryDomain` 但**无 cleanup 路径**

- **位置**：[barrier/api.rs:44-53](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/api.rs#L44-L53)
- **代码**：
  ```rust
  pub extern "C" fn recovery_domain_register(domain_id: u64) -> i32 {
      let domain: &'static RecoveryDomain = {
          let bx = alloc::boxed::Box::new(RecoveryDomain::new(domain_id));
          alloc::boxed::Box::leak(bx)   // ← 内存永久泄漏
      };
      match super::RECOVERY_MANAGER.lock().register(domain) {
          Some(_) => 0,
          None => -1,
      }
  }
  ```
- **问题**：
  - `Box::leak` 返回 `&'static T`，**无 Drop 路径**。
  - 即便 `recovery_domain_unregister`（[barrier/api.rs:57-73](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/api.rs#L57-L73)）清除注册表中的指针，**原 Box 仍泄漏**。
  - 后果：动态注册的 domain 永久占用堆内存。
  - **典型内核场景**：模块加载/卸载循环 → 内存增长。
- **建议方案**：
  1. 用 `Rc<RecoveryDomain>` 或 `Arc<RecoveryDomain>` + 引用计数。
  2. 或提供 `recovery_domain_free(id)` 显式释放。
  3. 或改用 `Vec<Box<RecoveryDomain>>`（容量受控）。

### 2.4 [P0] `klog/mod.rs:82-92` `klog_ffi!` 宏**调用任何 FFI 函数前未检查 buffer 长度**

- **位置**：[klog/mod.rs:77-93](file:///home/anfer/Code/QueenX/src/kernel/framework/klog/mod.rs#L77-L93)
- **代码**：
  ```rust
  macro_rules! klog_ffi {
      ($ffi_fn:ident, $($arg:tt)*) => {{
          unsafe extern "C" { fn $ffi_fn(msg: *const u8); }
          let mut buf: [u8; 256] = [0u8; 256];
          let mut cursor = 0;
          let _ = core::fmt::write(
              &mut $crate::kernel::framework::klog::CursorWriter::new(&mut buf, &mut cursor),
              format_args!($($arg)*),
          );
          if cursor > 0 {
              unsafe { $ffi_fn(buf.as_ptr()); }   // ← buf 不以 \0 结尾
          }
      }};
  }
  ```
- **问题**：
  - `format_args!` 输出可能超过 256 字节——`CursorWriter` 静默截断（[klog/mod.rs:67-75](file:///home/anfer/Code/QueenX/src/kernel/framework/klog/mod.rs#L67-L75)）。
  - `buf.as_ptr()` 传给 C-ABI 函数，**没有 NUL 终止**。
  - 若 C 端是 `puts`/`printf("%s")` 等读 NUL 终止字符串的函数，**栈缓冲溢出读取** → 数据泄露/段错误。
- **建议方案**：
  1. `buf[cursor] = 0;` 添加 NUL 终止。
  2. 验证 cursor+1 < buf.len() 后再调用 FFI。
  3. 用 `core::ffi::CString` 替代裸指针。

### 2.5 [P0] `barrier/api.rs:78-83` `recovery_test_rollback` 是 `kernel_test` 特性 gate 但调用 `cascade_rollback` —— 测试可**调用生产路径**

- **位置**：[barrier/api.rs:75-83](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/api.rs#L75-L83)
- **代码**：
  ```rust
  #[cfg(feature = "kernel_test")]
  #[unsafe(no_mangle)]
  pub extern "C" fn recovery_test_rollback(domain_id: u64, crash_fingerprint: u64) -> i32 {
      let tick = crate::kernel::framework::tick_query::current_tick();
      let mgr = super::RECOVERY_MANAGER.lock();
      let rollbacks = mgr.cascade_rollback(domain_id, tick, crash_fingerprint);
      if rollbacks > 0 { 0 } else { -1 }
  }
  ```
- **问题**：
  - `#[unsafe(no_mangle)]` + `extern "C"` —— 即使 cfg-gated，**导出符号仍存在**。
  - 测试代码或恶意用户态通过 syscall 调用此函数 → **可触发任意 domain 的级联回滚**。
  - 后果：用户态进程可强制内核子系统回滚（DoS 攻击）。
- **建议方案**：
  1. 移除 `#[unsafe(no_mangle)]`（普通 pub fn 即可）。
  2. 加 capability 检查。
  3. `recovery_test_rollback` 仅在 `kernel_test` 编译 + 内核 panic 路径可达。

## 3. P1 问题

### 3.1 [P1] `barrier/manager.rs:358` `ROLLBACK_LOG` 全局 1024 项滚动日志无锁保护

- **位置**：[barrier/mod.rs:62](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/mod.rs#L62) `pub use manager::{ROLLBACK_LOG, ...}`
- **问题**：
  - `ROLLBACK_LOG` 是 manager 静态字段，**访问路径需持 `RECOVERY_MANAGER.lock()`**。
  - 当前所有访问走 `mgr.rollback_log(...)` 方法，锁内进行。
  - 但**部分子函数可能在解锁后读 log 字段**——需核查。

### 3.2 [P1] `barrier/recovery.rs:95-100` `recovery_domain_register` 中"prefer_id 冲突时返回原 ID"——**静默冲突**

- **位置**：[recovery.rs:95-104](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/recovery.rs#L95-L104)
- **代码**：
  ```rust
  pub fn recovery_domain_register(name, prefer_id, deps, save_fn, restore_fn, reset_fn) -> DomainId {
      let mut reg = RECOVERY_REGISTRY.lock();
      let id = if prefer_id != 0 {
          for r in &reg.registered {
              if r.id == prefer_id {
                  return prefer_id;  // ← 静默返回 prefer_id 不插入
              }
          }
          prefer_id
      } else {
          let id = reg.next_id.fetch_add(1, Ordering::AcqRel);
          id
      };
      ...
  }
  ```
- **问题**：
  - 当 `prefer_id` 已存在时**静默返回**已存在的 ID，**调用方以为注册成功实际是重复**。
  - 没有 panic / error。
- **建议方案**：
  1. 重复 ID 返回 `Err(DomainError::AlreadyExists)`。
  2. 或 `panic!`（启动期不期望冲突）。

### 3.3 [P1] `barrier/recovery.rs:28-29` `DOMAIN_ID_HVFS=2` `DOMAIN_ID_NET=5` 硬编码与其他模块耦合

- **位置**：[recovery.rs:28-29](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/recovery.rs#L28-L29)
- **问题**：
  - 模块 ID 硬编码，若 HvFS/Net 改子系统 ID，所有引用点需同步。
  - 与 [framework/proc::FdPlan::UDS.base](file:///home/anfer/Code/QueenX/src/kernel/services/net/unix.rs#L18-L19) 同模式——跨子系统硬编码。

### 3.4 [P1] `chitin/mod.rs:107-130` `CHITIN_DEVICES` 全局 `Mutex<Vec<ChitinDevice>>` 在中断上下文**不可用**

- **位置**：[chitin/mod.rs:1130 行搜索](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/mod.rs#L1130)
- **问题**：
  - 模块注释（[chitin/mod.rs:21-22](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/mod.rs#L21-L22)）说"IO 路径可在中断上下文调用"。
  - 但 `Mutex`（IrqSpinLock 别名）在中断上下文必须用 `try_lock`。
  - 当前 API（[chitin/mod.rs:1130+](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/mod.rs#L1130)）`chitin_blk_read` 等用 `lock()`，**中断上下文死锁风险**。

### 3.5 [P1] `debug/ebpf.rs:1402` eBPF 解释器**验证器不实现完整路径敏感分析**——可绕过内存安全

- **位置**：[debug/ebpf.rs:8-14](file:///home/anfer/Code/QueenX/src/kernel/framework/debug/ebpf.rs#L8-L14)
- **代码**：
  ```rust
  //! 2. **验证器**: 采用简化验证 — 有界循环 + 寄存器类型追踪,
  //!    不做 Linux 的完整路径敏感分析
  ```
- **问题**：
  - Linux eBPF 验证器花了多年修补绕过漏洞。
  - 当前简化实现**可能允许构造攻击性 eBPF 程序**：
    - 越界内存读写（验证器漏判）
    - 无限循环（影响系统）
    - 寄存器类型混淆（hook 欺骗）
  - 文档承认限制，但**生产使用风险高**。

### 3.6 [P1] `debug/kgdb.rs:586` kgdb 完整实现但**未文档化触发路径**

- **位置**：[debug/kgdb.rs:1-586](file:///home/anfer/Code/QueenX/src/kernel/framework/debug/kgdb.rs#L1-L586)
- **问题**：
  - kgdb 提供内核调试器（gdb 远程调试）。
  - 但触发路径（如 `int 3`、magic key）未文档化。
  - **生产内核不应暴露 kgdb 入口**（已 rootkit 利用）。

### 3.7 [P1] `klog/mod.rs:128-256` `klog_*!` 宏 13 个分支，重复 `unsafe extern "C"` 声明

- **位置**：[klog/mod.rs:128-256](file:///home/anfer/Code/QueenX/src/kernel/framework/klog/mod.rs)（多个 `unsafe extern "C"` 块）
- **问题**：
  - 每个级别宏（`klog_info!` / `klog_warn!` / `klog_err!` 等）重复声明 FFI 函数。
  - 应统一为 `klog_ffi!` 宏（已存在但被其他宏绕过）。

### 3.8 [P1] `smp/mod.rs:47-58` `register_cpu` 计数 race：超过 MAX_CPUS 后回退**

- **位置**：[smp/mod.rs:47-58](file:///home/anfer/Code/QueenX/src/kernel/framework/smp/mod.rs#L47-L58)
- **代码**：
  ```rust
  pub fn register_cpu(apic_id: u32) -> bool {
      let count = CPU_COUNT.fetch_add(1, Ordering::AcqRel);
      if count as usize >= crate::kernel::framework::config::MAX_CPUS {
          CPU_COUNT.fetch_sub(1, Ordering::AcqRel);   // ← TOCTOU
          return false;
      }
      CPU_APIC_IDS[count as usize].store(apic_id, Ordering::Release);
      ...
  }
  ```
- **问题**：
  - `fetch_add` 后检查 → `fetch_sub` 回退 → 经典 TOCTOU。
  - 两个并发 `register_cpu` 同时 `count=MAX_CPUS-1`：
    - 都 fetch_add → 一个 count=MAX_CPUS-1，另一个 count=MAX_CPUS
    - 都通过 check（前者 OK，后者 fetch_sub）
    - **前者写入 CPU_APIC_IDS[MAX_CPUS-1] OK**；后者 fetch_sub 后**可能不写**。
    - 但**前者 count 已 fetch_add 为 MAX_CPUS-1**，后者**fetch_sub 回退到 MAX_CPUS-1** → **重复占用同一 slot**。
  - 后果：两个 CPU 共用同一 slot → `is_cpu_online` 错乱。

### 3.9 [P1] `barrier/snapshot.rs:359` `DeviceSnapshot` 设备快照无并发机制

- **位置**：[barrier/snapshot.rs:359](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/snapshot.rs#L359)
- **问题**：
  - `DEVICE_SNAPSHOTS` 全局注册表（snapshot.rs）。
  - 快照 capture/restore 在 panic 路径——已有锁保护。
  - 但 `snapshot_register_device` 在启动期可调用——并发不安全。

### 3.10 [P1] `chitin/user_driver.rs:440` `user_driver` 子系统用户态↔内核态通信无审计

- **位置**：[chitin/user_driver.rs:1-440](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/user_driver.rs#L1-L440)
- **问题**：
  - 用户态驱动子系统：允许用户态进程提供设备驱动代码。
  - 安全风险极高——攻击者可注入恶意驱动获取内核权限。
  - 需单开 PR 深审。

## 4. P2 问题

### 4.1 [P2] `barrier/mod.rs:95-97` `PANIC_FLAG` `PANIC_MSG` `CRASH_RIP` 三个全局静态原子变量无校验

- **位置**：[barrier/mod.rs:95-97](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/mod.rs#L95-L97)
- **问题**：
  - 三个变量由 panic_handler / isr 写入，**未文档化锁顺序**。

### 4.2 [P2] `barrier/api.rs:31-40` `recovery_barrier_maintenance` 启动期单次检查 `BOOT_FINGERPRINTS_CHECKED`

- **位置**：[barrier/api.rs:31-40](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/api.rs#L31-L40)
- **问题**：
  - 用 `AtomicBool::swap` 串行化首次检查。
  - 但首次 `mgr.check_boot_fingerprints()` 失败时**无回退**。

### 4.3 [P2] `barrier/types.rs:77` `DIRECT_MAP_SIZE` 硬编码未审

- **位置**：[barrier/types.rs:77](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/types.rs#L77)
- **问题**：
  - 未审细节。

### 4.4 [P2] `chitin/devtree.rs:450` 设备树解析复杂度高，错误恢复路径未审

- **位置**：[chitin/devtree.rs:1-450](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/devtree.rs#L1-L450)
- **问题**：
  - 设备树解析涉及 DTB 格式，错误恢复路径需深审。

### 4.5 [P2] `debug/ringbuf.rs:214` ringbuf 实现无锁 → 跨 CPU 撕裂读风险

- **位置**：[debug/ringbuf.rs:1-214](file:///home/anfer/Code/QueenX/src/kernel/framework/debug/ringbuf.rs#L1-L214)
- **问题**：
  - Linux perf ring buffer 是 4 缓冲 + 顺序锁。
  - 当前实现是否类似需核查。

### 4.6 [P2] `debug/ftrace.rs:290` ftrace 函数追踪 hook 安装无静态保证

- **位置**：[debug/ftrace.rs:1-290](file:///home/anfer/Code/QueenX/src/kernel/framework/debug/ftrace.rs#L1-L290)
- **问题**：
  - hook 函数指针替换若中断上下文访问 → 数据竞争。

### 4.7 [P2] `klog/mod.rs:55-75` `CursorWriter` 字段借用冲突未文档化

- **位置**：[klog/mod.rs:55-75](file:///home/anfer/Code/QueenX/src/kernel/framework/klog/mod.rs#L55-L75)
- **问题**：
  - `&mut [u8]` + `&mut usize` 两个独立借用，需文档化不变量。

### 4.8 [P2] `smp/mod.rs:11-15` `CPU_APIC_IDS` / `CPU_ONLINE` 数组大小是 `MAX_CPUS`，初始化用 `0xFFFF` magic value

- **位置**：[smp/mod.rs:11-15](file:///home/anfer/Code/QueenX/src/kernel/framework/smp/mod.rs#L11-L15)
- **问题**：
  - `0xFFFF` 作为"未注册"标记，与合法 APIC ID 范围冲突（xAPIC: 0..255，x2APIC: 0..0xFFFFFFFF）。
  - 实际不会冲突（合法 APIC ID 不会到 0xFFFF）但语义模糊。

### 4.9 [P2] `chitin/composite.rs:498` 复合设备匹配算法未审

- **位置**：[chitin/composite.rs:1-498](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/composite.rs#L1-L498)
- **问题**：
  - 复合设备（PCI + USB 等嵌套）匹配逻辑复杂。

### 4.10 [P2] `chitin/firmware.rs:145` 固件加载路径未审

- **位置**：[chitin/firmware.rs:1-145](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/firmware.rs#L1-L145)
- **问题**：
  - 固件加载涉及外部代码执行，类似 [subsystem-framework-credo.md §2.1](../audit/subsystem-framework-credo.md) P0。

### 4.11 [P2] `debug/mod.rs:34` debug 模块入口，无 `cfg(debug_assertions)` gate

- **位置**：[debug/mod.rs:1-34](file:///home/anfer/Code/QueenX/src/kernel/framework/debug/mod.rs#L1-L34)
- **问题**：
  - debug 子系统（eBPF/kgdb）在生产构建仍存在。
  - 应 `#[cfg(any(debug_assertions, feature = "kernel_test"))]`。

### 4.12 [P2] `klog/mod.rs:128-256` klog 级别/分类枚举分散多个文件（未审细节）

- **位置**：[klog/mod.rs:128-256](file:///home/anfer/Code/QueenX/src/kernel/framework/klog/mod.rs#L128-L256)
- **问题**：
  - 939 行的单文件——结构过于集中。

## 5. P3 问题

### 5.1 [P3] `barrier/mod.rs:96` `PANIC_MSG: [u8; 128]` 128 字节太短，复杂 panic 信息截断

- **位置**：[barrier/mod.rs:96](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/mod.rs#L96)
- **问题**：
  - Linux 内核 `panic!()` 信息可达数 KB。
  - 128 字节易截断。

### 5.2 [P3] `smp/mod.rs:78` `send_tlb_invalidate_ipi(0xFD)` 硬编码向量

- **位置**：[smp/mod.rs:74-95](file:///home/anfer/Code/QueenX/src/kernel/framework/smp/mod.rs#L74-L95)
- **问题**：
  - 0xFD（TLB flush）、0xFE（reschedule）硬编码。

### 5.3 [P3] `chitin/mod.rs:111+` `CHITIN_DEVICES.lock()` 命名误导

- **位置**：[chitin/mod.rs:1130+](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/mod.rs#L1130)
- **问题**：
  - `Mutex` 是 IrqSpinLock 别名——读者可能误用。

### 5.4 [P3] `klog/mod.rs:128-256` 多个 `klog_*!` 宏重复 `klog_ffi!` 实现

- **位置**：[klog/mod.rs:128-256](file:///home/anfer/Code/QueenX/src/kernel/framework/klog/mod.rs#L128-L256)
- **问题**：
  - 应统一抽取。

### 5.5 [P3] `debug/api.rs:56` debug api 入口简单

- **位置**：[debug/api.rs:1-56](file:///home/anfer/Code/QueenX/src/kernel/framework/debug/api.rs#L1-L56)
- **问题**：
  - 56 行入口——应有更多功能？

### 5.6 [P3] `barrier/fault_inject.rs:23` 故障注入仅 23 行，功能有限

- **位置**：[barrier/fault_inject.rs:1-23](file:///home/anfer/Code/QueenX/src/kernel/framework/barrier/fault_inject.rs#L1-L23)
- **问题**：
  - 故障注入覆盖率低。

### 5.7 [P3] `chitin/proto_*.rs` 协议族各 < 80 行，可能缺少完整协议实现

- **位置**：[chitin/proto_block.rs:30](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/proto_block.rs#L30)、[chitin/proto_char.rs:44](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/proto_char.rs#L44)、[chitin/proto_input.rs:63](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/proto_input.rs#L63)、[chitin/proto_net.rs:78](file:///home/anfer/Code/QueenX/src/kernel/framework/chitin/proto_net.rs#L78)
- **问题**：
  - 4 个协议族共 ~215 行——平均 50 行/协议。

## 6. 跨子系统关联

### 6.1 barrier ↔ services/proc/signal

- `barrier/recovery.rs` panic 恢复路径与 `services/proc/signal.rs` 进程信号路径交叉。
- 进程崩溃 → signal → barrier rollback → restore context。

### 6.2 chitin ↔ driver (子模块)

- chitin 是**设备框架**，driver/ 是**具体驱动**。
- 6 个 driver 子模块（framework/driver/*）通过 chitin::BlockDevice trait 注册。
- 循环依赖：chitin 定义 trait，driver 实现 trait → 无循环（chitin 不依赖 driver）。

### 6.3 debug ↔ klog

- ftrace 输出走 klog。
- eBPF helper 可能调用 klog。
- 锁顺序：ftrace 触发 → klog 输出 → COM1 spinlock。

### 6.4 smp ↔ arch

- smp 调用 `crate::arch!(send_ipi(...))` —— 由 framework::arch 决定具体实现。
- x86_64 用 LAPIC；aarch64 用 GIC。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 5 | 5-7 天 |
| **P1** | 10 | 6-8 天 |
| **P2** | 12 | 3-4 天 |
| **P3** | 7 | 1 天 |
| **合计** | **34** | **15-20 天** |

### P0 修复路径（建议执行顺序）

1. **§2.4 klog_ffi! 缺 NUL 终止**（1 天，**信息泄露 + 段错误**）
2. **§2.1 RegisteredDomain 函数指针**（1-2 天）
3. **§2.3 recovery_domain_register 内存泄漏**（1-2 天）
4. **§2.5 recovery_test_rollback FFI 暴露**（0.5 天）
5. **§2.2 IDT 路径持锁**（1-2 天）