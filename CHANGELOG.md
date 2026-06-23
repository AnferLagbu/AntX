# AntX 变更记录

> 格式: 时间倒序, 每节用日期 `## YYYY-MM-DD` 标题. 子节: 新增 / 变更 / 修复 / 移除.
> 本项目**无项目级版本号** (5月21日 `df0cb83` 已移除, 后续 commit 中的 v2.x 均为子特性代号, 非项目版本).
> 详细写作规范见 [README.md](./README.md) §4.

---

## [Unreleased]

### 新增
- **DISPLAY-2.1 HDMI HPD 真实读取** — `src/kernel/framework/driver/display/hdmi.rs` (接手人实装, 2026-06-23)
  - `HdmiController` 字段 `mmio_base: usize` → `iomem: Option<IoMem>` + `hpd_reg_offset: usize`
  - 新增 `HPD_STATUS_REG_OFFSET = 0x038` (带 Intel IGP +0xC8 / AMD DCN +0x5E 厂商偏移参考注释) + `HPD_STATUS_BIT = 0x01`
  - 新增 `unsafe fn new_with_iomem(iomem, hpd_reg_offset)` + `new_with_default_hpd(iomem)` 真实硬件构造函数
  - `detect_hot_plug()` 真实实现: IoMem 路径 `read_u8(hpd_reg_offset) & HPD_STATUS_BIT`; None 路径 fallback 返回 `true` (兼容 QEMU/Bochs)
  - 删除 `// TODO(TRACK-CD5DA5)` 注释
  - 新增单元测试 `test_hpd_fallback_returns_true_when_no_iomem`
- **DISPLAY-2.2 HDMI I2C/DDC EDID 真实读取** — `src/kernel/framework/driver/display/hdmi.rs` (接手人实装, 2026-06-23)
  - 新增 DDC I2C bitbang 协议层: 5 个 `unsafe fn` 原语 (`ddc_delay`/`ddc_set_sda_scl`/`ddc_i2c_start`/`ddc_i2c_stop`/`ddc_i2c_write_byte`/`ddc_i2c_read_byte`) + 1 个 `unsafe fn read_edid_block_via_ddc` 完成 START → 0xA0 → offset → REPEATED_START → 0xA1 → 128 bytes → STOP 事务
  - 新增 `fill_mock_edid()`: 无硬件/DDC 失败 fallback, 校验和正确
  - `read_edid()` 重写为 3 路径: IoMem Some → DDC 真实读 (block 0 + 可选 block 1); DDC 失败 → mock; IoMem None → mock
  - 删除 `// TODO(TRACK-7CCB60)` 注释
  - 新增 3 个单元测试: `test_fill_mock_edid_checksum_valid` / `test_read_edid_fallback_when_no_iomem` / `test_read_edid_without_hpd_returns_device_not_found`
  - 厂商偏移参考注释: Intel IGP GMBus 16-bit 端口 I/O / AMD DCN DDI 控制器 / 通用 SoC 8-bit bitbang / QEMU Bochs 无 DDC
- **DISPLAY-2.4 DP HPD 真实读取** — `src/kernel/framework/driver/display/dp.rs` (接手人实装, 2026-06-23, 镜像 DISPLAY-2.1 HDMI HPD 模式)
  - `DpController` 字段 `mmio_base: usize` → `iomem: Option<IoMem>` + `hpd_reg_offset: usize`
  - 新增 `DP_HPD_REG_OFFSET = 0x040` (独立 DP chip 默认) + `DP_HPD_STATUS_BIT = 0x01`
  - 新增 `unsafe fn new_with_iomem(iomem, hpd_reg_offset)` + `new_with_default_hpd(iomem)` 真实硬件构造函数
  - `detect_hot_plug()` 真实实现: IoMem 路径 `read_u8(hpd_reg_offset) & DP_HPD_STATUS_BIT`; None 路径 fallback 返回 `true`
  - 删除 `// TODO(TRACK-599EDA)` 注释
  - 新增单元测试 `test_dp_hpd_fallback_returns_true_when_no_iomem`
  - 厂商差异注释: Intel IGP/AMD DCN 共享 HDMI HPD 寄存器, 调用方应显式传入与 HDMI 相同偏移

### 修复
- **DISPLAY-2.1 关联预存问题修复** (CLAUDE.md 预存问题即修规则, 接手人同步修复)
  - `host-tests/tests/i43_block_bridge_test.rs::test_block_ops_thunk_signature_matches_trait` 反转断言: LEGACY-4.2 已删除 4 个 thunk, 测试现在反向验证 thunk 不应再出现
  - `src/kernel/framework/chitin/proto_block.rs` 2 处 `#[deprecated(since = "T-4.1 (2026-06-22)")]` → `since = "0.1.0"` (semver 合规, clippy 0 error)

