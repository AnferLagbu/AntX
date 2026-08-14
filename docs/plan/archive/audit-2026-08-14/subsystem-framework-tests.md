# framework/tests 子系统深度审计报告

> **审计范围**：`src/kernel/framework/tests/`（26 文件）
> **审计日期**：2026-08-14
> **文件数**：26 个源文件
> **代码规模**：约 6,682 LoC（含测试 + 注释）
> **总体结论**：✅ 0 unsafe / ⚠️ **19 个问题（P0×3, P1×7, P2×6, P3×3）**

## 1. 子系统概览

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs) | 468 | TestRunner / TestRegistry / 测试宏 + qemu_exit | **高** |
| [test_pi_mutex.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_pi_mutex.rs) | 295 | PI Mutex 状态机测试 | 中 |
| [test_pwm.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_pwm.rs) | 255 | Credo PWM 单元测试 | 中 |
| [test_new_features.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_new_features.rs) | 428 | 新功能集成测试 | 中 |
| [test_config.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_config.rs) | 340 | 配置子系统测试 | 中 |
| [test_smp.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_smp.rs) | 306 | SMP 多核测试 | **高** |
| [test_hvfs.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_hvfs.rs) | 306 | HvFS 文件系统测试 | 中 |
| [test_hvfs_ext.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_hvfs_ext.rs) | 283 | HvFS 扩展测试 | 中 |
| [test_ipc.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_ipc.rs) | 191 | IPC 子系统测试 | 中 |
| [test_uds.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_uds.rs) | 200 | Unix Domain Socket 测试 | 中 |
| [test_barrier_ext.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_barrier_ext.rs) | 214 | 内存屏障扩展测试 | 低 |
| [test_proc.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_proc.rs) | 173 | 进程管理测试 | 中 |
| [test_vfs.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_vfs.rs) | 183 | VFS 测试 | 中 |
| [test_devfs.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_devfs.rs) | 128 | devfs 测试 | 中 |
| [test_mm.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_mm.rs) | 162 | 内存管理测试 | 中 |
| [test_barrier.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_barrier.rs) | 120 | 内存屏障基础测试 | 低 |
| 其他 11 个 | < 100 | 各模块单元测试 | 低 |

### 1.2 子系统职责

`framework/tests` 是 QueenX 内核的**测试基础设施**：
- `TestRunner` + `TestRegistry` 全局测试运行器
- 26 个测试模块，覆盖 22+ 子系统
- 集成 QEMU 测试模式（`kernel_test` feature）+ serial_print 输出
- 通过 `qemu_exit(success)` 终止 QEMU 模拟器

## 2. 严重问题

### 2.1 [P0] `mod.rs:411-429` `test_runner_init` 中的"PAGETABLE 诊断"硬编码物理地址 `0x109000` 触发 speculative read

- **位置**：[mod.rs:416-429](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L416-L429)
- **代码**：
  ```rust
  // 诊断: test 运行前检查页表
  {
      let read_u64 = |phys: u64, idx: usize| -> u64 {
          let va = phys + crate::kernel::framework::mm::KERNEL_BASE + idx as u64 * 8;
          unsafe { core::ptr::read_volatile(va as *const u64) }
      };
      let pd24 = read_u64(0x109000, 24);
      let pd63 = read_u64(0x109000, 63);
      crate::klog_boot_info!("[PAGETABLE] before run_all: pd[24]=0x{:016X} pd[63]=0x{:016X}", pd24, pd63);
  }
  ```
- **问题**：
  - **生产路径**硬编码 `phys=0x109000` 读 PML4 entry——这是 QEMU 特定 boot 配置的 PML4 物理地址。
  - 在真实硬件（如物理机或非 QEMU 启动）上，**0x109000 可能是未映射或无关物理页**。
  - `read_volatile` 读取该地址 → 若未映射触发 page fault（**内核崩溃**）。
  - 即使已映射，**读 PML4 entry 不应发生在 `test_runner_init`**——这是 debug 残留代码。
- **建议方案**：
  1. 删除该诊断块。
  2. 或 `#[cfg(debug_assertions)]` 保护。
  3. 或用 `pmm_alloc_pages` 获取实际 PML4 物理地址。

### 2.2 [P0] `mod.rs:113-161` `run_all` 在持锁 `reg` 状态下调用 `func()` 测试函数，**测试函数可死锁**

- **位置**：[mod.rs:112-160](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L112-L160)
- **代码**：
  ```rust
  pub fn run_all(&self) {
      let reg = self.registry.lock();   // ← 持锁
      ...
      for i in 0..total {
          let tc = reg.cases[i];
          ...
          let result = func();           // ← 测试函数在锁内运行
          ...
      }
      drop(reg);
      ...
  }
  ```
