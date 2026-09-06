# 审计修复分册 07：services 网络、IPC 与凭据

> 修复 services/net（SCM_CREDENTIALS/句柄重用）、services/credo（pwm_set 提权/加密原语）、framework/credo（Ed25519 占位）与 wasm/ipc/barrier 的审计缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.3 节 + 第 7 章 TOP 20 + 附录 H（H.3.1）+ 附录 C（subsystem-services-net / services-wasm-ipc-credo / framework-credo 报告）。

> **2026-08-31 基线核实**：委托前对全部 22 项逐一对照当前磁盘代码核实（见各条目标注）。结论：**已修复/实装 7 项**（B07-09/10/11/13/14/19/20）、**部分修复 5 项**（B07-03/12/16/17/18）、**仍存在 7 项**（B07-01/02/05/06/07/15 + B07-03 残留 alloc_user_id）、背景 2 项（B07-04/08）+ 验证门槛 2 项（B07-21/22）。**已实装项标注 `[X]`，委托时跳过；仍存在/部分项为待办**。关键决策点：B07-05 项目无 CAP_SETUID 常量（需按先例 SYSTEM+0x01 或裁决）、B07-06 建议 fail-closed（短期）+ 真实验证库（长期）、B07-12 max_memory_pages 死值需接线。

## 工程计划 A: 网络凭据与句柄

### 背景

- **B07-01. UDS 凭据伪造**
  - 描述：sendmsg 路径硬编码 pid=1/uid=0/gid=0 写入 SCM_CREDENTIALS，任意进程自称 root。
  - 方案：改为真实当前进程凭据。
  - 状态：[X] (2026-09-01 修复：unix.rs `current_scm_credentials()` 取真实 pid/uid/gid；recv 侧反序列化末尾 12 字节真实凭据，不再返回占位全零)

### 待办