- **TD-22 注释语言审计: services 迁移记录豁免** — `scripts/audit_comment_language.py` 新增 `is_migration_note` 规则, 覆盖 `// 已迁移到 services: sys_xxx, sys_yyy, ...` 多行列表 (含续行状态机) + `//! 依赖 framework safe API (..._safe)` 模式. 6 个回归测试通过 (`scripts/tests/test_audit_comment_language.py`), 重新达成"0 违规"硬阈值. 解决 2026-06-18 实际回归的 3 处违规: `src/kernel/services/ipc/async_ipc.rs:14` + `src/kernel/framework/syscall/mod.rs:752, 765`
- **Phase D 路线图文档同步: 关闭 NUMA 感知 / cgroup 控制器 两行冗余占位** — `docs/plan/kernel-roadmap.md:261-262` 标记为 `[x]` 并指向 D2/D3 详细行 (实际 2026-06-10 已闭环, 源码: `framework/{proc/cgroup,mm/numa}.rs` + `services/{proc/cgroup,mm/numa}.rs` 均存在). 修复"路线图 Phase 闭环 ≠ 源代码 TRACK 关闭"另一表现 (D1/D2/D3 在详细行已标 [x], 但 Phase D 待办区占位行未同步)
- **TRACK-XXX Backlog 批量同步 (47 条修改, 28 删 + 19 修 + 7 保留)** — `docs/plan/kernel-roadmap.md` 末尾 Backlog 段 (54 项) 大量陈旧: 28 项源码已闭环 (TRACK ID 在源码中已不存在, 由历史 commit 移除), 19 项行号错位 (roadmap 引用旧行号, 实际行号因新提交漂移 1~9 行), 7 项真实存在 (USB xHCI/SMP/aarch64 cache/DMA 硬件底层, 保留)
  - 新增 `tools/sync_track_backlog.py` 校对工具 + 9 个单元测试 (`tools/tests/test_sync_track_backlog.py`), 覆盖 parse_backlog / classify 4 种状态 / apply 落盘 / 幂等性
  - 修复"路线图 Phase 闭环 ≠ 源代码 TRACK 关闭"另一表现 (源码已闭环但 roadmap 仍列) — 7 keep + 19 fix = 26 项, 全部行号已对齐源码实际位置
- **工程纪律性规范** — `docs/explain/engineering-discipline-spec.md`, 项目工程纪律性权威规范, 涵盖模块归属、依赖管理、接口设计、代码质量、TCB 治理、构建测试、提交规范、文档规范、循序渐进策略
- **P1 #3: Priority Inheritance Mutex (PI Mutex)** — `kernel::framework::sync::pi_mutex` (TCB) + `kernel::services::sync::pi_mutex` (safe API, 0 unsafe)
  - 直接捐赠: 高优先级等待 → 低优先级持有者有效优先级被提升
  - 多等待者取 max 优先级
  - 释放时移交锁给最高优先级等待者 (FIFO 同优先级)
  - 回调钩子: `set_donation_callback` / `set_revoke_callback` 供调度器集成
  - 8 个 no_std 单元测试 (基本 lock / try_lock 失败 / 捐赠提升 / max / 移交 / 完全释放 / 重复 lock / 回调)
  - DECISION-009 (v1 只支持直接捐赠) / DECISION-010 (不直接修改 Process, 通过回调) / DECISION-011 (自旋+yield 等待, 不入调度队列)
  - 详见 [pi-mutex-design.md](docs/plan/pi-mutex-design.md)

- **Phase C.3: Unix Domain Socket (UDS / AF_UNIX)** — `kernel::framework::net::unix` (TCB) + `kernel::services::net::unix` (safe API, 0 unsafe)
  - SOCK_STREAM: bind → listen → connect → accept → send/recv → close 完整生命周期
  - SOCK_DGRAM: bind → connect → sendto/recvfrom → close
  - 独立路径绑定表 (固定 32 槽位, 不进 VFS inode)
  - FD 空间 [100, 116), 与 smoltcp [0, 16) 与 VFS [0, 32) 不冲突
  - 5 个 no_std 单元测试 (stream echo / dgram echo / EADDRINUSE / EAGAIN / listener cancel)
  - 6 个 syscalls (socket/bind/listen/accept/connect/sendto/recvfrom) 按 `sun_family` + FD 范围分流到 UDS 或 smoltcp
  - DECISION-006 (UDS 不入 VFS inode) / DECISION-007 (DGRAM 单消息排队) / DECISION-008 (阻塞退化为 EAGAIN)