- **问题**：
  - `IrqSpinLock<>` 持锁期间**禁止睡眠 / 持其它锁 / 阻塞**。
  - `func()` 测试函数可能持其它 IrqSpinLock 或分配（[`GFP_KERNEL` 路径][framework/sync/pi_mutex.rs P0-08]）。
  - 若测试函数触发 panic 或 abort，**IrqSpinLock 永远不释放** → 系统挂死。
  - 与 [`audit_services_boundary.py`][AGENTS.md §2.2] 规则潜在冲突。
- **建议方案**：
  1. **复制注册表到本地 Vec，**释放**释放锁**后再逐个执行测试**：
     ```rust
     let cases: Vec<TestCase> = {
         let reg = self.registry.lock();
         reg.cases[..reg.count].to_vec()
     };
     for tc in cases { tc.func(); }
     ```
  2. 或用 RAII lock guard + 手动 drop（当前代码已 drop，但 lock 内仍执行 func）。

### 2.3 [P0] `mod.rs:122` `crate::arch!(interrupt_disable())` 在测试运行前**永久关闭中断**

- **位置**：[mod.rs:122](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L122)
- **代码**：
  ```rust
  crate::arch!(interrupt_disable());   // ← cli/sti 关闭中断
  for i in 0..total { ... }             // ← 中断被永久关闭
  ```
- **问题**：
  - `interrupt_disable()` 不对应 `interrupt_enable()`——**永久关闭中断**直到下次手动 enable。
  - 测试运行期间**所有硬件中断被屏蔽**：
    - 时钟中断（tick）→ 调度器饿死
    - 串口 RX → 用户输入丢失
    - NIC 中断 → 网络丢包
  - 后果：测试期间**任何中断驱动的功能停止工作**。
  - 同时**测试运行结束也没 enable 中断**，kernel 启动后中断永久关闭。
- **建议方案**：
  1. 保存中断状态：`let was_enabled = interrupt_save();`  → 测试结束 `if was_enabled { interrupt_enable(); }`。
  2. 或仅在 `kernel_test` 模式下关闭中断（生产模式不应关闭）。

## 3. P1 问题

### 3.1 [P1] `mod.rs:82-88` `register` 静默丢弃超容量测试用例（无 panic/warning）

- **位置**：[mod.rs:83-89](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L83-L89)
- **代码**：
  ```rust
  fn register(&mut self, module: &'static str, name: &'static str, func: TestFn) {
      if self.count < MAX_TESTS {
          self.cases[self.count] = TestCase { module, name, func };
          self.count += 1;
      }
      // ← else: 静默丢弃
  }
  ```
- **问题**：
  - 当 `count >= MAX_TESTS (256)` 时新测试用例**被静默丢弃**。
  - `test_runner_init` 输出 "Registered N test cases"，N 不含被丢弃的测试。
  - 后果：测试用例"看似注册但实际未运行"，**测试覆盖被静默减少**。
- **建议方案**：
  1. 超容量时 `panic!` 或返回错误。
  2. 至少 `klog_warn!` 记录。

### 3.2 [P1] `mod.rs:255` `serial_print_num` 重复实现数字转字符串（与 framework::klog 重复）

- **位置**：[mod.rs:267-290](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L267-L290)
- **代码**：
  ```rust
  pub fn serial_print_num(mut n: u64) {
      if n == 0 {
          serial_print(b"0");
          return;
      }
      let mut buf = [0u8; 20];
      let mut pos = 0usize;
      while n > 0 {
          buf[pos] = (n % 10) as u8 + b'0';
          pos += 1;
          n /= 10;
      }
      for i in (0..pos).rev() {
          unsafe { ... port_outb(COM1, buf[i]); }
      }
  }
  ```
- **问题**：
  - x86_64 / aarch64 两个分支都实现数字格式化逻辑。
  - 与 `framework/klog` 的格式化函数重复——应统一调用。
- **建议方案**：
  1. 抽取公共 `framework::klog::print_num_u64(n)`。
  2. 或 `format!` + 串口输出。

### 3.3 [P1] `test_pi_mutex.rs:295` 注释声称"覆盖 PI Mutex 状态机的关键路径"，但未测试 P0-08 (`pi_mutex_process_exit` 死代码)

