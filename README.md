# QueenX

QueenX 是一个从零构建的操作系统内核 (Rust + 少量 NASM 汇编, 支持 x86_64 与 aarch64 双架构, 采用 Asterinas 框内核 Framekernel 架构). 未来基于 QueenX 内核的完整操作系统/发行版代号为 AntX.

不包含 Linux 或其他现有内核的代码, 全部从零实现.

## 仓库

- 许可证: MIT (见 [LICENSE](file:///home/anfer/Code/QueenX/LICENSE))
- 协作规范: [AGENTS.md](file:///home/anfer/Code/QueenX/AGENTS.md) (项目硬约束 + AI 行为准则)
- 变更记录: [CHANGELOG.md](file:///home/anfer/Code/QueenX/CHANGELOG.md)


## 克隆与远程仓库

项目使用 Gitee 作为唯一远程仓库:

```bash
git clone git@gitee.com:AnferLagbu/QueenX.git
cd QueenX
git remote rename origin Gitee
git push Gitee main
```

## 架构

QueenX 采用 Asterinas 框内核 (Framekernel) 范式：单一地址空间，源码层划分为两个子树。

### framework 子树

`src/kernel/framework/` 是唯一允许出现 `unsafe` Rust 的模块，统称 TCB (Trusted Computing Base)。封装 MMU、DMA、中断、上下文切换、同步原语、设备寄存器等硬件交互，对外暴露安全的 Rust API。

### services 子树

`src/kernel/services/` 强制 `#![deny(unsafe_code)]`，100% safe Rust。实现系统调用分发、进程策略、文件系统业务、设备驱动集成、网络协议栈调用等机制之上的策略层。

### 边界强制

- 编译期 `services/` 顶层 `#![deny(unsafe_code)]` 拦截任何 unsafe 块/函数/impl/trait
- 静态扫描 `scripts/audit_services_boundary.py` 拦截 services 对 framework 内部模块的越界访问
- 静态扫描 `scripts/audit_safety_coverage.py` 校验 framework 每个 `unsafe` 块配 `// SAFETY:` 注释
- 静态扫描 `scripts/audit_deadlock_matrix.py` 标记锁顺序与中断上下文风险

完整规范见 [docs/explain/explain-framekernel.md](file:///home/anfer/Code/QueenX/docs/explain/explain-framekernel.md) 与 [docs/explain/guide-dev.md](file:///home/anfer/Code/QueenX/docs/explain/guide-dev.md)。

## 子系统

| 子系统 | framework 路径 | services 路径 | 关键内容 |
|--------|---------------|----------------|----------|
| 内存管理 | `framework/mm/` | — | Buddy PMM、VMM、Slab、Kmalloc、COW、VMA、Page Cache、Swap |
| 进程管理 | `framework/proc/` | `services/proc/` | CFS + 实时 + 批处理调度、进程表、ELF 加载、信号、CPU 亲和性 |
| 中断与异常 | `framework/arch/` `framework/idt/` | — | x86_64 IDT/GDT/APIC/IOAPIC；aarch64 GIC v3/PSCI/Generic Timer |
| 同步原语 | `framework/sync/` | `services/sync/` | Spinlock、Mutex、RwLock、SeqLock、RCU、OnceLock、IrqSpinLock |
| 文件系统 | `framework/fs/` | `services/fs/` | VFS、Inode trait (7 FS 原生实现)、HvFS v2、ramfs、devfs、procfs、ext2、exfat、overlayfs、tmpfs |
| 设备驱动 | `framework/driver/` | `services/driver/` | NVMe、AHCI、ATA、E1000、VirtIO-BLK/Net、xHCI、显示、键盘、PL011 |
| 网络 | `framework/net/` | `services/net/` | smoltcp 0.13.0 (vendored)、E1000、VirtIO-Net、Chitin NetOps |
| 系统调用 | `framework/syscall/` | `services/syscall/` | 用户态入口、业务分发、Futex、Epoll |
| IPC | `framework/ipc/` | `services/ipc/` | Pipe、SHM、MessageQueue、Semaphore、Signal |
| 身份与权限 | `framework/credo/` | `services/credo/` | Credo 能力系统 (前称 PWM/PWID v5) |
| 故障恢复 | `framework/barrier/` | `services/barrier/` | Barrier Stack、UndoLog、RecoveryDomain、BSR/BHR/BBR 三层恢复 |
| 配置中心 | `framework/config/` | — | 集中配置、启动镜像编码、KASLR |
| WASM | `framework/wasm/` | `services/wasm/` | 内核态 WebAssembly 解释器 (原型) |

## 架构状态

| 架构 | 目标三元组 | Makefile 参数 | 状态 |
|------|-----------|--------------|------|
| x86_64 | `x86_64-unknown-none` | `ARCH=x86_64` (默认) | 主架构 |
| aarch64 | `aarch64-unknown-none` | `ARCH=aarch64` | QEMU 验证中 |

详细路线图与各 Phase 进度见 [docs/plan/kernel-roadmap.md](file:///home/anfer/Code/QueenX/docs/plan/kernel-roadmap.md)。

## 构建

### 主目标

```bash
make                # x86_64 默认
make ARCH=aarch64   # aarch64
make user           # 5 个用户态 Rust 程序 (init/eash/install/fbterm/httpsrv)
make run            # QEMU 启动
make run-headless   # QEMU 无头模式, 串口写入 build/log/serial.log
make debug          # QEMU + gdb remote :1234
make iso            # GRUB2 启动 ISO (x86_64 only)
make clean
```

构建产物位于 `build/`，包括 `kernel.flat` (QEMU 直接加载的 raw 镜像) 与 `kernel.bin` (含符号)。

跨架构切换时，Makefile 通过根目录的 `.build-arch` 文件记录上次构建架构，不匹配时自动 `cargo clean` 并删除 `boot.o` 等架构相关产物，防止误用。

### 用户态程序

5 个用户态 Rust 程序位于 `src/user/`，通过共享工作空间编译，链接脚本为 `link.x` (x86_64) 与 `link_aarch64.x` (aarch64)。共享 lib (`src/user/lib/`) 提供基础 `print!`/`println!` 与串口 I/O。

## 测试与审计

`AGENTS.md` 规定硬约束：双架构编译 0 warning 0 error，所有审计与测试通过。

### CI 目标 (`Makefile.ci`)

```bash
make -f Makefile.ci ci             # 全量 CI 流程
make -f Makefile.ci ci-cargo       # cargo check x86_64 + aarch64
make -f Makefile.ci ci-audit       # 三个审计脚本
make -f Makefile.ci ci-unsafe-scan # services 0 unsafe 扫描
make -f Makefile.ci ci-test-host   # host-tests 单元 + 集成
make -f Makefile.ci ci-bench       # 性能基线回归 (15% 阈值)
make -f Makefile.ci ci-fix         # cargo fix --allow-dirty
```

CI 流程在 `ci/build.sh` 与 `ci/audit.sh` 中串联。`ci/audit.sh` 包含 fail-fast 门禁：审计失败立即 exit 1。

### 审计脚本 (`scripts/`)

| 脚本 | 检查目标 |
|------|----------|
| `audit_services_boundary.py` | services 是否越界访问 framework 内部模块 |
| `audit_safety_coverage.py` | framework 中每个 unsafe 块是否配 `// SAFETY:` 注释 |
| `audit_deadlock_matrix.py` | 锁顺序、中断上下文、sleep 锁、不可重入函数 |
| `audit_coupling.py` | 模块间循环依赖 |
| `audit_invariants.py` | 6 安全不变式断言 |
| `audit_tcb_ratio.py` | TCB 占比统计 |
| `audit_comment_language.py` | 中文注释强制 |
| `audit_block_registration.py` | 块设备注册 |
| `audit_once_cell.py` | OnceCell 模式统一 |
| `audit_c_naming.py` | C 命名规范 |
| `audit_repr_c.py` | repr(C) 字段错位检查 |
| `audit_volatile_access.py` | volatile 访问检查 |
| `audit_static_mut.py` | static mut 使用审查 |
| `audit_dead_code.py` | dead_code 禁止 |
| `audit_smoltcp_purity.py` | smoltcp vendored 纯净性 |
| `audit_edition2024.py` | Edition 2024 兼容性 |
| `audit_public_api_docs.py` | 公共 API 中文文档 |

### 测试分层

- `framework/tests/`: 31 个 no_std 单元测试，手动注册 + QEMU Runner
- `host-tests/`: 769 个测试 (含内联单元测试 + Cargo 自动发现的集成测试 + Plan B 契约测试 + 性能基准)
- `src/rust/queenx-tests/`: 用户态集成测试桩

> **miri-tests 已于 2026-06-26 删除** (4883 行死代码). UB 检测由 Rust 编译期 + 7 个审计脚本覆盖.

### 内置 make 测试目标

```bash
make test             # test-host + test-unit
make test-host        # host-tests cargo test
make test-unit        # kernel 内置单元测试 (需要 build/kernel_test.bin)
make test-smoke       # ISO 引导冒烟
make test-stress      # ISO 压力测试
make test-integration # ISO 集成测试
make test-all         # smoke + host + unit
make test-chaos       # fault_injection feature 启动混沌测试
make test-smp         # SMP 多核测试
```

## 代码组织

```text
.
├── AGENTS.md                项目开发规范 (硬约束 + AI 行为准则)
├── CHANGELOG.md             变更日志 (按日期 + 子特性代号)
├── LICENSE                  MIT
├── README.md
├── Makefile                 双架构构建入口
├── Makefile.ci              本地 CI 目标
├── clippy.toml              内核级 Clippy 阈值 (cognitive-complexity 25 等)
├── deny.toml
├── ci/                      CI 编排脚本 (build.sh, audit.sh)
├── scripts/                 17 个 Python 审计与检查脚本
├── tools/                   check_tcb.sh 等辅助
├── docs/
│   ├── README.md            文档写作规范
│   ├── explain/             架构与子系统解释 (7 篇)
│   │   ├── explain-framekernel.md
│   │   ├── guide-dev.md
│   │   ├── spec-engineering.md
│   │   ├── linux-compat-philosophy.md
│   │   ├── ref-lock-order.md
│   │   ├── ref-naming.md
│   │   └── vision-hope.md
│   └── plan/                路线图与立场书
│       ├── future-roadmap.md
│       └── archive/         已完成/归档的计划文档
├── src/
│   ├── kernel/
│   │   ├── framework/       TCB (允许 unsafe)
│   │   └── services/        去特权 (100% safe)
│   │       └── fs/inode.rs  Plan B: Inode trait + 7 FS 原生实现
│   ├── rust/                内核 crate queenx
│   │   ├── src/lib.rs       panic handler + kernel_init() 启动序列
│   │   └── queenx-tests/    用户态集成测试桩
│   └── user/                用户态 Rust 程序
│       ├── init/            PID 1
│       ├── eash/            Easy Shell
│       ├── install/         安装向导
│       ├── fbterm/          帧缓冲终端
│       ├── httpsrv/         HTTP 服务器
│       └── lib/             用户态共享 lib
├── host-tests/              769 个测试 (单元/集成/契约/性能)
└── build/                   构建产物 (kernel.bin / kernel.flat / .iso / .img)
```

## 文档索引

| 路径 | 内容 |
|------|------|
| [AGENTS.md](file:///home/anfer/Code/QueenX/AGENTS.md) | 项目硬约束 + AI 行为准则：编码风格、构建、测试、审计、编码前先思考、简单优先、外科手术式修改、目标驱动 |
| [CHANGELOG.md](file:///home/anfer/Code/QueenX/CHANGELOG.md) | 全部变更（时间倒序，无项目级版本号） |
| [docs/README.md](file:///home/anfer/Code/QueenX/docs/README.md) | 文档写作规范 |
| [docs/explain/explain-framekernel.md](file:///home/anfer/Code/QueenX/docs/explain/explain-framekernel.md) | 框内核定义与原理 |
| [docs/explain/guide-dev.md](file:///home/anfer/Code/QueenX/docs/explain/guide-dev.md) | 框内核开发与维护指导 |
| [docs/explain/spec-engineering.md](file:///home/anfer/Code/QueenX/docs/explain/spec-engineering.md) | 工程纪律性规范 (0 铁律 + 13 章节) |
| [docs/explain/linux-compat-philosophy.md](file:///home/anfer/Code/QueenX/docs/explain/linux-compat-philosophy.md) | Linux 兼容策略 (三层兼容) |
| [docs/explain/ref-naming.md](file:///home/anfer/Code/QueenX/docs/explain/ref-naming.md) | 命名、syscall 编号、libc 选型、linuxulator 立场 |
| [docs/explain/ref-lock-order.md](file:///home/anfer/Code/QueenX/docs/explain/ref-lock-order.md) | 锁顺序参考 |
| [docs/explain/vision-hope.md](file:///home/anfer/Code/QueenX/docs/explain/vision-hope.md) | 项目愿景 |
| [docs/plan/future-roadmap.md](file:///home/anfer/Code/QueenX/docs/plan/future-roadmap.md) | 未来路线图 |