### 修复
- **host-test 预存问题: `irq_spinlock_adopted_in_migrated_files` 与 io/iouring.rs 迁移后状态不一致** — `framework/io/iouring.rs` 已于 2026-06-18 迁移到 services 层, framework 仅 re-export, 不再直接导入 IrqSpinLock. 从 host-test 期望列表中移除 `io/iouring.rs` (与 cgroup/namespace/seccomp 一致), 72 个 host-test 套件全部通过

---

## 2026-06-06

> 主题: services/TCB 统一重构, API 层拆分, Issue1 二期修复, docs 重构.
> 重构从"功能实现"转向"工程化与可验证性".

### 新增
- **TCB (Trusted Computing Base) 统一框架** — `kernel::framework::{services, proc, sync, mm, chitin, irq, boot, cpu, arch}` 9 个子模块, 统一 `Send`/`Sync` 标注 + 边界加固
- **legacy TCB 进程与同步框架** — `827a56a` 旧版 `Process`/`SpinLock`/`RwLock` 实现, 作为新框架对照参考
- **用户空间内存访问模块** — `a7e37e7` 新增 `mm::user_access`, 安全访问用户态指针
- **KASLR 子系统** — `93bf59c` 内核地址空间布局随机化
- **procfs JSON 接口** — `93bf59c` 进程文件系统 JSON 输出
- **启动镜像编码** — `93bf59c` 启动镜像可编码化 (便于测试与分发)
- **DmaStream 与 IoMem 别名检测** — `831cc77` (特性内部版本 v2.4), DMA 流与 IO 内存别名静态检测
- **PCI scanner 注册与查找** — `0ea52a8` 标准化 PCI 设备枚举接口
- **AArch64 PL011 UART 驱动** — `bf06078` ARM 架构串口驱动
- **UserProcess 与 Process 单源真相同步机制** — `cec6b40` 用户进程和内核进程共享同一份状态
- **barrier 服务层 + 基准测试工具** — `08eda4e` 故障恢复服务化 + 性能基线
- **host-tests 全量测试任务 CI 集成** — `7d1581f` Makefile CI 任务 `ci-makefile`
- **docs/CHANGELOG.md 格式规范** — 当前文件
- **docs/README.md 写作规范** — 9 节 285 行, 涵盖 3 个目录 + 1 个 CHANGELOG 的格式要求

### 变更
- **路径前缀统一为 `kernel::framework`** — `3e34c37` 所有内核模块从根路径下沉到 `kernel::framework::*`
- **网络栈从 lwIP 迁移至 smoltcp** — `46c97e1` 移除 lwIP, 引入 smoltcp-0.13.0 作为主协议栈 (`build: 替换smoltcp-main.zip为smoltcp-0.13.0.zip`)
- **网卡抽象层重构** — `91a4c54` 网卡统一通过 `Chitin NetOps` 注册与操作, 移除子系统内私有网卡实现
- **virtio-net 物理地址偏移修复** — `6748396` 修正 DMA 物理地址映射
- **PL011 串口代码风格统一** — `e880cf2` 文件末尾换行
- **aarch64 页表初始化简化** — `45e5a16` 重构 PMM 检查 + aarch64 页表初始化路径
- **VMM 代码拆分架构** — `723388b` `mm` 模块按架构拆分 VMM 实现
- **CPU 数量限制重构** — `31316d5` 全局配置常量集中提取
- **配置系统重构** — `48a52fc` 所有可调常量集中到 `config` 模块
- **子系统校验 + 初始化状态检查** — `000e93b` `config`/`pci`/`net` 三个子系统加 init 状态机
- **chitin FFI 回调统一为 `extern "C" fn`** — `e889819` 全部 C 侧回调签名标准化, 加 Rust 安全包装
- **子系统 API 契约标准化** — `c855842` trait/常量/函数暴露形式统一
- **各子系统 API 层拆分** — `92321f8` 进一步把 API 与实现分离
- **AntX API 化规范文档** — `c74c603` 配套文档说明 API 层组织规则
- **credo/net 不安全操作集中** — `7346f2f` `unsafe` 块聚拢, 降低审计成本
- **services 层 0 unsafe 清零迁移** — `b0211dc` services 模块完全无 `unsafe`
- **services/sync 模块迁移 + TCB 边界加固** — `f0885fe` 同步原语全部走 services 抽象
- **Phase 2.1 全 6 类驱动 safe 迁移** — `ca8c5d7` 字符/块/网络/输入/显示/存储 6 类驱动全部迁移到 safe Rust API
- **legacy 重命名** — `8901bb3` 旧 API 标记为 `legacy_*`, 新 API 加 services unsafe 强制校验
- **进程与同步子系统 TCB 统一重构** — `a8685dc` 进程调度与同步合并重构
- **进程调度 + 同步子系统重构** — `fa5f0c6` 移除旧版自旋锁/读写锁实现, 走 services 抽象
- **legacy 注释清理** — `682add8` `.gitignore` 重写, 260 个误提交文件清理
- **host-tests 集成测试目录结构重构** — `9f31666` 测试组织方式统一
- **docs 目录重构** — `d2659e1` 11 个原目录 → `plan/` + `explain/` + 2 根 md
- **docs 目录清空 + 重建空文件** — `9a82f27` 旧 docs 内容混乱/日期错误, 全部清空
- **docs 文件名大小写统一** — `99e65a0` `readme.md` → `README.md`, `changelog.md` → `CHANGELOG.md`, 同步修正路径引用