- **位置**：[test_pi_mutex.rs:1-12](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_pi_mutex.rs#L1-L12)
- **代码**：
  ```rust
  //! 覆盖 PI Mutex 状态机的关键路径:
  //! - 基本 lock/unlock
  //! - try_lock 失败路径
  //! - 直接捐赠 ...
  ```
- **问题**：
  - 测试声称"覆盖关键路径"，**但 [subsystem-sync.md §2.x P0-08](../audit/subsystem-sync.md) 揭示 `pi_mutex_process_exit` 是死代码**。
  - 没有任何测试验证进程退出时 PI Mutex 是否被 force_unlock。
  - 测试盲区 = **生产 bug 隐藏地**。
- **建议方案**：
  1. 添加 `test_process_exit_releases_pi_mutex`：模拟持锁 → 模拟进程退出 → 验证 mutex 可被新 holder 获取。
  2. 测试先发现 `pi_mutex_process_exit` 死代码问题。

### 3.4 [P1] `test_pwm.rs:255` 未测试 `pwm_create_first_identity` / `pwm_try_genesis` 路径

- **位置**：[test_pwm.rs:1-255](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_pwm.rs#L1-L255)
- **问题**：
  - 仅测试 SHA-256、类型定义、CapBits。
  - **未测试核心身份管理流程**：
    - `pwm_create(password, note, creator)`
    - `pwm_verify_password(pwm, password)`
    - `pwm_grant` / `pwm_revoke`
    - `pwm_check_privilege`
    - `secure_boot::verify_image`（[subsystem-framework-credo.md §2.1](../audit/subsystem-framework-credo.md) P0）
- **建议方案**：
  1. 添加身份生命周期集成测试。
  2. 测试 Ed25519 签名验证当前占位行为（应失败）。

### 3.5 [P1] `test_new_features.rs:428` 集成测试 `MAX_TESTS=256` 在多个模块累计测试超过 256 时会被静默截断

- **位置**：[test_new_features.rs:1-428](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_new_features.rs#L1-L428)
- **问题**：
  - 26 个测试模块，每个 5-15 个测试用例，**总计可能 > 256**。
  - `register` 静默丢弃（见 §3.1），**末尾模块的测试根本不运行**。
- **建议方案**：
  1. 提升 MAX_TESTS 到 1024。
  2. 或实现动态 Vec。

### 3.6 [P1] `test_smp.rs:306` SMP 测试在 QEMU 单核模拟下完全跳过，但断言代码可能被绕过

- **位置**：[test_smp.rs:1-306](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_smp.rs#L1-L306)
- **问题**：
  - 多核测试需要 `-smp N` 启动 QEMU；CI 单核模式可能跳过。
  - 若测试用例静默通过（`Pass`），无法区分"真通过"与"被跳过"。
- **建议方案**：
  1. 用 `TestResult::Skip("SMP not available")` 显式标记。
  2. 加 `count_active_cpus() == 1` 校验。

### 3.7 [P1] `mod.rs:294-298` `runner()` 使用 OnceLock 但 `slot.write(TestRunner::new())` 不返回值校验

- **位置**：[mod.rs:292-298](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L292-L298)
- **代码**：
  ```rust
  static TEST_RUNNER: OnceLock<TestRunner> = OnceLock::new();

  pub fn runner() -> &'static TestRunner {
      TEST_RUNNER.get_or_init(|slot| {
          slot.write(TestRunner::new());
      })
  }
  ```
- **问题**：
  - `OnceLock::get_or_init` 要求闭包返回 `T`——当前闭包不返回值（unit `()`）。
  - 与 [framework/credo/identity.rs:613-621](../audit/subsystem-framework-credo.md) `OnceLock<IdentityTable>` 同模式——可能在 OnceLock 上有 monkey patch 或 IdentityTable 有 Default impl。
  - 但 `TestRunner` 无 Default impl——**编译期应该报错**。
  - 如果能编译过，**说明 OnceLock 的 `slot.write` 旁路了初始化路径**，可能有隐藏 bug。
- **建议方案**：
  1. 实现 `TestRunner::default()` 或 `TestRunner::new()` 的 auto-init。
  2. 验证 cargo check 是否真的能编译。

## 4. P2 问题

### 4.1 [P2] `mod.rs:300-336` 4 个测试宏（check/assert_eq_test/skip_test/register_tests_inner）未导出文档

- **位置**：[mod.rs:300-336](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L300-L336)
- **问题**：
  - `check!` / `assert_eq_test!` / `skip_test!` 行为接近标准 `assert!`，但**无文档**说明与标准 assert 的差异。
  - 调用方误用 `assert!`（panic 而非 TestResult::Fail）导致测试结果混乱。

### 4.2 [P2] `mod.rs:340-342` `test_runner_init` 硬编码 klog_boot_info 字符串格式

- **位置**：[mod.rs:344-345](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L344-L345)
- **问题**：
  - "[TEST] === QueenX Test Framework ===" 横幅固定，无法 CI 自定义。

### 4.3 [P2] `mod.rs:292` `TEST_RUNNER: OnceLock<TestRunner>` 全局但未提供 `#[cfg(feature = "kernel_test")]` gate

- **位置**：[mod.rs:292-298](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L292-L298)
- **问题**：
  - `OnceLock<TestRunner>` 在生产构建也存在（即使 `kernel_test` 未启用）。
  - `OnceLock::new()` 是 const，无运行时开销，但增加了二进制体积。

### 4.4 [P2] `mod.rs:361-408` `test_runner_init` 内 30+ `register_*_tests()` 调用列表硬编码

- **位置**：[mod.rs:354-408](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L354-L408)
- **问题**：
  - 每个新测试模块都需修改此列表——容易遗漏。
  - 应改为自动发现（如 `inventory` 或 `ctor`）。

### 4.5 [P2] `test_barrier_ext.rs:214` 与 `test_barrier.rs:120` 重复注册 barrier 测试

- **位置**：[test_barrier_ext.rs:1-214](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_barrier_ext.rs#L1-L214)
- **问题**：
  - 两个文件测试相似的 barrier 功能——重复或互补？

### 4.6 [P2] `test_hvfs.rs` 与 `test_hvfs_ext.rs` 测试覆盖重叠

- **位置**：[test_hvfs.rs:306](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_hvfs.rs#L306)、[test_hvfs_ext.rs:283](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/test_hvfs_ext.rs#L283)
- **问题**：
  - 类似 `test_hvfs` 与 `test_hvfs_ext` 分裂——可能是历史遗留。

## 5. P3 问题

### 5.1 [P3] `mod.rs:43-49` `TestResult::Fail(&'static str)` 字符串引用，**无法携带详细诊断**

- **位置**：[mod.rs:43-49](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L43-L49)
- **问题**：
  - 失败信息仅支持 `&'static str`，无法携带现场值（如 "expected X but got Y"）。

### 5.2 [P3] `mod.rs:64-68` `NOOP_CASE` 公共静态但无用途说明

- **位置**：[mod.rs:64-68](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L64-L68)
- **问题**：
  - 用途不清，仅用于初始化 `[NOOP_CASE; MAX_TESTS]`。

### 5.3 [P3] `mod.rs:162` `drop(reg);` 是 no-op（lock guard 自动 drop）

- **位置**：[mod.rs:162](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L162)
- **问题**：
  - `let reg = self.registry.lock();` 在循环结束自动 drop，显式 `drop(reg)` 是冗余。

## 6. 跨子系统关联

### 6.1 tests ↔ framework 全模块

- `test_runner_init` 注册了 22+ framework 子模块的测试。
- **测试模块必须与生产模块同步**——任何生产 bug，测试必须能发现。
- 当前 P0-08（pi_mutex_process_exit 死代码）**测试盲区**就是典型反例。

### 6.2 tests ↔ QEMU

- `qemu_exit(success)` 通过 `out 0xf4, al` ISA-debug exit 调用。
- 必须与 `run-qemu.sh` 配置一致。

### 6.3 tests ↔ 引导流程

- `test_runner_init` 在引导流程中调用位置决定测试何时运行。
- 当前在 `test_runner_init`（[mod.rs:344](file:///home/anfer/Code/QueenX/src/kernel/framework/tests/mod.rs#L344)）由 `kernel_main` 显式调用。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 3 | 2-3 天 |
| **P1** | 7 | 3-4 天 |
| **P2** | 6 | 1-2 天 |
| **P3** | 3 | 0.5 天 |
| **合计** | **19** | **7-10 天** |

### P0 修复路径（建议执行顺序）

1. **§2.3 永久关闭中断**（1 天，**生产安全 bug**）
2. **§2.2 持锁运行测试**（0.5 天）
3. **§2.1 PAGETABLE 诊断硬编码**（0.5 天）
4. **§3.1 MAX_TESTS 静默丢弃**（0.5 天）

### P1 修复路径

5. **§3.3 PI Mutex 死代码未测试**（0.5 天，可单开 PR 修复）
6. **§3.4 PWM 集成测试缺失**（1-2 天）
7. **§3.6 SMP 跳过机制**（0.5 天）
8. **§3.2 serial_print_num 重复**（0.5 天）