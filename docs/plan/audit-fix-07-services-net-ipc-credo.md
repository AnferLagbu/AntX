# 审计修复分册 07：services 网络、IPC 与凭据

> 修复 services/net（SCM_CREDENTIALS/句柄重用）、services/credo（pwm_set 提权/加密原语）、framework/credo（Ed25519 占位）与 wasm/ipc/barrier 的审计缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.3 节 + 第 7 章 TOP 20 + 附录 H（H.3.1）+ 附录 C（subsystem-services-net / services-wasm-ipc-credo / framework-credo 报告）。

## 工程计划 A: 网络凭据与句柄

### 背景

- **UDS 凭据伪造**
  - 描述：sendmsg 路径硬编码 pid=1/uid=0/gid=0 写入 SCM_CREDENTIALS，任意进程自称 root。
  - 方案：改为真实当前进程凭据。
  - 状态：[]

### 待办

- **SCM_CREDENTIALS 硬编码（TOP 20 #2）**
  - 描述：[net/syscall.rs:407-409](file:///home/anfer/Code/QueenX/src/kernel/services/net/syscall.rs#L407-L409) `let pid: u64 = 1; let uid: u64 = 0; let gid: u64 = 0;` 写死 root 凭据。
  - 方案：从当前进程取真实 pid/uid/gid；补 UDS 凭据 host-tests（发送方身份断言）。
  - 状态：[]

- **socket 句柄 u32::MAX 冲突（TOP 20 #10）**
  - 描述：services/net 句柄 u32::MAX 冲突，use-after-close 风险。
  - 方案：句柄分配自增 + 冲突检测 + 释放表回收。
  - 状态：[]

## 工程计划 B: 凭据与安全启动

### 背景

- **凭据提权 + 签名占位**
  - 描述：pwm_set_syscall 任意提权、Ed25519 验证为占位（任何非零签名通过）、cred 子系统无加密原语。
  - 方案：按提权 → 签名 → 加密原语顺序修复。
  - 状态：[]

### 待办

- **pwm_set_syscall 任意设 root（P0-07）**
  - 描述：[credo/auth.rs:119-121](file:///home/anfer/Code/QueenX/src/kernel/services/credo/auth.rs#L119-L121) `pwm_set_syscall(pwm)` 任何进程可设自身 PWM 为 root，绕过所有 UID/GID 检查。
  - 方案：检查 `credo::pwm_has_capability(pwm_current, CAP_SETUID)`，否则 EPERM。
  - 状态：[]

- **Ed25519 签名验证占位（TOP 20 #5 / ISSUE-SRC-002）**
  - 描述：[framework/credo/secure_boot.rs:197-210](file:///home/anfer/Code/QueenX/src/kernel/framework/credo/secure_boot.rs#L197-L210) `verify` 为占位——签名非全零即通过；含 `TODO(TRACK-7A8BAB)`。
  - 方案：短期 fail-closed（无真实验证时拒绝签名）；长期引入 curve25519 验证库。
  - 状态：[]

- **cred 子系统完全无加密原语（H.3.1 P0-24）**
  - 描述：cred 子系统无加密原语，密码存储/会话完整性无保障。
  - 方案：登记密码哈希（argon2/sha256+盐）原语需求，评估 TCB 影响后实施。
  - 状态：[]

## 工程计划 C: IPC / wasm / barrier

### 背景

- **services 多子目录 P0 引用**
  - 描述：`subsystem-services-wasm-ipc-credo.md` 报告无限循环、shm 无 size、签名占位；`services-misc.md` 报告 barrier 等。详见 archive 报告。
  - 方案：以 archive 报告为准逐项登记实施。
  - 状态：[]

### 待办

- **wasm 解释器无限循环防护**
  - 描述：wasm 解释器缺指令上限/超时，恶意 bytecode 可无限循环。
  - 方案：加指令计数 + 步进上限。
  - 状态：[]

- **shm 无 size 校验**
  - 描述：共享内存创建/映射缺 size 边界校验。
  - 方案：补 size 校验与越界拒绝。
  - 状态：[]

- **services 其余 P0/P1 登记**
  - 描述：barrier/ipc/debug 子系统发现项以 `subsystem-services-misc.md` 为准。
  - 方案：逐条登记到本分册待办并实施。
  - 状态：[]

### 验证门槛

- **网络凭据回归**
  - 描述：SCM_CREDENTIALS 修复后跑 UDS host-tests。
  - 方案：`make test-host`。
  - 状态：[]

- **凭据回归**
  - 描述：pwm_set 修复后跑 credo host-tests（权限拒绝用例）。
  - 方案：`make test-host`。
  - 状态：[]