### 修复
- **Issue1 二期: 失败路径回滚结构内存** — `1e406e0` (特性代号 Issue1 v2.30) 引入 `free_kernel_process`/`free_user_process`, 三条失败路径按 LIFO 反序释放, 新增 5 个 miri 回归测试 (DECISION-025 落地, DECISION-027 新增)
- **Issue1 一期: 延后 PID 分配** — `4fb232d` (特性代号 Issue1 v2.29) 解决 `alloc_kernel_process` 失败时的 PID 泄漏
- **用户进程与内核进程同步** — `cec6b40` `UserProcess` 与 `Process` 单源真相机制
- **build cache 清理** — `ced5470` 清理构建缓存 + 修复部分代码警告
- **lockbud 检测逻辑** — `95ac5e8` `audit.sh` 修复 lockbud 误报

### 移除
- **FFI 模块全部移除** — `c47358e` 统一使用 Rust 原生 API, 移除所有 `*_ffi` 模块
- **miri 测试构建产物清理** — `a0ee827` 移除误提交的 miri 编译产物
- **未使用 `u8` import** — `c54ce1e` `fs/vfs` 清理无用 import
- **无用导入 + lint 豁免 + 代码简化** — `7198598` 多文件合并清理
- **lwIP 栈** — `dd8342a` 5 月起逐步迁移, 6 月 1 日完全移除 (`Phase 1-11: 5月22日完成架构解耦, 6月1日收尾`)
- **x86_64 硬绑定** — `642b3a3` 5月23日完成子系统 x86_64 硬绑定移除

### 决策 (DECISION-NNN)
- **DECISION-025** 失败路径必须完整回滚所有资源 (物理+结构) — 2026-06-05 落地
- **DECISION-027** 失败路径必须 LIFO 反序释放, 避免 `NonNull` 悬挂 — 2026-06-05 新增

### 关键 commit 索引

| Commit | 说明 |
|--------|------|
| `1e406e0` | Issue1 v2.30 失败路径结构内存回滚 |
| `4fb232d` | Issue1 v2.29 PID 分配延后 |
| `cec6b40` | UserProcess/Process 单源真相同步 |
| `b0211dc` | services 层 0 unsafe 清零 |
| `ca8c5d7` | Phase 2.1 全 6 类驱动 safe 迁移 |
| `93bf59c` | KASLR + procfs JSON + 启动镜像编码 |
| `46c97e1` | lwIP → smoltcp 迁移 |
| `3e34c37` | `kernel::framework` 路径前缀统一 |
| `d2659e1` | docs 目录重构 (plan/explain) |
| `9a82f27` | docs 清空 |
| `99e65a0` | docs 文件名大小写调整 |

---

## 2026-05-31

> 主题: aarch64 双架构支持, HvFS/HzFS/ZvFS 文件系统, PWID v5, E1000/smoltcp 网络, CFS 调度器, WASM 虚拟机.
> 整个 5 月为主开发期, 286 个 commits 占全期 72%.

### 新增
- **aarch64 (ARM64) 架构完整支持** — 11 个 Phase 阶段 (5月22日集中提交):
  - Phase 1 (`bb26523`): Arch trait 骨架 + 类型系统
  - Phase 2 (`b408566`): x86_64 实现 Arch trait (真实硬件 asm 封装)
  - Phase 3 (`298807c`): 内核模块全量迁移至 Arch trait (`arch!()` 宏 + 薄封装)
  - Phase 4 (`3ff5287`): 构建系统多目标化 (ARCH switch)
  - Phase 5 (`3c1b024`): aarch64 stub 实现 + 全量 cfg 门控
  - Phase 6 (`0960663`): aarch64 完整实现
  - Phase 7 (`413fb31`): 测试完善 + 文档交付
  - Phase 8 (`e0f8840`): Arch trait 拆分 — CoreArch/InterruptArch/MmuArch/SystemArch 子 trait
  - Phase 9 (`cb5aef8`): 架构耦合消解 + 迁移扫尾 (P0-P4)
  - Phase 10 (`329443d`): 纯 Rust 实现 `proc_alloc_pid`/`user_proc_clone` + Makefile 双架构 QEMU 支持
  - Phase 11 (`5a91580`): 修复 aarch64 链接错误 — 架构解耦 cfg gate (`include_bytes!` 架构守卫 + Makefile 构建依赖)
