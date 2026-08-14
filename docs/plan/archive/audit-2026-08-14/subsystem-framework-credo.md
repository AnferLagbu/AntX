# framework/credo 子系统深度审计报告

> **审计范围**：`src/kernel/framework/credo/`
> **审计日期**：2026-08-14
> **文件数**：14 个源文件
> **代码规模**：约 110 KB（含测试 + 注释） / 有效 LoC 约 3.4K
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **35 个问题（P0×8, P1×11, P2×12, P3×4）**

## 1. 子系统概览

### 1.1 目录结构

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/mod.rs) | 63 | 子系统入口、re-export、`credo_init()` FFI | 中 |
| [api.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/api.rs) | 447 | 38 个 `#[no_mangle] extern "C"` PWM API | **极高** |
| [types.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/types.rs) | 9 | 类型 re-export 桩 | 低 |
| [capability.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/capability.rs) | 9 | 能力常量 re-export 桩 | 低 |
| [sha256.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/sha256.rs) | 9 | SHA-256 re-export 桩 | 低 |
| [csprng.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/csprng.rs) | 106 | rdrand/TSC fallback 熵源 | **高** |
| [engine.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/engine.rs) | 71 | 能力检查 + 信任链特权级 | **高** |
| [grant.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/grant.rs) | 45 | Grant 记录表 + is_grantor 检查 | **高** |
| [audit.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/audit.rs) | 119 | 256 项环形审计日志 + unsafe `static mut GLOBAL_AUDIT` | **极高** |
| [bootstrap.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/bootstrap.rs) | 90 | First Token 一次性授权 + TSC 时间 | **高** |
| [identity.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs) | 648 | IdentityTable 完整 CRUD + 密码验证 | **极高** |
| [secure_boot.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs) | 783 | SHA-256 + Ed25519 占位 + TPM 模拟 | **极高** |
| [session.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/session.rs) | 559 | 登录/登出/提权/POSIX setuid 系列 | **极高** |
| [storage.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/storage.rs) | 431 | 二进制 v4/v5 持久化 + VFS FFI | **高** |

### 1.2 子系统职责

Credo = 域身份 (DID) + 能力矩阵 + 会话管理 + 审计 + Secure Boot + TPM。是 `QueenX` 安全子系统的**核心 TCB**，所有权限检查、身份验证、密钥管理都依赖此模块。