- **B07-02. SCM_CREDENTIALS 硬编码（TOP 20 #2）**
  - 描述：[net/syscall.rs:407-409](file:///home/anfer/Code/QueenX/src/kernel/services/net/syscall.rs#L407-L409) `let pid: u64 = 1; let uid: u64 = 0; let gid: u64 = 0;` 写死 root 凭据。
  - 方案：从当前进程取真实 pid/uid/gid；补 UDS 凭据 host-tests（发送方身份断言）。
  - 状态：[X] (2026-09-01 修复：sendmsg 取真实凭据；recvmsg 经 `uds_peer_creds(fd)` 反序列化接收缓冲真实凭据，无凭据时不伪造。B07-21 host-tests 已补)

- **B07-03. socket 句柄 u32::MAX 冲突（TOP 20 #10）**
  - 描述：services/net 句柄 u32::MAX 冲突，use-after-close 风险。
  - 方案：句柄分配自增 + 冲突检测 + 释放表回收。
  - 状态：[X] (2026-09-01 修复：`alloc_user_id` 改 `Option<u32>` + 线性探测跳过已占用句柄 + 冲突即返回 `NoFreeSocket`，消除 wrapping 回绕复用；两处调用点已适配)

## 工程计划 B: 凭据与安全启动

### 背景

- **B07-04. 凭据提权 + 签名占位**
  - 描述：pwm_set_syscall 任意提权、Ed25519 验证为占位（任何非零签名通过）、cred 子系统无加密原语。
  - 方案：按提权 → 签名 → 加密原语顺序修复。
  - 状态：[]

### 待办

- **B07-05. pwm_set_syscall 任意设 root（P0-07）**
  - 描述：[credo/auth.rs:119-121](file:///home/anfer/Code/QueenX/src/kernel/services/credo/auth.rs#L119-L121) `pwm_set_syscall(pwm)` 任何进程可设自身 PWM 为 root，绕过所有 UID/GID 检查。
  - 方案：检查 `credo::pwm_has_capability(pwm_current, CAP_SETUID)`，否则 EPERM。
  - 状态：[X] (2026-09-01 修复：新增 `SYSTEM_CAP_SET_PWM = 1 << 1` 专用能力位（DECISION-078），pwm_set 前校验 `pwm_has_capability(current, SYSTEM, SET_PWM)`，无能力返回 `EPERM`。B07-22 host-tests 已补)

- **B07-06. Ed25519 签名验证占位（TOP 20 #5 / ISSUE-SRC-002）**
  - 描述：[framework/credo/secure_boot.rs:197-210](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L197-L210) `verify` 为占位——签名非全零即通过；含 `TODO(TRACK-7A8BAB)`。
  - 方案：短期 fail-closed（无真实验证时拒绝签名）；长期引入 curve25519 验证库。
  - 状态：[X] (2026-09-01 修复：引入 ed25519-dalek 3.x（`default-features=false, features=["fast"]`，no_std 兼容），`verify` 改 `VerifyingKey::verify_strict` 真实验证 RFC 8032，拒绝 malleable 签名。deny.toml 放行 BSD-3-Clause；Cargo.toml 新增 4 依赖)

- **B07-07. cred 子系统加密原语缺口（H.3.1 P0-24）**
  - 描述：实测**密码存储侧非缺口**——`framework/credo/identity.rs:28-53` 已是 SHA-256 加盐 + 32768 轮拉伸 + 常数时间比较（`constant_time_eq`），csprng 生成盐；真实缺口为：① `services/credo/sha256.rs:112` 返回 **48 字节**（PWM_HASH_LEN）但只填充前 32 字节（异常签名）；② `secure_boot.rs` Ed25519 `verify` 占位（见上条）；③ 无 AES/ChaCha/HMAC/KDF 对称原语（中期路线）。
  - 方案：① 修复 sha256 返回 32 字节标准输出或明确文档化前 32 字节语义（低优先）；② Ed25519 真实验证（DECISION-078 已定直接引入验证库）；③ 对称加密原语登记为中期独立任务（评估 TCB 影响后实施）。
  - 状态：[X] (2026-09-01 修复①：`sha256()` 返回类型改 `[u8; PWM_DIGEST_LEN]`(32) 标准输出；`crypto::password_hash` 改 `PasswordHash::from_parts` 显式拼 salt 到 `full[32..48]`（原 salt 恒 0 加盐失效）。② 见 B07-06。③ 对称原语仍为中期任务，登记 [unresolved-issues](../../docs/plan/unresolved-issues-2026-08-09.md))

## 工程计划 C: IPC / wasm / barrier

### 背景

- **B07-08. services 多子目录 P0 引用**
  - 描述：`subsystem-services-wasm-ipc-credo.md` 报告无限循环、shm 无 size、签名占位；`services-misc.md` 报告 barrier 等。详见 archive 报告。
  - 方案：以 archive 报告为准逐项登记实施。
  - 状态：[]

### 待办

- **B07-09. wasm 解释器无限循环防护**
  - 描述：wasm 解释器缺指令上限/超时，恶意 bytecode 可无限循环。
  - 方案：加指令计数 + 步进上限。
  - 状态：[X] (2026-08-31 核实：**已实装**——interpreter.rs:435-438 逐指令 gas 计数 + max_gas 上限，runtime.rs:333 默认 10M gas + max_call_depth 256 防递归)

- **B07-10. shm 无 size 校验**
  - 描述：共享内存创建/映射缺 size 边界校验。
  - 方案：补 size 校验与越界拒绝。
  - 状态：[X] (2026-08-31 核实：**已实装**——[shm.rs:37](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/shm.rs#L37) `size==0 || size>SHM_MAX_SIZE` 拒绝，SHM_MAX_SIZE=16MB)

- **B07-11. credo/auth.rs 密码比较时间侧信道（P0）**
  - 描述：`credo/auth.rs` 密码比较若用 `==` 短路求值 → 攻击者测量响应时间推断前缀 → 爆破（framework `constant_time_eq` 已存在可复用）。
  - 方案：密码验证全程使用常数时间比较。
  - 状态：[X] (2026-08-31 核实：**已实装**——验证逻辑在 framework/credo/identity.rs:13-22 `constant_time_eq` 全字节 XOR 无短路，auth.rs 为薄封装；crypto.rs 提供 ct_eq 系列原语)

- **B07-12. wasm LinearMemory 增长无上限（P1）**
  - 描述：`wasm/runtime.rs` `memory.grow` 可增长到 4GB/16EB，当前允许任意增长 → 内存耗尽。
  - 方案：限制 max memory（如 256MB per instance）。
  - 状态：[X] (2026-09-01 修复：`InterpreterConfig.max_memory_pages`(256) 接入 LinearMemory 创建路径（模块声明 max 取交集、未声明强制 256 页）；`grow` 加 checked_add/checked_mul 防溢出（返回 u32::MAX 失败)）

- **B07-13. wasm call_indirect 类型检查不完整（P1）**
  - 描述：`wasm/interpreter.rs` call_indirect 可能仅校验 `index < table_size` 未校验 type → 类型混淆 → 越权调用。
  - 方案：补全 function index + type index 双重校验。
  - 状态：[X] (2026-08-31 核实：**已实装（fail-closed）**——interpreter.rs:482-484 `CallIndirect` 操作码整体拒绝返回 Unreachable，无间接调用面，比补 type 校验更保守)

- **B07-14. ipc/async_ipc.rs 无背压（P1）**
  - 描述：`ipc/async_ipc.rs` 发送者任意 push，接收者未消费 → 内存爆炸（DoS）。
  - 方案：实现背压/队列容量上限。
  - 状态：[X] (2026-08-31 核实：**已实装**——async_ipc.rs:213-227 队列满返回 Pending，msgq 有界 64 条 + MSG_MAX_SIZE 4096)

- **B07-15. ipc/scheduler_integration.rs 中断上下文唤醒（P1）**
  - 描述：`ipc/scheduler_integration.rs` IPC 唤醒等待进程调用 `wake_up`，中断上下文调用可能死锁。
  - 方案：检查 `in_interrupt_context()` → 延后到 softirq。
  - 状态：[X] (2026-09-01 修复（完整接线）：① WaitQueue 改 IrqSpinLock 保护 + try_lock/pending 中断安全；② wake_one/wake_all 中断上下文登记 pending 不调度；③ block_current_thread 真实 `scheduler_block` 阻塞 + 中断上下文守卫；④ block_with_timeout 用 hrtimer 睡眠替代忙等待；⑤ sem_wait 真实阻塞 + sem_post/pipe/msgq wake 经调度器 `scheduler_unblock` 唤醒；⑥ TRACK-21BAF1/8C5FFB 消除)
  - 审查发现（2026-09-02 复审）：
    - **复审缺陷①（tid/pid 错位）**：`block_current_thread` 用 `thread_get_current()`（线程 tid，独立 id 空间）入队，而 `scheduler_unblock` 按 **pid** 查找进程表 → 唤醒错位（丢失→死锁 或 唤醒错误进程）。epoll.rs:317 既有范式是 `tid: current_pid`（字段名 tid 实存 pid）。
    - **复审缺陷②（wake_all 不调度）**：`wake_all_threads` 仅 `wait_queue.wake_all()` 清空队列、从不 `scheduler_unblock` → pipe_close/sem_destroy 唤醒的进程永不被唤醒（死锁）。
    - **复审缺陷③（中断上下文唤醒被丢弃）**：IRQ 路径 `drain_pending()` 仅清标志不做实际唤醒（文档声称"补唤醒"但实现无动作）→ 中断上下文唯一唤醒丢失。
    - **复审缺陷④（队列满死锁）**：WaitQueue 4 槽 + `add` 静默丢弃；改真实阻塞后第 5 个等待者 `scheduler_block` 但不在任何队列 → 永久睡眠。
    - **修复（2026-09-02，方案 B）**：① 入队/唤醒统一以 pid 为准（`process_get_current_pid`，字段 `tid`→`pid` 重命名，同步 epoll.rs 两处）；② `wake_all_threads` 改 `while let Some(item) = wake_one() { scheduler_unblock(pid) }`；③ IRQ 上下文仅 `request_wake()` 登记，进程上下文 `drain_pending()` 真实补唤醒（返回全部待唤醒项）；④ `add` 返回 `bool`，队列满时 `block_current_thread` 返回 `Err(-1)` 由调用方回退忙等；补 WaitQueue 行为单测 4 例（types.rs `#[cfg(test)]`）。

- **B07-16. credo policy/grants/sessions/crypto 加固（P1）**
  - 描述：`credo/policy.rs` CapabilityMatrix 16×64 容量不足；`credo/grants.rs` 委托链无最大深度限制（权限放大）；`credo/sessions.rs` MAX_SESSIONS 硬编码过小；`credo/crypto.rs` 自实现加密易错。
  - 方案：capability 扩容或 HashMap；委托加最大深度；session 扩容或动态；crypto 集成经审计库（ring/RustCrypto）。
  - 状态：[X] (2026-09-01 修复（分层修复，用户裁决）：① 常量冲突消除——`GRANT_TABLE_CAPACITY`(256) 与 `CREDO_MAX_SESSIONS`(64) 改名，明确与 `types::MAX_GRANT_RECORDS`(1024)/`config::MAX_SESSIONS`(16) 的职责区分（授权表 vs 委托引擎、认证会话 vs POSIX 进程会话，非重复实现）；② 委托链级联撤销实装——`delegate` 支持 `parent_gen` 父链 + `mark_revoked` 递归收集后代级联撤销 + 悬空父链拒绝；③ capability 扩容与 crypto 审计库登记为中期任务（磁盘格式迁移 + TCB 评估）)

- **B07-17. barrier/attribution.rs 自动降级滥用（P0）**
  - 描述：`barrier/attribution.rs:24-28` 服务域连续失败自动降级 capability → 攻击者可故意触发服务失败强制降级关键服务 → 绕过 capability 检查。
  - 方案：降级需多因子决策（连续失败次数 + 时间窗口 + 失败模式）；仅降级非关键 capability；单开 PR 深审。
  - 状态：[X] (2026-09-01 修复：① `record_failure` 升级多因子——连续失败次数 + heartbeat_gap(>500 直接升级) + dependents(有依赖且≥3 次升级)，与 recovery_policy 阈值一致；② `handle` 降级写入落地——`downgrade_for_tier` 结果实际写回 capability matrix（原 `let _ = target` no-op，攻击者触发降级收不回能力）；③ health_monitor report_failure 传真实 heartbeat_gap)

- **B07-18. debug/ebpf_verifier.rs 规则不足（P0）**
  - 描述：`debug/ebpf_verifier.rs:14-23` 验证仅 7 条规则（指令数/寄存器/跳转/回边/EXIT/R1-R5/R10），**缺** ALU 溢出、栈越界、helper 参数类型检查 → 恶意 eBPF 被放行。
  - 方案：添加 ALU 范围检查、栈访问深度验证；配套 fuzzing 测试。
  - 状态：[X] (2026-09-01 修复：① 栈偏移深度校验——LDX 从 StackPtr 读取校验 `off ∈ [-512, 0]`（BPF_STACK_SIZE）；② helper 参数校验——按签名要求 R1..Rn 已初始化（MAP_UPDATE 需 R1-R3、LOOKUP/DELETE 需 R1-R2），缺参拒绝；③ ALU64 溢出检测——单点常量 ADD/SUB/MUL 用 checked 运算，溢出拒绝（RegState 增 range 字段）；④ 新增 5 个回归单测。fuzzing 仍为待办（登记）)

- **B07-19. debug/ebpf.rs 验证逻辑分散（P1）**
  - 描述：`debug/ebpf.rs:1-33` 入口仅 33 行与 754 行验证器 + 1402 行 framework 实现不对称，验证逻辑分散在 services 与 framework 两处。
  - 方案：统一验证逻辑归属，明确 services（策略）/framework（机制）边界。
  - 状态：[X] (2026-08-31 核实：**已实装**——services/debug/ebpf.rs 33 行薄代理，验证策略统一收敛到 ebpf_verifier.rs StandardBpfVerifier（0 unsafe）实现 framework BpfVerifier trait，framework 仅提供 set_verifier 机制，边界清晰)

- **B07-20. barrier/health_monitor.rs 阈值未审（P1）**
  - 描述：`barrier/health_monitor.rs:1-266` 健康监控阈值未深审。
  - 方案：单开 PR 深审。
  - 状态：[X] (2026-08-31 核实：**已深审/多因子化**——阈值决策形式化为 recovery_policy.rs:145-195 决策矩阵（fault_kind×retry_count×heartbeat_gap×dependents，含 8 单测），health_monitor tick 走 RecoveryPolicy::decide；注：dependents 调用处硬编码 0 未接通)

### 验证门槛

- **B07-21. 网络凭据回归**
  - 描述：SCM_CREDENTIALS 修复后跑 UDS host-tests。
  - 方案：`make test-host`。
  - 状态：[X] (2026-09-01 完成：新增 `b07_creds_audit_test.rs`（UDS 真实凭据/反序列化/无硬编码/句柄无回绕 4 项断言）；host-tests 全量 910+5 passed/0 failed)

- **B07-22. 凭据回归**
  - 描述：pwm_set 修复后跑 credo host-tests（权限拒绝用例）。
  - 方案：`make test-host`。
  - 状态：[X] (2026-09-01 完成：`b07_creds_audit_test.rs` 含 pwm_set 能力位校验 + SYSTEM_CAP_SET_PWM 定义断言；现有 credo 单测覆盖常数时间/会话限额)

## DECISION-078（2026-08-31，分册 7 委托前用户裁决）

分册 7 委托前的 3 个决策点，用户裁决如下（对应 AskUserQuestion 2026-08-31）：

- **B07-05 pwm_set_syscall 权限校验 — 新增专用能力位**
  - 裁决：按"常量应用面重要性"决策准则——检查常量应用面与既有先例后，pwm_set 属**身份安全关键操作**（任意提权漏洞 P0-07），重要性高，**新增专用能力位**而非复用 SYSTEM+CAP_SYS_ADMIN(0x01)。
  - 准则（用户定义，后续常量决策沿用）：**重要或特殊用途常量采用新增，其他的采用复用既有**。既有先例对照：mount/umount2/setns/open_by_handle 均复用 SYSTEM 域 0x01（这些操作已有 CAP_SYS_ADMIN 语义可复用），而 pwm_set 改变进程身份无既有能力位可精确表达 → 新增。
  - 实施待定：新增能力位归属域（SYSTEM 域 or USER_MGMT 域）+ 位号由委托人按 capability.rs 布局设计。
  - 后续：2026-08-31 对既有复用先例回溯治理，见独立计划 [credo-capability-constants-plan.md](./credo-capability-constants-plan.md)（B1 reboot/B2 open_by_handle 新增专用位、B3 sethostname 修魔法数、B4 mmap 补 MEM_CAP 命名、B5 sys_boot_install 待澄清；A 类 mount/setns/ramfs-hvfs 保持复用）。
- **B07-06 Ed25519 签名验证 — 直接引入验证库**
  - 裁决：跳过 fail-closed 中间态，直接引入 curve25519 验证库实现真实验证。
  - 前置评估（委托人开工前必须完成）：① no_std 兼容性（内核裸机 target 需 `#![no_std]` 可用）；② TCB 占比影响（新依赖计入 TCB，需 <30% 软目标内）；③ 许可证（需过 cargo-deny，与 MIT 内核兼容，deny.toml 已配置）；④ 候选：curve25519-dalek（ed25519-dalek）或其他 no_std 兼容实现，由委托人调研后定。
- **B07-12 wasm memory.grow — 接线 256 页上限**
  - 裁决：把已存在的 `InterpreterConfig.max_memory_pages: 256` 接入 LinearMemory 创建路径，模块不声明 max 时强制 256MB 上限。
  - 实施要点：interpreter.rs:83/89 的 LinearMemory 创建改用 config.max_memory_pages 作为无声明时的默认 max_pages。

## 变更历史

- **2026-09-01（分册 7 全项实施）**
  - 描述：完成分册 7 全部待办项（13 项已修复/落地），通过五条验证门槛。
  - 方案：
    - **B07-01/02 UDS 凭据**：unix.rs `current_scm_credentials()` 真实凭据 + recv 反序列化；syscall.rs sendmsg 真实凭据 + recvmsg 经 `uds_peer_creds` 回传。
    - **B07-03 句柄**：`alloc_user_id` 改 `Option<u32>` + 冲突检测，消除 wrapping 回绕。
    - **B07-05 能力位**：新增 `SYSTEM_CAP_SET_PWM = 1 << 1`（DECISION-078），pwm_set 前校验，无能力返回 EPERM。
    - **B07-06 Ed25519**：引入 ed25519-dalek 3.x，`verify_strict` 真实验证；deny.toml 放行 BSD-3-Clause。
    - **B07-07 sha256**：`sha256()` 改 32 字节标准输出；`password_hash` 经 `from_parts` 显式拼 salt。
    - **B07-12 wasm**：`max_memory_pages`(256) 接线 + grow checked 溢出防护。
    - **B07-15 IPC**：WaitQueue 中断安全（IrqSpinLock+try_lock+pending）、真实阻塞（scheduler_block/unblock）、hrtimer 超时、sem/pipe/msgq wake 全接线。
    - **B07-16 credo**：常量冲突消除（GRANT_TABLE_CAPACITY/CREDO_MAX_SESSIONS）+ 委托链级联撤销；capability 扩容/crypto 审计库登记中期。
    - **B07-17 attribution**：record_failure 多因子（次数+heartbeat+dependents）+ 降级写入落地。
    - **B07-18 ebpf**：栈偏移深度校验 + helper 参数签名校验 + ALU64 溢出检测 + 5 回归单测。
    - **B07-21/22**：新增 `host-tests/tests/b07_creds_audit_test.rs`（5 用例）。
  - 验证：双架构 cargo check 0w0e + clippy `-D pedantic` 双架构 0 warning + 核心审计全过（boundary/safety/deadlock/coupling/invariants/comment_language）+ host-tests 910+5 passed/0 failed + QEMU x86_64 完整启动进入 Ring 3（1/1）。
  - 状态：[X]

- **2026-09-02（复审修复 + 预存编译错误修复）**
  - 描述：分册 7 复审（委托提交 62335c40）发现 B07-15 IPC 阻塞/唤醒 4 缺陷（方案 B 修复）；修复 `kernel_test` 模式预存编译错误，`test-unit` 首次可构建运行。
  - 方案：
    - **B07-15 复审修复**：① 入队/唤醒统一以 pid 为准（`WaitQueueItem.tid`→`pid` 重命名，`process_get_current_pid` 入队，同步 epoll.rs 两处）；② `wake_all_threads` 改逐个 `wake_one`+`scheduler_unblock`；③ 中断上下文仅 `request_wake()` 登记、进程上下文 `drain_pending()` 真实补唤醒；④ `WaitQueue::add` 返回 `bool`，队列满 `block_current_thread` 返回 `Err(-1)` 回退忙等；补 4 例 WaitQueue 行为单测（`#[cfg(test)]`）。
    - **预存编译错误（kernel_test）**：`framework/tests/sys.rs` 按旧 API 编写（`E_` 前缀变体 + `as_i64`/`from_i64` + 48 字节 sha256），修复为现 API（`EPERM`/`as_ret`/模块级 `errno_from_i64`/`[u8; 32]`），消除 12 处编译错误。
  - 验证：双架构 cargo check 0w0e + clippy 0 warning + 核心审计全过 + host-tests 915/0 + QEMU x86_64 1/1 + **`make test-unit` 全量 256 passed / 0 FAILED**（含预存 `PI_MUTEX::basic_lock_unlock` 修复，见下）。
  - 状态：[X]

  > **预存问题（2026-09-02 裁决修复）**：`make test-unit` 首次运行暴露 `PI_MUTEX::basic_lock_unlock` 失败（`should be unlocked after drop`）。根因：`pi_mutex.rs::unlock_internal` 用 `current_pid()` 校验 holder（合法生产安全逻辑，防非持有者释放），而测试硬编码 holder=1 与 kernel_test 环境 `current_pid()` 不符，`drop(g)` 提前 return、锁未释放。处置（用户裁决：修测试，环境 pid + RAII 路径）：`test_basic_lock_unlock` 改用 `sync::raw::current_pid()` 持锁，使 `PiMutexGuard::drop → unlock_internal` 的 holder 校验通过，真实覆盖 RAII 释放路径（不再依赖 `force_unlock`）。其余走 `force_unlock` 的用例保持 v2.1 既有设计。附带预存问题（用户裁决：规划方案并修复）：`make test-unit`（kernel_test 构建）暴露 e1000.rs `unused_imports` warning（`E1000Io`/`E1000_ICR_*`/`E1000_RDT` 仅被 `#[cfg(not(feature="kernel_test"))]` 门控的 probe/handle_interrupt 使用），已拆分为两条 use（`E1000Driver` 常驻 + 其余符号整组门控），kernel_test 构建 0 warning。修复后 `make test-unit` 全量 256 passed / 0 FAILED，`./ci/build.sh all` + `./ci/audit.sh quick` + 核心审计全过。