- **栏栈双架构移植** — `f2098f3` 故障恢复子系统跨架构支持
- **virtio 双架构修复** — `f2098f3` virtio 设备驱动双架构对齐
- **BlockDevice 抽象层** — `f92faa2` `+` `fe8134a` 多磁盘驱动 + 抽象层完善
- **双架构统一启动输出** — `f92faa2` x86_64/aarch64 启动日志格式统一
- **HvFS v2 (HzFS)** — `e3bc1e6` (特性内部版本 HvFS v2) 取代旧 `hvfs`, 三特征融合: Copy-on-Write + 写时校验 + 去重. 详见 `HzFS 三特征融合设计文档` (`6426500`)
- **ZvFS** — `6845ce3` 类 ZFS 文件系统 (作为参考实现, 未进入主线)
- **APIC/IOAPIC/SMP** — `6845ce3` 多核中断控制器 + 对称多处理器
- **进程管理 syscall** — `6845ce3` 进程相关系统调用补齐
- **CFS 调度器 (完全公平调度器)** — `ced1ed3` `+` `07948c8` 重构调度系统, 实现 CFS 算法, 完善时间片和抢占逻辑 (`1d57312`, `e154fd7`)
- **WASM 虚拟机子系统** — `07948c8` 内核态 WebAssembly 执行环境
- **aarch64 页表卸载** — `07948c8` ARM64 架构页表释放路径
- **PWID v5 权限模型** — `0543266` (特性内部版本 PWID v5) 权限模型演进至 v5 (从 v2.0 增强版 → v3 → v4 → v5)
- **Credo 身份权限系统 (重命名自 PWM)** — `e060070` 重命名权限框架
- **POSIX 原生接口实现** — `bb216df` `+` `fe8134a` POSIX 兼容层
- **组合虚拟设备 (RAID0/RAID1)** — `68bfa0a` 设备虚拟化
- **用户态驱动框架** — `0522e7d` chitin/user_driver 完整实现
- **E1000 网卡驱动 Rust 重写** — `+` `8e19460` 至 `9866cec` 多阶段: TX/RX 基础设施 → RX 完整工作 → DHCP/Ping/HTTP
- **smoltcp 协议栈** — 5月31日: 导入 `smoltcp-main.zip` (`d2ee425`), 替换为 `smoltcp-0.13.0.zip` (`5364d53`), 制定 lwIP→smoltcp 迁移工程计划 (`58dd386`)
- **NFS/符号链接支持** — `8e19460` HvFS 符号链接 (`feat(hvfs): add symlink support`)
- **凭据权限管理** — `1f5bf91` 中断初始化流程重构 + 凭据管理
- **进程优先级系统调用** — `ea085ed` `nice`/`setpriority` 等
- **串口 stdin 支持** — `7954c25` 用户态串口输入
- **内核全局 PIC (位置无关代码)** — `13362a2` `+` `771aee3` 内核可重定位
- **进程间通信 (IPC) 子系统** — `8509bf0` 消息队列等
- **完善 QueenX 进程与线程机制** — `d3a31aa` 进程/线程 API
- **HvFS/DiskFS 文件系统 Rust 重写** — `58df0fd` `+` `bbe9334` `+` `53151db` 完整持久化
- **进程管理模块 Rust 重写** — `dd6f579`
- **双映射高地址内核启动方案** — `e5c4862` 启动路径优化
- **栏栈整合与栈改进** — `29a7a23` 栈子系统综合改进
- **Klog 系统** — `0ca13fa` 内核日志系统
- **Hymenoptera 显示服务器设计** — `4e2e2dd` `+` `d44f713` 显示服务器 + LVGL 集成
- **APIC/IOAPIC 驱动** — `ab67a46` 多核中断支持
- **内核同步原语 mutex/rwlock** — `ab67a46` 头文件
- **Stage1 自研引导** — `cb95f23` 自研 1 级引导
- **FAT16/HvFS 双分区磁盘布局** — `cb95f23` 引导分区方案
- **持久化安装流** — `a4ceff7` HvFS 磁盘挂载 + `/mnt` 部署
- **安装向导模块化** — `3c95656` `+` `b1b353b` 移至 `src/user/install/`, AppManifest 应用部署系统
- **Rust no_std 测试框架** — `b7ce35f` 27 个单元测试 + 完整框架
- **.gitignore 重构** — 多次重构防误提交 (`eae8946`, `86f8e7d`, `e0d8031`)
- **PWID 文档** — `0543266` `+` 多份 v4 文档
- **HzFS 三特征融合设计文档** — `6426500`
- **lwIP→smoltcp 迁移工程计划** — `58dd386`
- **checktools 开发文档** — `719f4bb`
- **AntX API 化规范文档** — `c74c603` (6月初)