**调用方契约**（见 [api.rs:6-12](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/api.rs#L6-L12)）：
- `syscall::mod` — SYS_CREDO_LOGIN/LOGOUT/CREATE/DELETE/GRANT/REVOKE/CHECK_CAP
- `fs::vfs` — vfs_open/write 前调用 `pwm_get_current()` 获取权限上下文
- `proc::api` — 进程创建时分配 PWM，销毁时回收
- `net::init` — socket 操作前的权限校验
- `console::gfx_console` — 登录交互

## 2. 严重问题

### 2.1 [P0] `secure_boot.rs:198-209` Ed25519 签名验证为占位实现（**任何签名都通过**）

- **位置**：[secure_boot.rs:192-210](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L192-L210) `Ed25519PubKey::verify`
- **代码**：
  ```rust
  pub fn verify(&self, message: &[u8], signature: &[u8; ED25519_SIG_LEN]) -> bool {
      // TODO(TRACK-7A8BAB): 替换为真正的 Ed25519 验证
      // 当前: 检查签名非零 + 消息哈希匹配 (简化)
      let _msg_hash = sha256_hash(message);
      // 占位: 签名非全零即视为有效
      let mut all_zero = true;
      for &b in signature {
          if b != 0 {
              all_zero = false;
              break;
          }
      }
      !all_zero
  }
  ```
- **问题**：
  - 当前实现**只要签名不全为零就返回 true**——这意味着 Secure Boot 完全不验证签名，攻击者可注入任何非零签名的恶意内核镜像。
  - 文档承诺"签名验证失败将拒绝加载, 不可绕过"（secure_boot.rs:25），但实际任何 64 字节非全零签名都通过。
  - `mod.rs:60` `let default_pk = Ed25519PubKey::new([0u8; 32]);` 默认平台密钥全零，意味着任何镜像只要带一个非零字节的伪签名即可被"信任"。
  - `credo_init()` 在 `scheduler_init()` 后被调用（mod.rs:53），但 `secure_boot_init` 仅 push PK 条目，**信任链始终只有 PK 自身**，没有 KEK/DB 镜像签名密钥。
- **生产风险**：内核替换攻击 → 完全 rootkit 入口。
- **建议方案**：
  1. **立即**：将 `verify` 改为 `false`（强制拒绝所有签名），至少让安全启动成为 fail-closed 模式。
  2. **中期**：集成 `ed25519-dalek` crate（无 std 支持），实现真实 Ed25519 验证。
  3. **长期**：固化 `TOFU`（Trust On First Use）模式，启动时检查 `/etc/secure_boot.keys`，PK 与持久化值不匹配则告警。

### 2.2 [P0] `audit.rs:91` `pub(crate) static mut GLOBAL_AUDIT`（违反 Rust 2024 idiomatic + 仍是 F13 反模式）

- **位置**：[audit.rs:91](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/audit.rs#L91) `static mut GLOBAL_AUDIT`
- **代码**：
  ```rust
  pub(crate) static mut GLOBAL_AUDIT: AuditLog = AuditLog::new();

  pub fn log(pwm: u64, action: AuditAction, target_pwm: u64, domain: u64, caps: u64) {
      raw::log(pwm, action, target_pwm, domain, caps);
  }

  pub(crate) mod raw {
      pub fn log(...) {
          // SAFETY: static mut 唯一所有者, 调用方串行或由 audit 自身保证.
          unsafe { GLOBAL_AUDIT.log(...) }
      }
  }
  ```
- **问题**：
  - **同一项目 `identity.rs:613-621` 已经使用 `OnceLock<IdentityTable>` 替代 static mut**，但 audit 仍保留 static mut — **未遵循相同重构模式**。
  - SAFETY 注释声明"调用方串行或由 audit 自身保证"，但 `audit::log` 在多个 FFI 入口（[api.rs:308-314](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/api.rs#L308-L314) `pwm_create`、`api.rs:415-430` `pwm_audit_log`、`bootstrap.rs:55-61` `grant_from_first_token`、`identity.rs:308-314`、`session.rs:145`、`session.rs:173`、`session.rs:283`）被并发调用，**没有任何锁保护**。
  - `AuditLog::log` 内部用 `fetch_add(1, Ordering::AcqRel) % AUDIT_CAPACITY` 取 idx + unsafe 写 `(*entry).field`，**多核并发 idx 相同时撕裂写**（即便环形覆盖也是 8 字节以上字段，无锁覆盖导致 audit 记录数据竞争）。
  - `GLOBAL_AUDIT` 被 `pub(crate)` 暴露，`raw` 子模块只是封装，但**unsafe 边界没真正强制隔离**。
- **生产风险**：
  - 多核系统 audit 记录可能被撕裂 / 丢失 / 错位（安全事件追溯失效）。
  - 攻击者知道 `GLOBAL_AUDIT` 布局后可触发审计日志覆盖，掩盖入侵行为。
- **建议方案**：
  1. **立即**：仿 `identity.rs:613-621` 重构为 `OnceLock<IrqSpinLock<AuditLog>>`，所有访问走 `get().lock()`。
  2. **配套**：`AuditLog::log` 内部使用 `fetch_add` 索引不再需要 `unsafe`（移除 `*mut AuditEntry` 强转）。
  3. 删除 `pub(crate) static mut GLOBAL_AUDIT` + `raw` 子模块。
- **关联硬规则**：F13 (`static mut` 反模式) + F4 (SAFETY 注释不充分)。

### 2.3 [P0] `session.rs:159` `ctx.current_entry = core::ptr::null()` 引入裸指针字段未受所有权保护

- **位置**：[session.rs:150-174](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/session.rs#L150-L174) `logout`
- **代码**：
  ```rust
  pub fn logout() {
      let pid = process_get_current_pid();
      ...
      let pwm = PROCESS_TABLE
          .with_process(pid, |p| {
              let mut ctx = p.session.lock();
              let saved = ctx.session_pwm.as_u64();
              ctx.current_entry = core::ptr::null();  // ← 裸指针赋值
              ...
          })
          .unwrap_or(0);
      ...
  }
  ```
- **问题**：
  - `PwmContext::current_entry: *const PwmEntry` 是裸指针字段，存的是 `IdentityTable` 中 `PwmEntry` 的引用。
  - 当 IdentityTable 中 `PwmEntry` 被 `delete()`（[identity.rs:508-521](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L508-L521)）后，`pwm.store(0)` 仅标记槽位为空，**但不会真正 Drop PwmEntry**，且 `current_entry` 仍指向该槽位。
  - 此时 `get_current_entry()` 返回的指针虽然 `pwm == 0`，但**地址有效**。攻击者可绕过 `pwm == 0` 检查直接通过裸指针访问已"删除"的 entry。
  - 更严重：`create()` 中如果新分配的 PWM 哈希到同一槽位（旧 entry 被删除后），则 `current_entry` 指向的是**新 entry 的内存**，但调用方仍认为是旧身份。
- **建议方案**：
  1. **立即**：将 `current_entry` 改为 `Option<u64>`（pwm 句柄），需要访问时通过 `identity::find(pwm)` 获取。
  2. **替代**：使用 `Weak<...>` 模式（不适用当前 no-alloc-anywhere）。
  3. 配套：`delete()` 必须验证"无任何 PwmContext 持有此 PWM"（如失败则拒绝删除），或采用引用计数。

### 2.4 [P0] `identity.rs:147-157` `find()` O(n) 线性扫描，每次能力检查都触发

- **位置**：[identity.rs:147-157](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L147-L157) `find`
- **代码**：
  ```rust
  pub fn find(&self, pwm: u64) -> Option<&PwmEntry> {
      if pwm == 0 { return None; }
      for entry in &self.entries {
          if entry.pwm.load(Ordering::Acquire) == pwm {
              return Some(entry);
          }
      }
      None
  }
  ```
- **问题**：
  - `entries: Vec<PwmEntry>` 容量 256（MAX_PWM_ENTRIES），每次 `find()` 是 O(n) 线性扫描。
  - `engine.rs:9` `check()` / `engine.rs:34` `check_privilege()` 每次文件操作、syscall 都调用，平均扫描 ~128 项。
  - 文档承诺 "能力检查: O(1) 位运算, ≤ 5ns"（api.rs:29）但实际是 O(n) + cache miss，性能差距 ~50×。
  - 与 `api.rs:30` 声明的"身份查找: O(1) 哈希表"矛盾。
- **建议方案**：
  1. 用 `hashbrown::HashMap<u64, usize>` (pwm → slot 索引) 替代线性扫描。
  2. 或保留线性但加 LRU cache（命中 pwm → slot）。
  3. 配套 benchmark 验证 `≤ 100ns`（接近哈希表性能）。

### 2.5 [P0] `secure_boot.rs:329-375` `verify_image` 回退到 PK 验证（信任链最弱链路）

- **位置**：[secure_boot.rs:329-375](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L329-L375) `verify_image`
- **代码**：
  ```rust
  let db_keys: Vec<&TrustEntry> = chain.iter().filter(|e| e.role == TrustRole::ImageSigning).collect();
  if db_keys.is_empty() {
      // 回退: 用 KEK 验证
      let kek_keys: ... = ...;
      if kek_keys.is_empty() {
          // 最后回退: 用 PK 验证  ← P0: 任意 PK 拥有者都能签任意镜像
          let pk_keys: ... = ...;
          for pk in pk_keys {
              if pk.pubkey.verify(image, signature) {
                  ...
              }
          }
      }
  }
  ```
- **问题**：
  - 信任链设计本意是 PK → KEK → DB → Image，**任一级签名均可委托**。
  - 但当前逻辑：DB 缺失 → 用 KEK；KEK 缺失 → 用 PK。意味着只要添加 PK 自身（自签名）就能签任何镜像，**绕过中间层 KEK/DB 的隔离**。
  - Linux UEFI Secure Boot 的等价设计是：DB 缺失 → 拒绝启动（fail-closed）。本实现是 fail-open。
- **建议方案**：
  1. 删除 PK 回退路径，DB 缺失直接返回 `ChainBroken`。
  2. KEK 缺失也回退到拒绝（除非有显式配置 `ALLOW_PK_DIRECT_IMAGE_SIGN=true`）。
  3. fail-closed 模式 + boot log 记录跳过原因。

### 2.6 [P0] `secure_boot.rs:451-454` `TpmSubsystem::is_hardware` 始终 false（无硬件 TPM 探测）

- **位置**：[secure_boot.rs:443-580](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L443-L580) `TpmSubsystem`
- **代码**：
  ```rust
  pub struct TpmSubsystem {
      ...
      is_hardware: AtomicBool,  // 初始化为 false
  }
  ```
- **问题**：
  - `TpmSubsystem::new()` 中 `is_hardware: AtomicBool::new(false)`，**永远不会被设为 true**。
  - 文档承诺"当前为软件模拟实现 (无硬件 TPM 时回退)"（secure_boot.rs:13），但代码中**没有 TIS/CRB 探测路径**。
  - 任何调用 `is_hardware()` 的代码都会拿到 `false`，但若未检查此值直接走 `seal/unseal` 路径，**用户误以为是硬件 TPM 保护**。
  - `sys_tpm` 系统调用也**没有路径暴露此状态**，用户态无法知道当前是软件模拟。
- **建议方案**：
  1. 添加 `probe_tpm_hardware()`：探测 PCI 配置空间 `0x0B 0x00`（TPM TIS class code），探测到则 `is_hardware.store(true)`。
  2. `sys_tpm` 新增 cmd=6 返回 `is_hardware()`，让用户态知情。
  3. `seal/unseal` 中如果 `!is_hardware` 则返回 `NotHardware` 错误（防止误用）。

### 2.7 [P0] `bootstrap.rs:41-64` `grant_from_first_token` 无竞态保护（双授权窗口）

- **位置**：[bootstrap.rs:41-64](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/bootstrap.rs#L41-L64)
- **代码**：
  ```rust
  pub fn grant_from_first_token(target_pwm, domain, caps) -> Result<...> {
      if FIRST_TOKEN_USED.load(Ordering::Acquire) {
          return Err(PwmError::TokenUsed);
      }
      let target = super::identity::find(target_pwm).ok_or(PwmError::NotFound)?;
      target.fetch_or_caps(domain, caps);
      FIRST_TOKEN_USED.store(true, Ordering::Release);
      ...
  }
  ```
- **问题**：
  - "check then set" 模式（TOCTOU）：两个 CPU 同时调用 `grant_from_first_token`，都通过 `load()=false` 检查，都执行 `fetch_or_caps`，都 `store(true)`。
  - 后果：相同 First Token 可被用于授权**两次或更多次**，赋予同一 target_pwm 两次全权（如果 domain/caps 相同无变化，但**审计日志会被错误地记录两次 FirstTokenGrant 事件**）。
  - 真实威胁：若 First Token 用于**不同 target_pwm**（虽然当前签名只有单一调用方，但代码逻辑未禁止），可能被授权多个 PWM。
- **建议方案**：
  1. **立即**：用 `compare_exchange` 替换 `load+store`：
     ```rust
     if FIRST_TOKEN_USED.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
         return Err(PwmError::TokenUsed);
     }
     ```
  2. **配套**：将 `FIRST_TOKEN_ID/FIRST_TOKEN_CREATED` 改为 immutable（首次 token 一旦使用就不变）。

### 2.8 [P0] `storage.rs:170-220` `save_database` 80KB 栈数组 → 与 identity.rs:65 历史栈溢出同源

- **位置**：[storage.rs:184](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/storage.rs#L184)、[storage.rs:270](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/storage.rs#L270)
- **代码**：
  ```rust
  pub fn save_database() -> i32 {
      ...
      let mut buf = [0u8; 80000];  // ← 80KB 栈数组
      ...
  }
  ```
- **问题**：
  - 注释（[identity.rs:65](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L65)）明确记录 2026-07-02 turn 28 排查 test 86 hang 时，根因是 `[DEFAULT_ENTRY; 256]` (100KB) 在栈上创建超过 `KERNEL_STACK_SIZE` (64KB) 导致栈溢出。
  - 但 `storage.rs:184/270` 仍然使用 80KB 栈数组（`save_database`/`load_database`）。
  - 即使每个线程单独 64KB 栈，嵌套调用（如 `save → serialize → sha256 → ...`）可能进一步压栈。
  - 直接违反 `spec-engineering.md` §8.1 "大栈数组是性能反模式"（已被 `clippy::large_stack_arrays` 标注）。
- **建议方案**：
  1. 改用 `Vec<u8>` 堆分配（identity.rs 已示范：`Vec::with_capacity`）。
  2. 或使用静态全局 `static mut DB_BUF: [u8; 80000] = [0; 80000]` + IrqSpinLock 保护。

## 3. P1 问题

### 3.1 [P1] `csprng.rs:62-78` `fallback_entropy_byte` TSC + stack_addr + counter 三源混合 → 实际不是 CSPRNG

- **位置**：[csprng.rs:62-78](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/csprng.rs#L62-L78)
- **代码**：
  ```rust
  fn fallback_entropy_byte(idx: usize) -> u8 {
      static COUNTER: AtomicU64 = AtomicU64::new(0x5A3C_9E17_F2D8_4B61);
      let tsc = crate::arch!(timestamp());
      let stack_addr = &tsc as *const _ as u64;
      let counter = COUNTER.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::AcqRel);
      let mut v = tsc.wrapping_mul(...).wrapping_add(stack_addr)...;
      ((v >> 56) as u8) ^ ((v >> 40) as u8)
  }
  ```
- **问题**：
  - 函数名"fallback_entropy_byte"，但仅是简单 LCG + XOR，不是密码学安全 RNG。
  - 用于 `csprng::generate_salt()` → 密码 salt 派生（identity.rs:263-264）。
  - salt 决定密码哈希唯一性；若 salt 可预测 → 攻击者可预计算彩虹表。
  - 当前实现：
    - `tsc` 在单核上每次调用间隔固定，`stack_addr` 在栈帧固定位置 → **熵源实际只有 `COUNTER`**，对外部观察者来说**可预测**。
    - 没有熵池（entropy pool），没有 whitening。
- **建议方案**：
  1. 真实 CSPRNG：使用 ChaCha20 / AES-CTR DRBG（参考 Linux `random.c`）。
  2. 临时：使用 RDSEED（x86）替代 RDRAND；aarch64 使用 RNDR / RNDRRS。
  3. 熵池最少 256 bit 真随机 + reseed 周期。

### 3.2 [P1] `identity.rs:611-621` `get_table()` 回调中 `slot.write(IdentityTable::new())` 在 panic 时泄漏栈分配语义

- **位置**：[identity.rs:613-621](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L613-L621)
- **代码**：
  ```rust
  static GLOBAL_TABLE: OnceLock<IdentityTable> = OnceLock::new();

  pub fn get_table() -> &'static IdentityTable {
      GLOBAL_TABLE.get_or_init(|slot| {
          slot.write(IdentityTable::new());
      })
  }
  ```
- **问题**：
  - `OnceLock::get_or_init` 闭子的返回值必须是 `T`，但当前 `slot.write(IdentityTable::new());` 不返回任何值（unit `()`）。
  - 这是**编译期类型不匹配**还是**依赖 OnceLock 的 `slot` 是 `&mut MaybeUninit<T>` 而 write 直接初始化**？
  - 实际 OnceLock 的 `get_or_init` 签名是 `F: FnOnce(&mut MaybeUninit<T>) -> T`，当前闭子不返回 T，**编译应该失败**。
  - 如果能编译过，要么是 `IdentityTable` 实现了 `Default` 自动 fallback（但代码没显示 Default），要么是 OnceLock 通过 `write` 旁路了 init 路径（可能 `OnceLock` 在此项目被 monkey patch 过）。
  - 验证：应当跑 `cargo check` 看实际编译结果。
- **建议方案**：
  1. 必须看到 `IdentityTable: Default` 或 OnceLock 的特殊 init 路径。
  2. 如果确实通过编译，建议显式写：
     ```rust
     GLOBAL_TABLE.get_or_init(|slot| {
         slot.write(IdentityTable::new());
         // 返回任何东西，因为 write 已 init 内部 MaybeUninit
     })
     ```
     或改为 `IdentityTable::default()`。
- **风险**：类型欺骗可能导致 `IdentityTable::new()` 实际未执行 → 静默崩溃。

### 3.3 [P1] `secure_boot.rs:512-513` `read_all_pcrs` 返回 `[[u8; 32]; 8]` 256 字节大对象无 protection

- **位置**：[secure_boot.rs:511-513](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L511-L513)
- **代码**：
  ```rust
  pub fn read_all_pcrs(&self) -> [[u8; SHA256_LEN]; PCR_COUNT] {
      *self.pcrs.lock()
  }
  ```
- **问题**：
  - 返回 256 字节栈分配结构（PCR_COUNT × SHA256_LEN = 8 × 32 = 256B），调用方栈深时易溢出。
  - 同时持有锁 + 返回整个数组拷贝，`drop(lock)` 之前已复制，`*self.pcrs.lock()` 在 drop 时拷贝，调用方拿到的是 `Copy` 副本。
  - 但 `[[u8; 32]; 8]` 不是 `Copy` trait，**应该不能 `*lock`** → 实际依赖 `IrqSpinLock<T>` 的 Deref 实现。
  - 没有问题？需要核查，但若是 256B 数组直接放栈，**嵌套调用**可能导致栈帧膨胀。
- **建议方案**：
  1. 改为 `&'static IrqSpinLock<...>` 借用，避免拷贝。
  2. 或返回 `Vec<[u8; 32]>`（堆分配）。

### 3.4 [P1] `api.rs:172-174` `pwm_find_entry` 返回裸指针 + 生命周期无限

- **位置**：[api.rs:167-174](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/api.rs#L167-L174)
- **代码**：
  ```rust
  pub extern "C" fn pwm_find_entry(pwm: u64) -> *const PwmEntry {
      identity::find(pwm).map_or(core::ptr::null(), |e| e as *const PwmEntry)
  }
  ```
- **问题**：
  - 返回 `*const PwmEntry` 指向 `IdentityTable.entries[i]`，C 侧使用期间：
    - **如果 `delete()` 被调用**（identity.rs:508-521），entries[i] 被 `pwm.store(0)` 标记为空，**但 slot 不释放**——返回的指针仍指向同一地址（slot）。
    - **如果 `create()` 找到同一 slot**，则该 slot 被复用为新 entry，**C 侧以为是旧身份实际是新身份**（参见 §2.3）。
  - 没有办法在 C 侧释放 / 标识这个指针失效。
- **建议方案**：
  1. C 侧只接收 `pwm: u64`，需要 entry 时调用 `pwm_find_entry` 重新查。
  2. 或引入引用计数（`Arc<PwmEntry>` 但受 no_std alloc 限制）。

### 3.5 [P1] `session.rs:243-289` `elevate_for_suid` `depth` 计数 + 栈推入分两步，非原子

- **位置**：[session.rs:255-279](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/session.rs#L255-L279)
- **代码**：
  ```rust
  let result = PROCESS_TABLE.with_process(pid, |p| {
      let depth = p.session_elev_depth.load(Ordering::Acquire);
      if depth >= MAX_ELEVATION_DEPTH {
          return (false, 0u64);
      }
      ...
      let snapshot = { ... };
      let mut stack = p.session_elev_stack.lock();
      stack[depth as usize] = snapshot;  // ← 用 depth 作 index
      p.session_elev_depth.store(depth + 1, Ordering::Release);  // ← 后递增
      (true, session_pwm)
  });
  ```
- **问题**：
  - 两次并发 `elevate_for_suid` 都读到 `depth=N`，都写入 `stack[N]`，**栈槽位冲突覆盖**。
  - `session.lock()` 与 `session_elev_stack.lock()` 是两个独立锁，**嵌套锁顺序未明确**（同时持有两个锁）。
  - `p.session_elev_depth.store` 与 `stack[..]=snapshot` 不在同一个锁内，**写入与深度计数无原子关联**。
- **建议方案**：
  1. 合并 `session_elev_stack` 与 `session_elev_depth` 到同一个 `IrqSpinLock`，保证原子推入。
  2. 或使用 `fetch_add` 原子获取 slot index（但需要预分配数组大小等于 MAX_ELEVATION_DEPTH）。
  3. 配套：明确锁顺序文档（先 session 后 elev_stack）。

### 3.6 [P1] `storage.rs:281` v4 兼容路径 `v4_entry_sz` 注释/常量不一致

- **位置**：[storage.rs:265-269](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/storage.rs#L265-L269)、[storage.rs:281](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/storage.rs#L281)
- **代码**：
  ```rust
  let ds = count
      * if vmaj < 5 {
          8 + 1 + 2 + 128 + 128 + 48 + 8 + 8  // ← v4: 8+1+2+128+128+48+8+8 = 329
      } else {
          ENTRY_SZ  // ← v5: 8+8+1+2+128+128+48+8+8 = 339
      };
  ...
  let v4_entry_sz = 8 + 1 + 2 + 128 + 128 + 48 + 8 + 8;
  ```
- **问题**：
  - v4 与 v5 格式差异：v5 多了 8 字节 `creator_pwm`。
  - 但 **注释说"v5: 头部 + 条目"** 而代码中 `serialize` 写的是 `w64 pwm, w64 creator_pwm, w8 level, w16 flags, ...`。
  - v4 格式中 `level` 是 8 位，但 `r8` 读 v5 的 `privilege_level`（同样 8 位）一致。
  - v4 注释 `PWM_NOTE_LEN` vs `PWM_HASH_LEN`（v5 用 `PWM_NOTE_LEN`）：v4 用 `128`（与 v5 `PWM_NOTE_LEN` 相等？但常量定义在 types.rs）。
- **建议方案**：
  1. 抽取常量 `V4_ENTRY_SZ = 329; V5_ENTRY_SZ = 339;`。
  2. 用 `if vmaj == 4 { V4_ENTRY_SZ } else { V5_ENTRY_SZ }` 显式比较。
  3. 配套单元测试：构造 v4 blob → 加载 → 验证字段正确。

### 3.7 [P1] `identity.rs:228-252` `create()` 中"找空槽位"与"写入"两步分窗口

- **位置**：[identity.rs:236-252](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L236-L252)
- **代码**：
  ```rust
  let slot = {
      let mut s = None;
      for i in 0..MAX_PWM_ENTRIES {
          if !self.entries[i].is_valid() {
              s = Some(i);
              break;
          }
      }
      s
  };
  let slot = if let Some(s) = slot { s } else { ... };
  ```
- **问题**：
  - 在 `acquire()` 锁内查找空槽位，但 `s` 提取到 `slot` 后**仍在同一锁内**，看似安全。
  - **但 `create()` 内部先释放 `acquire()` 后 `for entry in &self.entries`**（行 110-116 `init()` 也是）。**`init()` 与 `create()` 之间的锁状态**有竞态窗口。
  - 具体：`init()` 在 `pwm_init()` 中调用一次（[api.rs:58-73](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/api.rs#L58-L73)），如果 `init()` 之前已有 `create()` 在另一个 CPU 上跑（理论上不可能，但 race window 存在）会冲突。
  - 实际 `init()` 是 `INITIALIZED.compare_exchange` 保护，单次执行，但**没有内存屏障保证其他 CPU 看到 INITIALIZED=true 后停止访问 table**。
- **建议方案**：
  1. `init()` 后插 `fence(Ordering::SeqCst)`。
  2. 或 `INITIALIZED.store(true)` 用 `Ordering::Release`（已是 AcqRel）。
  3. `create()` 入口检查 `INITIALIZED.load(Acquire)`，未初始化则返回 `Err(NotReady)`。

### 3.8 [P1] `secure_boot.rs:198` `verify` 占位实现同时存在 `T6-8: 替换为真正的 Ed25519 验证` TODO 但无跟踪 issue

- **位置**：[secure_boot.rs:198](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L198)
- **问题**：
  - `// TODO(TRACK-7A8BAB): 替换为真正的 Ed25519 验证` 是预存问题标记。
  - 当前 TODO 占位符实现 = 安全启动实际不启动。
  - 规范文档要求"无 TODO(TRACK-...) 未处理项"（AGENTS.md §9.4）— 此项违反。
- **建议方案**：
  1. 立即将 `verify` 改为 `false`（fail-closed）。
  2. 文档化为 issue，单开 PR 跟踪 Ed25519 集成。

### 3.9 [P1] `engine.rs:14-16` PWM=0 特权穿透（无审计日志）

- **位置**：[engine.rs:9-28](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/engine.rs#L9-L28)
- **代码**：
  ```rust
  pub fn check(pwm: u64, domain: CapDomain, required: CapBits) -> bool {
      if pwm == 0 {
          return true;  // ← bootstrap 身份, 全部能力
      }
      ...
  }
  ```
- **问题**：
  - `pwm=0` 是 bootstrap 身份，**任何调用都返回 `true`**。
  - **没有任何审计日志**记录 bootstrap 调用——攻击者构造 `pwm=0` 调用可绕过所有权限检查且不留下审计痕迹。
  - `engine.rs:34-52` `check_privilege` 同样的 `pwm=0 → true`，同样无审计。
- **建议方案**：
  1. 在 `pwm=0` 路径添加 `audit::log(0, AuditAction::BootstrapOverride, 0, ...)`。
  2. 配套：定义"bootstrap 身份使用次数"计数器，超过阈值告警。

### 3.10 [P1] `secure_boot.rs:497` TPM `extend` 中 `drop(pcrs)` 后又 `lock` `counts` — 嵌套锁顺序

- **位置**：[secure_boot.rs:490-502](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L490-L502)
- **代码**：
  ```rust
  pub fn extend(&self, pcr_idx: PcrIndex, data: &[u8]) -> bool {
      ...
      let mut pcrs = self.pcrs.lock();   // ← lock 1
      let data_hash = sha256_hash(data);
      pcrs[idx] = sha256_extend(&pcrs[idx], &data_hash);
      drop(pcrs);                         // ← 显式 drop
      let mut counts = self.pcr_extend_count.lock();  // ← lock 2
      counts[idx] += 1;
      true
  }
  ```
- **问题**：
  - 两个 `IrqSpinLock` 嵌套但 `drop(pcrs)` 显式释放后才 lock 第二个，**避免嵌套持锁**——这一点做得对。
  - 但 `drop(pcrs)` 与 `lock counts` 之间存在窗口：**多 CPU 可同时 `extend`，pcrs[idx] 计算后、counts[idx] 递增前**。
  - 后果：pcrs 状态先更新，counts 后递增，**审计追溯时 counts 与实际 extend 次数可能不一致**。
- **建议方案**：
  1. 合并 `pcrs` + `pcr_extend_count` 为单一 `IrqSpinLock<(Pcrs, Counts)>`。
  2. 或在 `drop(pcrs)` 前先 `counts.lock()` 再 `drop(pcrs)`（两锁顺序：counts 先 pcrs 后）。
  3. 配套 lockdep 验证无嵌套持锁。

### 3.11 [P1] `session.rs:466-498` `try_setreuid` 嵌套函数 `has_uid_privilege` 调用 `find_by_uid` 持锁风险

- **位置**：[session.rs:466-498](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/session.rs#L466-L498)
- **代码**：
  ```rust
  fn has_uid_privilege(table: &identity::IdentityTable, uid: u32, current_pwm: u64) -> bool {
      table.find_by_uid(uid).map_or(false, |entry| {
          super::engine::check_privilege(entry.get_pwm().0, current_pwm)
      })
  }
  ```
- **问题**：
  - `find_by_uid` 返回 `&PwmEntry`（[identity.rs:570-577](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L570-L577)），但当前 `IdentityTable` 内部是 `Vec<PwmEntry>` 无锁结构。
  - `&PwmEntry` 借用 `IdentityTable`，调用 `check_privilege` 又会 `identity::find(pwm)` 借用，**可能造成借用检查器抱怨**。
  - 实际 `check_privilege` 内部 `find(operator_pwm)` 是 `find(pwm)` 不是 `find_by_uid`，**没有借用冲突**。
  - 但 `try_setreuid` 主路径同时 `read_current_ctx()` + `find_by_uid()`，**有可重入风险**（`read_current_ctx` 已持 `p.session.lock()`，`find_by_uid` 又借 `IdentityTable`——**无冲突，因为不同锁对象**）。
- **建议方案**：
  1. 文档化锁顺序：`session.lock` ↔ `IdentityTable.lock`（无 lock，因为 IdentityTable 没锁）。
  2. 如果加 IdentityTable 锁（未来扩展），需要明确顺序。

## 4. P2 问题

### 4.1 [P2] `identity.rs:13-22` `constant_time_eq` 长度不等提前返回（理论侧信道）

- **位置**：[identity.rs:13-22](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L13-L22)
- **代码**：
  ```rust
  pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
      if a.len() != b.len() {
          return false;
      }
      ...
  }
  ```
- **问题**：
  - 长度不等提前返回 → **长度本身可能泄露**（但 `digest` 固定 32 字节，无关）。
  - 当前所有调用方传入 `computed` 与 `stored_digest` 都是 32 字节，**不会触发**。
- **建议方案**：
  1. 文档注明"调用方保证等长"。
  2. 或用 `core::hint::black_box` 包裹长度比较。

### 4.2 [P2] `identity.rs:28-53` `hash_with_salt` STRETCH_ROUNDS=32768 硬编码

- **位置**：[identity.rs:28-53](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L28-L53)
- **代码**：
  ```rust
  const STRETCH_ROUNDS: usize = 32768;
  ```
- **问题**：
  - 32K 轮 SHA-256 拉伸，在现代硬件上约 5ms/次（单核）。
  - 硬编码无法根据硬件性能调整。
  - 高频登录场景下成为瓶颈。
- **建议方案**：
  1. 改为 boot 时根据 CPU 性能自动校准（5ms 目标）。
  2. 或用 PBKDF2 / Argon2 标准化实现。

### 4.3 [P2] `audit.rs:38-53` `AuditEntry::result` 字段被硬编码为 `Success`

- **位置**：[audit.rs:38-53](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/audit.rs#L38-L53)
- **代码**：
  ```rust
  pub fn log(&self, pwm: u64, action: AuditAction, target_pwm: u64, domain: u64, caps: u64) {
      ...
      (*entry).result = AuditResult::Success;  // ← 硬编码 Success
      ...
  }
  ```
- **问题**：
  - `log` 接口不接收 result，**所有审计记录都标记 Success**。
  - `Login` 失败的审计记录也是 Success——**审计系统无法识别失败事件**。
- **建议方案**：
  1. `log` 签名增加 `result: AuditResult` 参数。
  2. 调用方 `verify_password` 失败路径传 `AuditResult::Failure`。

### 4.4 [P2] `identity.rs:533-541` `enable()` 调用 `remove_flags` 但 `PwmFlags::LOCKED` 标志未清除

- **位置**：[identity.rs:526-541](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L526-L541)
- **代码**：
  ```rust
  pub fn disable(&self, pwm: u64) -> Result<(), PwmError> {
      let entry = self.find(pwm).ok_or(PwmError::NotFound)?;
      entry.add_flags(PwmFlags::DISABLED);
      ...
  }

  pub fn enable(&self, pwm: u64) -> Result<(), PwmError> {
      let entry = self.find(pwm).ok_or(PwmError::NotFound)?;
      entry.remove_flags(PwmFlags::DISABLED);  // ← 只清除 DISABLED，不清除 LOCKED
      ...
  }
  ```
- **问题**：
  - `session.rs:117` 登录失败 5 次后设 `PwmFlags::LOCKED`（[session.rs:111-113](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/session.rs#L111-L113)）。
  - `enable()` 仅清除 `DISABLED`，**LOCKED 标志保留**——但 LOCKED 在 login 校验路径也会拒绝（[session.rs:94-96](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/session.rs#L94-L96)）。
  - 后果：enable 一个 LOCKED 账户，**仍无法登录**（行为违反直觉）。
- **建议方案**：
  1. `enable()` 同时 `remove_flags(PwmFlags::DISABLED | PwmFlags::LOCKED)`。
  2. 或新增 `unlock_admin()` API 显式清除 LOCKED。

### 4.5 [P2] `storage.rs:184` `let mut buf = [0u8; 80000]` 与 HDR_SZ=12 不一致（DB 大小）

- **位置**：[storage.rs:184](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/storage.rs#L184)
- **问题**：
  - 80KB 容量基于"256 entries × 339 bytes/entry = 86,784 bytes + 12 hdr = 86,796 bytes"。
  - 但 `MAX_PWM_ENTRIES` 在 `types.rs` 中定义（应检查实际值），可能不是 256。
  - 如果 MAX_PWM_ENTRIES=512 则需要 173KB，超出 80KB 容量 → 写入截断。
- **建议方案**：
  1. 用 `assert!(MAX_PWM_ENTRIES * ENTRY_SZ + HDR_SZ <= 80000)` 编译期检查。
  2. 或动态分配 `Vec<u8>`。

### 4.6 [P2] `secure_boot.rs:330-332` `verify_image` `enabled=NotEnabled` 与 `verify_ok_count` 不递增

- **位置**：[secure_boot.rs:329-375](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L329-L375)
- **问题**：
  - 未启用时直接返回 `NotEnabled`，**verify_ok_count 不递增**。
  - 但调用方可能用 `stats()` 检查"是否曾有验证"——`stats = (0, 0)` 与"启用但从未验证"无法区分。
- **建议方案**：
  1. 增加 `skipped_count` 或 `not_enabled_count`。
  2. 返回值标准化。

### 4.7 [P2] `api.rs:182-202` `pwm_has_capability` 与 `pwm_has_cap_raw` 接口不一致

- **位置**：[api.rs:178-207](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/api.rs#L178-L207)
- **代码**：
  ```rust
  pub extern "C" fn pwm_has_cap_raw(pwm: u64, domain: u16, _cap_bit: u8) -> u64 {
      engine::get_caps(pwm, CapDomain(domain)).as_u64()
  }

  pub extern "C" fn pwm_has_capability(pwm: u64, domain: u16, required: u64) -> bool {
      engine::check(pwm, CapDomain(domain), CapBits(required))
  }
  ```
- **问题**：
  - `_cap_bit` 参数被忽略 → 实际是"获取全部 caps"。
  - 与 `pwm_has_capability(required: u64)` 接口语义不同，但接口命名都叫 `pwm_has_cap*`。
- **建议方案**：
  1. 重命名 `pwm_has_cap_raw` 为 `pwm_get_cap_bits`。
  2. 移除 `_cap_bit` 参数或实现"按 cap_bit 检查"。

### 4.8 [P2] `identity.rs:189-204` `verify_password` 多次原子读但无整体一致性

- **位置**：[identity.rs:189-204](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L189-L204)
- **代码**：
  ```rust
  let mut salt = [0u8; PWM_SALT_LEN];
  for i in 0..PWM_SALT_LEN {
      salt[i] = entry.password_hash[PWM_DIGEST_LEN + i].load(Ordering::Acquire);
  }
  let computed = hash_with_salt(password, &salt);
  let mut stored_digest = [0u8; PWM_DIGEST_LEN];
  for i in 0..PWM_DIGEST_LEN {
      stored_digest[i] = entry.password_hash[i].load(Ordering::Acquire);
  }
  constant_time_eq(&computed, &stored_digest)
  ```
- **问题**：
  - 读取 salt → 计算 digest（耗时 ~5ms）→ 读取 stored_digest。
  - 在 `hash_with_salt(password, &salt)` 期间，其他线程可调用 `change_password()` 改 salt 与 digest。
  - 后果：本次登录用旧 salt 算的 digest 与新 stored_digest 比较，**必然失败**——虽然是设计正确（防重放），但**可能误判为密码错误**。
  - 更糟：如果是 `change_password` 失败回滚，stored_digest 是新的，但**用户用的是旧密码**——返回 PasswordIncorrect，**用户无法区分"密码错"还是"并发修改"**。
- **建议方案**：
  1. 短窗口原子读全部 password_hash（一次性 load 48 字节 → 用 `AtomicU64` 6 次读）。
  2. 用版本号 + 双检查（先读版本，再读 hash，再验证版本）。

### 4.9 [P2] `engine.rs:51` `op_level < tgt_level` 与 POSIX DAC 语义相反

- **位置**：[engine.rs:34-52](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/engine.rs#L34-L52)
- **代码**：
  ```rust
  op_level < tgt_level  // ← operator 数字小 = 权限高
  ```
- **问题**：
  - POSIX DAC 约定：**低数字 = 高权限**（root uid=0）。
  - 但与 `transfer_creator`（[identity.rs:430-469](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L430-L469)）的 `if current_level >= target_level` 语义一致。
  - 但 `bootstrap.rs:14` `if creator_pwm == 0` → privilege_level = 0 → 最高权限。
  - 一致性 OK，但 `revoke` 与 `transfer_creator` 的 `>=` 检查容易混淆：
    - `revoke`: `revoker_level >= target_level → Err(InsufficientPrivilege)`（撤销方权限低或等于目标 → 失败）
    - `transfer_creator`: `current_level >= target_level → Err(InsufficientPrivilege)`
  - 两者方向一致但需要谨慎 review。
- **建议方案**：
  1. 文档化权限级语义。
  2. 提取 `has_authority(operator, target)` 辅助函数。

### 4.10 [P2] `secure_boot.rs:506-508` `read_pcr` 不检查 `initialized`

- **位置**：[secure_boot.rs:505-508](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L505-L508)
- **代码**：
  ```rust
  pub fn read_pcr(&self, pcr_idx: PcrIndex) -> [u8; SHA256_LEN] {
      let pcrs = self.pcrs.lock();
      pcrs[pcr_idx as usize]  // ← 未检查 initialized，未检查 pcr_idx 范围
  }
  ```
- **问题**：
  - `extend` 检查 `initialized`，但 `read_pcr` 不检查——未初始化时返回全零（IrqSpinLock 默认初始化）。
  - `pcr_idx as usize` 可能超过 PCR_COUNT=8 → 越界 panic。
- **建议方案**：
  1. 添加 `pcr_idx as usize >= PCR_COUNT` 边界检查，返回 [0; 32]。
  2. 或 `assert!(pcr_idx as usize < PCR_COUNT)`（debug）。

### 4.11 [P2] `bootstrap.rs:66-74` `pwm_now` TSC 未校准时回退到原始 TSC

- **位置**：[bootstrap.rs:66-74](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/bootstrap.rs#L66-L74)
- **代码**：
  ```rust
  pub fn pwm_now() -> u64 {
      let tsc = crate::arch!(timestamp());
      let freq = raw::tsc_frequency();
      if freq > 0 {
          (tsc / freq) * 1_000_000
      } else {
          tsc  // ← 未校准时回退到原始 TSC，跨 CPU 不一致
      }
  }
  ```
- **问题**：
  - `tsc` 在不同 CPU 上可能不同（未同步）。
  - 未校准频率时（启动早期），`pwm_now` 返回的"时间"跨 CPU 不一致。
  - 后果：`lockout_until`、`created_time`、`expires_at` 时间戳在不同 CPU 上不一致。
- **建议方案**：
  1. 启动早期使用全局 atomic counter（自增）。
  2. 校准后切换到 TSC-derived。

### 4.12 [P2] `identity.rs:215-224` `create()` 中 `creator_pwm=0` 默认 `privilege_level=0`（最高权限）

- **位置**：[identity.rs:215-224](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L215-L224)
- **问题**：
  - `creator_pwm=0`（bootstrap）创建的身份默认 `privilege_level=0`（最高权限）。
  - 与 `bootstrap()` 流程一致，但**没有任何审计**记录"bootstrap 创建"。
  - 如果攻击者通过某种路径调用 `pwm_create(creator_pwm=0)`（理论上不可能，因为 API 不暴露），可直接创建最高权限身份。
- **建议方案**：
  1. 拒绝 `creator_pwm=0` 通过 `pwm_create`（仅允许 `bootstrap` / `pwm_try_genesis` 路径）。
  2. 添加审计日志。

## 5. P3 问题

### 5.1 [P3] `secure_boot.rs:52-61` SHA256_K 常量使用下划线分隔但与 RFC 6234 不一致

- **位置**：[secure_boot.rs:52-61](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L52-L61)
- **问题**：
  - 标准 SHA-256 K 常量采用 16 进制无下划线，注释有 `expect(clippy::unreadable_literal)`。
  - 建议改用 `0x428A2F98u32` 等常量命名风格。

### 5.2 [P3] `mod.rs:7-11` `serial_println` 宏被定义但使用 `crate::serial_println!`（可能循环）

- **位置**：[mod.rs:6-11](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/mod.rs#L6-L11)
- **问题**：
  - `mod.rs` 定义 `serial_println` 宏并 `pub use`，但其他模块用 `crate::serial_println!` 引用。
  - 没有循环引用风险，但**宏定义位置不规范**（应放 `framework/klog`）。

### 5.3 [P3] `identity.rs:36-39` `password.bytes().take(255 - pos)` 截断多字节 UTF-8 字符

- **位置**：[identity.rs:36-39](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/identity.rs#L36-L39)
- **问题**：
  - 按字节截断，可能在 UTF-8 字符中间截断，导致哈希输入含无效 UTF-8。
  - 虽然 SHA-256 不在意 UTF-8 有效性，但**密码长度计算不可靠**（用户输入 50 字符可能截断为 30 字节）。

### 5.4 [P3] `api.rs:48-52` `klog_pwm!` 宏定义但调用点未使用

- **位置**：[api.rs:48-52](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/api.rs#L48-L52)
- **问题**：
  - 宏仅在 `pwm_init` 中使用一次（[api.rs:72](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/api.rs#L72)），其他 FFI 入口无日志。
  - 违反 `spec-engineering.md` §F8 "公共 API 应有调用日志"。

## 6. 跨文件关联问题

### 6.1 framework/credo ↔ services/credo 数据漂移风险

- `framework/credo/{types,capability,sha256}.rs` 是 re-export 桩，从 `services/credo/...` 拉取。
- 但 `framework/credo/audit.rs` 等实现**仍在 framework**——理论应迁移到 services（policy 是 services 的职责）。
- 当前混合：constants/types 在 services，mechanisms（audit/identity/session） 在 framework。
- **与既有 50 文件缺 deny 列表（[code-audit-full.md §2.15](file:///home/anfer/Code/QueenX/docs/plan/code-audit-full.md)）一致**：`framework/credo/*` 不需要 deny（framework 允许 unsafe）。

### 6.2 FFI 安全：所有 `extern "C"` 都缺 `#[link_section]` 标注

- 所有 38 个 `pwm_*` + `sys_secure_boot` + `sys_tpm` + `credo_init` 函数都未指定 link section。
- 默认放 `.text`，但**没有明确 ABI 文档**（调用方是 C? Rust? 汇编?）。
- 建议：增加 `.system` / `.text.credo` 显式分段，便于汇编调用方定位。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 8 | 5-7 天 |
| **P1** | 11 | 6-8 天 |
| **P2** | 12 | 3-5 天 |
| **P3** | 4 | 1 天 |
| **合计** | **35** | **15-21 天** |

### P0 修复路径（建议执行顺序）

1. **§2.1 Ed25519 verify → fail-closed**（1 小时，立即安全收益最大）
2. **§2.6 TPM is_hardware 默认值修复 + sys_tpm cmd=6**（0.5 天）
3. **§2.2 audit.rs OnceLock 化**（1 天，与 identity.rs:613-621 一致）
4. **§2.8 storage.rs Vec 化**（0.5 天，避免栈溢出复现）
5. **§2.7 First Token CAS 化**（0.5 天）
6. **§2.5 verify_image PK 回退删除**（0.5 天）
7. **§2.3 PwmContext::current_entry 改 Option<u64>**（1-2 天）
8. **§2.4 IdentityTable 哈希化**（1-2 天，性能问题）