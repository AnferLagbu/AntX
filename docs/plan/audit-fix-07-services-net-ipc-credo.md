# 审计修复分册 07：services 网络、IPC 与凭据

> 修复 services/net（SCM_CREDENTIALS/句柄重用）、services/credo（pwm_set 提权/加密原语）、framework/credo（Ed25519 占位）与 wasm/ipc/barrier 的审计缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.3 节 + 第 7 章 TOP 20 + 附录 H（H.3.1）+ 附录 C（subsystem-services-net / services-wasm-ipc-credo / framework-credo 报告）。

> **2026-08-31 基线核实**：委托前对全部 22 项逐一对照当前磁盘代码核实（见各条目标注）。结论：**已修复/实装 7 项**（B07-09/10/11/13/14/19/20）、**部分修复 5 项**（B07-03/12/16/17/18）、**仍存在 7 项**（B07-01/02/05/06/07/15 + B07-03 残留 alloc_user_id）、背景 2 项（B07-04/08）+ 验证门槛 2 项（B07-21/22）。**已实装项标注 `[X]`，委托时跳过；仍存在/部分项为待办**。关键决策点：B07-05 项目无 CAP_SETUID 常量（需按先例 SYSTEM+0x01 或裁决）、B07-06 建议 fail-closed（短期）+ 真实验证库（长期）、B07-12 max_memory_pages 死值需接线。

## 工程计划 A: 网络凭据与句柄

### 背景