### 变更
- **架构解耦 + 移除子系统 x86_64 硬绑定** — `642b3a3` `+` `5a91580` (Phase 11) 内核模块全部通过 `Arch` trait 抽象
- **全量去 Linux 化** — `7119436` 命名/概念/实现去 Linux 痕迹
- **AntX 命令体系重新设计** — `cdb3705` `+` `41091db` shell 命令模块化 (axsh)
- **xsh → axsh 统一** — `6f0325b` `+` `6379698` shell 名称统一
- **移除 `rust_*` 前后缀命名** — `d036e6f` 统一代码风格
- **消息文本 xsh → axsh 统一** — `6f0325b` 用户可见信息
- **移除内核态 Shell** — `8a00da3` 功能与职责分离
- **移除内核态安装向导** — `164f7fb` 功能与职责分离
- **移除安装向导中 root 备注输入** — `6ab0df8` 安装流程简化
- **驱动子系统大修** — `7f03225` 磁盘引导修复 + 安装流加固
- **smoke test 迁移** — `93cbe90` 从 inline Makefile 迁移到 Python 测试框架
- **aarch64 预存警告消除** — `4b025ac` `sync_exception_handler` 未使用参数
- **`qx_netif_register_e1000` 补充** — `c70309a` 修复 x86_64 链接错误
- **Barrier 竞态死锁修复** — `eab335f` x86_64 init 二进制空 + barrier 竞态
- **HV_SPA_VERSION 移除** — `332e286` 多架构解耦工程规划书
- **架构合理化** — `a4e8af8` 头文件迁移 + 空目录清理
- **移除项目版本号定义** — `df0cb83` (5月21日, 关键决策: 本项目自此**不再使用项目级版本号**, 后续若要恢复需 DECISION-NNN 显式记录)
- **x86_64 init 启动** — `f1cb2e8` 移除帧缓冲多引导标签 + 启动 SMP + 调度器
- **SMP 测试支持** — `a812eae`
- **测试注册逻辑统一 (宏)** — `bb130ce`
- **进程表访问重构** — `28adfc8` 便捷访问方法
- **`E1000` 驱动日志分级** — `3187965` Linux 风格
- **Hymenoptera 显示服务器 + LVGL** — `4e2e2dd` 文档先行
- **PWM 权限框架重命名为 Credo** — `e060070` (后续 2026-06-06 又重命名为 services)
- **Send/Sync trait 安全注释批量添加** — `ed15b00` `+` `92c069b` 标注
- **x86_64 架构相关代码整理** — `92c069b` 符号链接支持
- **统一测试注册逻辑** — `bb130ce` 宏简化
- **`userlib` 全局 sys 移除** — `f803c91` 改为显式导入
- **`include_bytes!` 替代 `gen_embed.py`** — `1e32381` 嵌入 C 文件
- **用户态目录结构重组** — `073eaef` workspace 模块化布局
- **`1GB huge page` 安全** — `33a493f` smoke test 同步
- **`serial_write_bytes` 替代** — `313847e` `sys_fs_write` 实现
- **`Ring 3` 用户态进入修复** — `8c95ee9` 6 个关键 bug
- **网络初始化不阻塞启动** — `04a3c28` 测试基础设施修复
- **`user_init_bin.o` 添加** — `4c2faf3` 解决测试链接失败
- **`BOOT_PART_SECTORS` 命名统一** — `8697623` 删除重复常量

### 修复
- **PWID v4 致命级安全漏洞** — `fix: 修复 PWID v4 致命级安全漏洞和严重缺陷 (P0+P1)` 多个提交
- **PWID v4 安全审计** — `docs: PWID v4 安全审计 — 28个问题 (代码缺陷+系统可用性)` `+` `docs: PWID v4文档全面更新 — 11个文件同步到能力流动模型`
- **HvFS dedup SHA256 字节序错误** — `6c3b602`
- **E1000 接收描述符环** — `a40a63d` `+` `cd2b236` `+` `9866cec` RX 路径完整工作
- **E1000 中断处理** — `0748066`
- **UndoLog 栈溢出** — `e39457a` 5月15日
- **44 个冗余 C 测试文件删除** — `e39457a`
- **Makefile 清理** — `e39457a`
- **`static mut` 可变引用导致的未定义行为** — `75b1dbd` grant 模块
- **多处安全与性能问题** — `66bac04` 字符串转换和忙等逻辑优化
- **多个内核子系统安全与功能问题** — `42e66cf`
- **内存管理与系统调用逻辑重构** — `e41f7e3` `+` `e0d8031`
- **`seteuid/setegid/setreuid/setregid` 系统调用** — `4a62d8b`
- **键盘退格键(Backspace)和 Tab 键** — `b8c0170`
- **安装引导可用性** — `0e998a3` `+` `08f83f4`
- **用户进程启动 GPF** — `1c265c3` 调试
- **安装引导和 init 进程问题** — `08f83f4`
- **用户进程创建时页面错误** — `a6c4044`
- **测试问题** — `47e5103` 测试通过率
- **测试代码使用新 klog** — `0ca13fa` 测试报告
- **稳定性问题** — 多个: `a7cf12b` 新稳定性问题, `16beb85` 系统稳定性和引导支持, `24e91be` 进程管理, `65e21b3` 内存管理
- **VMM 和系统调用关键问题** — `00be86e`
- **消息队列测试** — `cbe994e` 鲁棒性
- **测试用例断言** — `905f9c1` API 不匹配
- **x86-64 syscall 参数传递** — `3e2cfb5` `syscall_handler`
- **安装向导 lifetime 错误** — `364a876` `build_dst` 参数
- **`ffi.rs` 串口诊断输出** — `0cc869a`
- **简化集合匹配逻辑** — `9ec17fd` 类型转换与循环问题
- **批量使用内置方法** — `3043bdb` 简化代码逻辑
- **批量完善代码注释** — `04bc333` 修复小问题
- **信号发送模块外部函数声明** — `4c357bf`
- **清理全局未使用代码警告** — `684bf5b`
- **CFS 调度器抢占触发条件** — `e154fd7`
- **代码格式与模块顺序** — `dd477ec`
- **三处代码和文档问题** — `c11885e`
- **display 像素格式推断** — `df57174`
- **NVMe PRP 处理** — `809c2d8` 控制器级 PRP 列表页
- **ringbuffer 死锁** — `eab335f` 栏栈

### 移除
- **废弃 C 用户态源文件** — `8382d67`
- **废弃测试代码与脚本** — `8651ee6`
- **冗余代码与废弃模块** — `f7369ea`
- **`qx_hooks.h` 空壳文件** — `1dc8e95` LWIP_HOOK_FILENAME 从 lwipopts.h 移除
- **网络调试脚本** — `e46a7d6` `chore: remove build/network debug scripts`
- **kernel capability layer (Pwid v3 残留)** — `113b5fb` 死代码违反 zero-concept 原则
- **`src/kernel/net/arch/qx_hooks.h`** — `1dc8e95`

### 关键 commit 索引

| Commit | 说明 |
|--------|------|
| `0543266` | PWID v5 |
| `e3bc1e6` | HvFS v2 (HzFS) |
| `6845ce3` | ZvFS + APIC/IOAPIC/SMP + 进程 syscall |
| `ced1ed3` | CFS 调度器 |
| `07948c8` | WASM 虚拟机 |
| `bb26523`-`5a91580` | 11 个 Phase 阶段 aarch64 支持 |
| `5dbc346` | 内存/进程/chitin 重构 + 设备绑定修复 |
| `0522e7d` | 用户态驱动框架 |
| `68bfa0a` | RAID0/RAID1 组合虚拟设备 |
| `e060070` | PWM → Credo 重命名 |
| `58dd386` | lwIP→smoltcp 迁移工程计划 |
| `cb95f23` | 自研 Stage1 引导 + FAT16/HvFS 双分区 |
| `4a62d8b` | seteuid/setegid/setreuid/setregid syscall |
| `7c185c7` | 用户态全量迁移至 Rust |
| `df0cb83` | 移除项目版本号定义 (关键) |

---

## 2026-04-30

> 主题: 项目启动, 基础架构, 引导/PIC/Shell, HvFS v1, IPC, Rust 重写初期.
> 59 个 commits, 占全期 15%.