- **B07-01. UDS 凭据伪造**
  - 描述：sendmsg 路径硬编码 pid=1/uid=0/gid=0 写入 SCM_CREDENTIALS，任意进程自称 root。
  - 方案：改为真实当前进程凭据。
  - 状态：[] (2026-08-31 核实：**仍存在**——[unix.rs:518/646](file:///home/anfer/Code/QueenX/src/kernel/services/net/unix.rs#L518) 两处 `ScmCredentials { pid: 1, uid: 0, gid: 0 }`；真实凭据 API 存在（credo/identity.rs current_uid/current_gid、sync/mod.rs current_pid）但未接入)

### 待办

- **B07-02. SCM_CREDENTIALS 硬编码（TOP 20 #2）**
  - 描述：[net/syscall.rs:407-409](file:///home/anfer/Code/QueenX/src/kernel/services/net/syscall.rs#L407-L409) `let pid: u64 = 1; let uid: u64 = 0; let gid: u64 = 0;` 写死 root 凭据。
  - 方案：从当前进程取真实 pid/uid/gid；补 UDS 凭据 host-tests（发送方身份断言）。
  - 状态：[] (2026-08-31 核实：**仍存在**——sendmsg L407-409 硬编码 + recvmsg L490 硬编码 pid=1/gid=0)

- **B07-03. socket 句柄 u32::MAX 冲突（TOP 20 #10）**
  - 描述：services/net 句柄 u32::MAX 冲突，use-after-close 风险。
  - 方案：句柄分配自增 + 冲突检测 + 释放表回收。
  - 状态：[] (2026-08-31 核实：**部分**——FD 编号层已用位图分配器修复（[fd_alloc.rs](file:///home/anfer/Code/QueenX/src/kernel/services/proc/fd_alloc.rs) V2 位图），socket_open 已加自增+冲突检测+槽位回收（[smoltcp_impl.rs:673-681](file:///home/anfer/Code/QueenX/src/kernel/services/net/smoltcp_impl.rs#L673)）；但审计点名的 [alloc_user_id](file:///home/anfer/Code/QueenX/src/kernel/services/net/smoltcp_impl.rs#L202-L210) 回绕缺陷原样保留（wrapping_add 无冲突校验，u32 回绕后仍可撞旧句柄）——**残留待修**)

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
  - 状态：[] (2026-08-31 核实：**仍存在**——auth.rs:119-121 无校验直调 proc_set_pwm，framework/proc_ops.rs:902 亦无检查。⚠ 决策点：项目**无 CAP_SETUID 常量**，需按 B06 先例用 `pwm_has_capability(pwm, 0, 0x01)`（SYSTEM+CAP_SYS_ADMIN）或裁决新增能力位)

- **B07-06. Ed25519 签名验证占位（TOP 20 #5 / ISSUE-SRC-002）**
  - 描述：[framework/credo/secure_boot.rs:197-210](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L197-L210) `verify` 为占位——签名非全零即通过；含 `TODO(TRACK-7A8BAB)`。
  - 方案：短期 fail-closed（无真实验证时拒绝签名）；长期引入 curve25519 验证库。
  - 状态：[] (2026-08-31 核实：**仍存在，且未 fail-closed**——secure_boot.rs:198 TODO 在，L201-209 签名非零即通过；信任链 enroll/verify_image (L317/352/359/367) 全链依赖该占位。**建议按方案：短期 fail-closed 优先**)

- **B07-07. cred 子系统加密原语缺口（H.3.1 P0-24）**
  - 描述：实测**密码存储侧非缺口**——`framework/credo/identity.rs:28-53` 已是 SHA-256 加盐 + 32768 轮拉伸 + 常数时间比较（`constant_time_eq`），csprng 生成盐；真实缺口为：① `services/credo/sha256.rs:112` 返回 **48 字节**（PWM_HASH_LEN）但只填充前 32 字节（异常签名）；② `secure_boot.rs` Ed25519 `verify` 占位（见上条）；③ 无 AES/ChaCha/HMAC/KDF 对称原语（中期路线）。
  - 方案：① 修复 sha256 返回 32 字节标准输出或明确文档化前 32 字节语义（低优先）；② Ed25519 真实验证（上条 fail-closed→库）；③ 对称加密原语登记为中期独立任务（评估 TCB 影响后实施）。
  - 状态：[] (2026-08-31 核实：**仍存在**——① sha256.rs:113 返回 [u8;48] 仅填前 32，且 crypto.rs:180-188 salt 未写入 full 后半段（表示语义异常）；② Ed25519 占位见 B07-06；③ 全 src/kernel 无 AES/ChaCha/HMAC/KDF，且未登记中期任务)

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
  - 状态：[] (2026-08-31 核实：**部分**——runtime.rs:165-176 grow 有 max_pages 上限检查，但 LinearMemory 创建用模块声明的 `mem_type.limits.max`（不声明 max 时 = None 无界）；`InterpreterConfig.max_memory_pages: 256` 是**死值未接线**（全仓仅定义+Default 2 处命中）。**残留：默认 256 页配置接入内存创建**)

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
  - 状态：[] (2026-08-31 核实：**仍存在**——scheduler_integration.rs:68-81 wake_one/wake_all 直接调 wait_queue.wake，**无 in_interrupt_context 检测**（全内核无此 API，需新增）+ 无 softirq 延后；block_with_timeout 仍含 TODO(TRACK-8C5FFB))

- **B07-16. credo policy/grants/sessions/crypto 加固（P1）**
  - 描述：`credo/policy.rs` CapabilityMatrix 16×64 容量不足；`credo/grants.rs` 委托链无最大深度限制（权限放大）；`credo/sessions.rs` MAX_SESSIONS 硬编码过小；`credo/crypto.rs` 自实现加密易错。
  - 方案：capability 扩容或 HashMap；委托加最大深度；session 扩容或动态；crypto 集成经审计库（ring/RustCrypto）。
  - 状态：[] (2026-08-31 核实：**部分**——常数时间原语/加盐拉伸/每 PWM 会话限额(≤8)已加；但 capability 仍 16×64 未扩容、**委托链无最大深度限制**（grants.rs parent_gen 恒 0）、MAX_SESSIONS 仍 64 未动态化、crypto 仍自实现 sha256 未集成审计库)

- **B07-17. barrier/attribution.rs 自动降级滥用（P0）**
  - 描述：`barrier/attribution.rs:24-28` 服务域连续失败自动降级 capability → 攻击者可故意触发服务失败强制降级关键服务 → 绕过 capability 检查。
  - 方案：降级需多因子决策（连续失败次数 + 时间窗口 + 失败模式）；仅降级非关键 capability；单开 PR 深审。
  - 状态：[] (2026-08-31 核实：**部分**——attribution.rs:241-256 record_failure 仍仅凭连续失败次数定级（last_failure_tick 记录了但不参与决策），且 handle 的降级写入实际是 no-op（`let _ = target`）；多因子决策已在上层 health_monitor/recovery_policy 实现（retry_count×heartbeat_gap×dependents）——**残留：attribution 内部单因子定级 + 降级写入落地**)

- **B07-18. debug/ebpf_verifier.rs 规则不足（P0）**
  - 描述：`debug/ebpf_verifier.rs:14-23` 验证仅 7 条规则（指令数/寄存器/跳转/回边/EXIT/R1-R5/R10），**缺** ALU 溢出、栈越界、helper 参数类型检查 → 恶意 eBPF 被放行。
  - 方案：添加 ALU 范围检查、栈访问深度验证；配套 fuzzing 测试。
  - 状态：[] (2026-08-31 核实：**部分**——规则已扩充至 ~11 条（含 helper 白名单/LD ctx 偏移/LDX 指针源/ST 指针类型/ALU 未初始化，单测 ~20）；但**仍缺** ALU 溢出范围检查、栈偏移深度验证（[R10+off] 未校验 BPF_STACK_SIZE）、helper 参数类型检查（R2-R5 未验）、fuzzing 测试——**残留待修**)

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
  - 状态：[] (2026-08-31 核实：委托前基线已确认——host-tests 全量 910/0（分册 6 复验实测）；B07-01/02 修复后需补 UDS 凭据用例并复跑)

- **B07-22. 凭据回归**
  - 描述：pwm_set 修复后跑 credo host-tests（权限拒绝用例）。
  - 方案：`make test-host`。
  - 状态：[] (2026-08-31 核实：B07-05 修复后需补 pwm_set 无能力拒绝用例；现有 credo 单测已覆盖常数时间/会话限额)