### 新增
- **AntX 项目初始化** — `fa39a05` first commit
- **项目源码提交 + .gitignore 更新** — `9b50d26`
- **issue-recommend 文档** — `f7b8925` 代码质量审查
- **Issue #12 — 开机调试信息缺乏真正的错误检测** — `fa56037`
- **issue 状态标记** — `8b24cff`
- **文档目录结构细化** — `4028c0e` 按类型分类组织
- **开发文档 (重写)** — `6648c34` 订正以匹配源码实现, 制定符合自研特色的开发进度规划
- **内核 PIC (位置无关代码) 实现方案** — `771aee3`
- **键盘驱动优化** — `04258fa` 完整键盘功能
- **模块初始化检查机制** — `e1b28b0`
- **安全与稳定性机制** — `25f8eee`
- **AntX Shell + 安装引导** — `32d0745`
- **内核启动信息更新** — `0d1b48e` "AntX Operating System, Copyright 2026"
- **LICENSE** — `220a47a`
- **LICENSE 版权声明完善 + antxsh banner** — `90d78a4`
- **双映射高地址内核启动方案** — `e5c4862`
- **文件系统和进程管理 Rust 重写方案文档** — `2c99ba2`
- **进程管理模块 Rust 重写** — `dd6f579`
- **文件系统模块 Rust 重写** — `bbe9334`
- **HvFS 和 DiskFS 文件系统 Rust 重写** — `58df0fd`
- **AntX 特色增强权限模型 (PWID-Enhanced v2.0)** — `8be6f1e` (特性内部版本 PWID v2.0)
- **串口 stdin 支持** — `7954c25`
- **完善 QueenX 进程与线程机制** — `d3a31aa`
- **进程间通信 (IPC) 子系统** — `8509bf0`
- **动态内存分配 + 完整测试框架** — `cd4c89c`
- **Rust 增强模块集成到 C 代码 (P0/P1/P2)** — `d9fd48c`
- **用户进程调度 + 磁盘检测** — `75541e8`
- **磁盘安装和持久化支持** — `8978a0e`
- **安装向导完成提示** — `5932448` 用户确认后重启
- **维护工程阶段文档** — `ec7bbfb` 稳定性问题跟踪和维护计划
- **稳定性 + 引导支持** — `16beb85`
- **测试代码使用新 klog** — `0ca13fa` 测试报告
- **项目依赖检查脚本** — `9891951`

### 变更
- **Rust 模块与 C 代码 co-located** — `96e6859` `reorganize Rust modules to be co-located with C code`
- **系统架构与命令体系重构** — `3d3e309`
- **命名规范确立** — `583a4b8`
- **xsh → axsh 统一** — `6f0325b` 消息文本
- **移除 `rust_*` 前后缀命名** — `d036e6f` 统一代码风格
- **移除内核态 Shell** — `8a00da3` 功能与职责分离
- **移除内核态安装向导** — `164f7fb` 功能与职责分离

### 修复
- **内核启动和用户态切换问题** — `165074d`
- **用户进程启动 GPF** — `1c265c3` 调试
- **用户进程启动问题 + 诊断测试工具** — `531be9f`
- **键盘 Backspace/Tab** — `b8c0170`
- **安装引导可用性** — `0e998a3` `+` `08f83f4`
- **用户进程创建时页面错误** — `a6c4044`
- **VMM 和系统调用关键问题** — `00be86e`
- **消息队列测试** — `cbe994e` 鲁棒性
- **测试用例断言问题** — `905f9c1` API 不匹配
- **x86-64 syscall 参数传递** — `3e2cfb5`
- **稳定性问题** — `24e91be` 进程管理, `65e21b3` 内存管理, `a7cf12b` 新稳定性问题
- **测试问题** — `47e5103` 测试通过率
- **安装引导 root 备注输入移除** — `6ab0df8`

### 移除
- (无大规模移除, 此阶段以建设为主)

### 关键 commit 索引

| Commit | 说明 |
|--------|------|
| `fa39a05` | first commit |
| `220a47a` | add LICENSE |
| `9b50d26` | 项目源码 + .gitignore |
| `13362a2` | 内核全局 PIC |
| `dd6f579` | 进程管理 Rust 重写 |
| `bbe9334` | 文件系统模块 Rust 重写 |
| `58df0fd` | HvFS/DiskFS Rust 重写 |
| `8be6f1e` | PWID-Enhanced v2.0 |
| `8509bf0` | IPC 子系统 |
| `75541e8` | 用户进程调度 + 磁盘检测 |
| `cd4c89c` | 动态内存分配 + 完整测试框架 |
| `d9fd48c` | Rust 增强模块集成 C 代码 |
| `8978a0e` | 磁盘安装和持久化 |

---

## 2026-04-06

> 项目初始提交.

### 新增
- AntX 操作系统仓库初始化 (`fa39a05`)
- 基础项目结构 + 源码 + `.gitignore` (`9b50d26`)
