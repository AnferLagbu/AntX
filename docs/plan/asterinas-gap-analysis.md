# QueenX vs Asterinas 差距分析与远期代办

> 基于 2026-06-28 全量代码扫描与逐项对比,识别 QueenX 相对 Asterinas 0.18.0 的结构性差距,作为远期代办任务记录. 评估基准: Asterinas 源码位于 `tmp/asterinas-main/`.

## 背景
- **背景条目**
  - 描述: 通过深度扫描 Asterinas 源码 (kernel 119K + ostd 41K + comps 38K + libs 16K = 214K LoC),识别 QueenX (framework 107K + services 53K + smoltcp 59K = 219K LoC) 在架构、功能、工程化三个维度的差距
  - 方案: QueenX 自研代码量已达 Asterinas 75%,但 TCB 比例 64% vs 19% 差距显著; 功能上调度/容器/安全/驱动/栏栈领先, 但文件系统种类/syscall 覆盖/CI 文档落后; 结论: 12 项差距中 3 项 P0, 4 项 P1, 3 项 P2, 2 项 P3
  - 状态: [X]

## 关键发现
- **发现 1: 组件化是 TCB 比例差距根本原因**
  - 描述: Asterinas Cargo workspace 48 个 members (comps 14 + ostd libs 18 + osdk 8 + 其他 8), QueenX 单体 framework crate (107K)
  - 方案: 拆 framework 为 ~15 个独立 crate 是 TCB 从 64% 降至 30% 以下的关键路径; 按子系统切分 (arch/mm/sched/sync/fs/net/driver/credo/barrier/syscall 等); 每 crate 独立 #![deny(unsafe_code)] 边界
  - 状态: [X]
- **发现 2: 文件系统种类缺失导致容器无法落地**
  - 描述: QueenX 有 HiveFS+ramfs+devfs+procfs+sysfs+initramfs 6 种, Asterinas 有 ext2+exfat+ramfs+tmpfs+overlayfs+virtiofs+procfs+sysfs+cgroupfs+devpts+configfs+pseudofs 12 种
  - 方案: overlayfs + tmpfs 是 Docker/containerd 镜像挂载与 /dev/shm 的基础; 缺这两个, 现有 cgroup+namespace 无法支撑容器
  - 状态: [X]
- **发现 3: 现代 syscall 缺失阻碍真实应用**
  - 描述: 缺 pidfd_open/pidfd_getfd/pidfd_send_signal/signalfd/timerfd/memfd_create/name_to_handle_at/open_by_handle_at 等 8-10 个 syscall
  - 方案: systemd/runc/docker/crun 均强依赖 pidfd + memfd; 缺这些则现代 init 系统与容器运行时无法启动
  - 状态: [X]

## P0 — 结构性差距 (3 项)

### G1: Cargo workspace 组件化拆分
- **G1 背景**
  - 描述: framework 单体 107K 行, TCB 占比 64%; Asterinas 通过 48 个 workspace members 拆解, TCB 占比 19%
  - 方案: 拆分为 ~15 个独立 crate: arch/{x86_64,aarch64}/ + mm/ + sched/ + sync/ + fs/{vfs,pcache,swap}/ + net/ + driver/{pci,virtio,block}/ + credo/ + barrier/ + syscall/ + ipc/ + timer/ + idt/ + klog/ + boot/
  - 状态: [ ]
- **G1 工作量**
  - 描述: 估算工作量
  - 方案: 每个 crate 拆解 + 调整依赖图 + 修复循环依赖 + 重构 import 路径, 预计 2-3 周
  - 状态: [ ]
- **G1 验收**
  - 描述: TCB 占比 + 编译时间 + workspace 结构
  - 方案: framework 107K → 拆分后最大 crate < 15K; TCB 64% → <30%; cargo clean 后冷编译 <60s; 双架构 cargo check 0 error 0 warning
  - 状态: [ ]

### G2: overlayfs 实装
- **G2 背景**
  - 描述: overlayfs 是容器镜像的核心; Docker/containerd/podman/CRI-O 均通过 overlayfs 提供 rootfs; 缺它则 cgroup/namespace 沦为形式, 容器无法落地
  - 方案: 上层 (upperdir) + 下层 (lowerdir) + 工作层 (workdir) 三目录合并; copy_up 机制; 元数据/dentry/inode 三层结构; readdir 合并去重
  - 状态: [ ]
- **G2 实施计划**
  - 描述: 实施步骤与依赖
  - 方案: 依赖 G3 tmpfs 完成 (后者先行, 工程量小可快速出成果); 推荐顺序 G3 → G2; G3 完成后启动 G2
  - 状态: [ ]
- **G2 工作量**
  - 描述: 估算工作量
  - 方案: 参考 Linux ovl_lookup/ovl_copy_up/ovl_iterate 路径, services 层实现 ~1500 行, 包含 5-8 个 host-tests
  - 状态: [ ]
- **G2 验收**
  - 描述: overlayfs 实装完成度
  - 方案: mount -t overlay overlay -o lowerdir=A,upperdir=B,workdir=C /merged 工作; touch/read/write/rm 行为正确; 集成 docker run hello-world (远期); 双架构编译 0w0e; 6+ host-test
  - 状态: [ ]
- **G2 启动条件**
  - 描述: G2 应在 G3 完成后启动, 不宜并行
  - 方案: G3 完成 → G2 立即启动
  - 状态: [ ]

### G3: tmpfs 实装
- **G3 背景**
  - 描述: tmpfs 是基于内存的文件系统, 关键用途 /dev/shm (POSIX 共享内存)、容器临时文件系统、tmp 目录; 当前 FsType 枚举 (vfs_types.rs:137) 仅含 RamFs/HvFs/DevFs/Unknown, 无 TmpFs, mount.rs 注释声称支持 5 种含 tmpfs 是文档失实
  - 方案: 基于 ramfs 扩展, 加入 size 限制 + 统计已用/可用; 在 VMA 中预分配 page cache; 与 swap 集成 (内存压力时换出)
  - 状态: [ ]
- **G3 实施计划**
  - 描述: 立即启动, 4 步走
  - 方案: 步骤 1: FsType 枚举新增 TmpFs 变体, from_name/as_str 添加 "tmpfs" 分支; 步骤 2: services/fs/tmpfs.rs 新建 (~500 行), 实现 TmpFsData + size 限制; 步骤 3: framework/fs/vfs/api.rs vfs_mount_internal 添加 FsType::TmpFs 分支; 步骤 4: mount.rs 注释"5 种内置 FS"修正为"4 种已实装 + tmpfs 实装中"
  - 状态: [ ]
- **G3 工作量**
  - 描述: 估算工作量
  - 方案: ~500 行 services 层实装, 复用 ramfs inode/dentry 基础设施; 1 周工作量
  - 状态: [ ]
- **G3 验收**
  - 描述: tmpfs 实装完成度
  - 方案: mount -t tmpfs tmpfs /dev/shm 工作; df -h 显示容量限制; dd 大文件触发 swap 换出; 5+ host-test; FsType 枚举与 mount 注释一致
  - 状态: [ ]
- **G3 预期完成时间**
  - 描述: 启动后可完成时间
  - 方案: G3 启动后 1 周完成; G3 完成后 G2 立即启动, 总计 4 周完成 G2+G3
  - 状态: [ ]

## P1 — 功能性缺口 (4 项)

### G4: 现代 syscall 补全 (8-10 个)
- **G4 列表**
  - 描述: 缺失的关键 syscall
  - 方案: pidfd_open (SYS 434) + pidfd_getfd (SYS 438) + pidfd_send_signal (SYS 424) + signalfd4 (SYS 74/289) + timerfd_create (SYS 85/283) + timerfd_settime/gettime + memfd_create (SYS 279/319) + name_to_handle_at (SYS 303) + open_by_handle_at (SYS 304) + clone3 (SYS 435) [可选]
  - 状态: [ ]
- **G4 工作量**
  - 描述: 每个 syscall 估算
  - 方案: pidfd 套件 1 周 (需扩展 Process 数据结构 + fd 表); signalfd/timerfd 1 周 (复用 signal/posix_timer); memfd 3 天; name_to_handle 1 周; 总计 ~4 周
  - 状态: [ ]
- **G4 验收**
  - 描述: syscall 补全
  - 方案: 8-10 个 syscall 全部实装; Linux 兼容号 + QX 原生号双号; 服务层 100% safe 代理; 双架构编译 0w0e; 每 syscall ≥1 host-test
  - 状态: [ ]

### G5: RISC-V 架构支持
- **G5 背景**
  - 描述: Asterinas 支持 x86_64 + riscv64 + loongarch64; QueenX 仅 x86_64 + aarch64
  - 方案: RISC-V 64 启动 (OpenSBI) + 页表 (Sv39) + 异常 (stvec/sepc/scause) + 中断 (PLIC/CLINT) + 调度切换; 复用 framework 与 services
  - 状态: [ ]
- **G5 工作量**
  - 描述: 估算工作量
  - 方案: arch/riscv/ 全新目录 ~3000-5000 行; 启动 + 异常 + 中断 + 调度切换 4 子项; 预计 6-8 周
  - 状态: [ ]
- **G5 验收**
  - 描述: RISC-V 启动 + 运行
  - 方案: QEMU -machine virt -cpu rv64 双架构 cargo check 0w0e; hello world 用户程序启动成功; QEMU GDB 调试通过
  - 状态: [ ]

### G6: mdBook 文档体系
- **G6 背景**
  - 描述: Asterinas 有完整 book/ (内核手册 + OSTD 手册 + OSDK 手册 + RFC + to-contribute), mdBook 静态站点
  - 方案: docs/book/ 拆分 kernel-handbook + services-handbook + architecture + rfcs + contributing 5 部分; mdBook.toml 配置; CI 自动部署
  - 状态: [ ]
- **G6 工作量**
  - 描述: 估算工作量
  - 方案: 文档整理 + 章节重构 + mdBook 配置 + CI, 预计 2 周
  - 状态: [ ]
- **G6 验收**
  - 描述: mdBook 完整度
  - 方案: mdbook build 成功; 5 大章节齐全; 交叉引用完整; GitHub Pages 部署
  - 状态: [ ]

### G7: CI/CD 流水线完善
- **G7 背景**
  - 描述: QueenX 仅 .github/workflows/ci.yml 1 个; Asterinas 有 18 个 workflow (test_x86/tdx/riscv/loongarch + benchmark + xfstests + publish_docker + sctrace)
  - 方案: 拆分为 test_x86.yml + test_aarch64.yml + test_riscv.yml + benchmark.yml + xfstests.yml + lint.yml + docs.yml + docker.yml + codecov.yml 9 个
  - 状态: [ ]
- **G7 工作量**
  - 描述: 估算工作量
  - 方案: 配置 CI runner + 集成 QEMU + xfstests 集成 + codecov, 预计 1-2 周
  - 状态: [ ]
- **G7 验收**
  - 描述: CI 完整度
  - 方案: 9 个 workflow 全部 PASS; xfstests 集成分数; codecov 覆盖率上报; PR 自动 lint/test
  - 状态: [ ]

## P2 — 工程化提升 (3 项)

### G8: fsx 集成测试
- **G8 背景**
  - 描述: Asterinas 有 `test_xfstests_full.yml` 在 CI 中跑 fsx/seek-flood 等文件系统压力测试; QueenX 无
  - 方案: 引入 fsx-linux 测试程序, 集成到 host-tests/; 针对 HiveFS/ext2/overlayfs/tmpfs 各 1 轮
  - 状态: [ ]
- **G8 工作量**
  - 描述: 估算工作量
  - 方案: 移植 fsx + 适配 HiveFS 调用路径 + CI 集成, 预计 1 周
  - 状态: [ ]
- **G8 验收**
  - 描述: fsx 测试完整度
  - 方案: fsx-linux 在 host-tests 下跑通; 100 万次操作无崩溃; 数据校验通过
  - 状态: [ ]

### G9: ext2 + exfat 磁盘 FS
- **G9 验收**
  - 描述: ext2/exfat 完整度
  - 方案: mount -t ext2 /dev/sda1 /mnt 工作; mount -t exfat /dev/sdb1 /mnt 工作; 与 Linux 互操作 (ext2 读写); 双架构编译 0w0e
  - 状态: [X] (ext2 已完成)

### G10: TDX 机密计算支持
- **G10 背景**
  - 描述: Asterinas 有完整 TDX guest 支持 (tdx_guest.rs + kernel/tdx-guest/); QueenX 无
  - 方案: TDX module 检测 (CPUID 0x21) + tdcall 指令封装 + attest quote + 内存加密; 复用 Asterinas ostd/src/arch/x86/tdx_guest.rs 设计
  - 状态: [ ]
- **G10 工作量**
  - 描述: 估算工作量
  - 方案: arch/x86/tdx/ ~1500 行 + services/credo/tdx/ ~800 行 + QEMU TDX 验证, 预计 4-6 周
  - 状态: [ ]
- **G10 验收**
  - 描述: TDX guest 启动
  - 方案: QEMU -machine confidential-guest-support=tdx 启动成功; CPUID 检测通过; tdcall 包装 OK
  - 状态: [ ]

## P3 — 锦上添花 (2 项)

### G11: 容器辅助 FS 补全
- **G11 列表**
  - 描述: devpts + cgroupfs + configfs + virtiofs
  - 方案: devpts (伪终端文件系统) ~400 行; cgroupfs (cgroup 虚拟文件系统) ~600 行; configfs (内核对象配置) ~500 行; virtiofs (virtio 透传 FS) ~800 行
  - 状态: [ ]
- **G11 工作量**
  - 描述: 估算工作量
  - 方案: 总计 4 周; 与 G2/G3 同步实施最佳
  - 状态: [ ]
- **G11 验收**
  - 描述: 容器辅助 FS 完整度
  - 方案: 4 个 FS 全部实装; mount -t devpts/cgroupfs/configfs/virtiofs 工作; docker run 路径全部通
  - 状态: [ ]

### G12: systree 动态系统树
- **G12 背景**
  - 描述: Asterinas aster-systree 是动态系统树, 比传统 procfs/sysfs 更灵活, 支持运行时注册/注销节点
  - 方案: Tree + Node + Attr 三元组; 节点支持属性读写回调; 文件操作 (open/read/write/ioctl) 动态分发; 替代部分 procfs/sysfs 实现
  - 状态: [ ]
- **G12 工作量**
  - 描述: 估算工作量
  - 方案: ~2500 行 services/fs/systree.rs + 迁移 5-10 个 procfs/sysfs 节点, 预计 3-4 周
  - 状态: [ ]
- **G12 验收**
  - 描述: systree 完整度
  - 方案: aster-systree 实装; 5+ 动态节点迁移; 与 procfs/sysfs 兼容性
  - 状态: [ ]

## 实施优先级建议

### 第一阶段: P0 (4-6 周)
- **G1 Cargo workspace 组件化**
  - 描述: 拆 framework 为 ~15 个 crate
  - 方案: TCB 从 64% → <30%
  - 状态: [ ]
- **G2 overlayfs**
  - 描述: 容器镜像核心
  - 方案: ~1500 行 services 层
  - 状态: [ ]
- **G3 tmpfs**
  - 描述: /dev/shm 与容器临时 FS
  - 方案: ~500 行 services 层
  - 状态: [ ]

### 第二阶段: P1 (10-14 周)
- **G4 syscall 补全**
  - 描述: 8-10 个现代 syscall
  - 方案: pidfd + signalfd/timerfd + memfd + name_to_handle + clone3
  - 状态: [ ]
- **G5 RISC-V 架构**
  - 描述: 第三架构支持
  - 方案: arch/riscv/ 全新实装
  - 状态: [ ]
- **G6 mdBook**
  - 描述: 文档体系
  - 方案: docs/book/ 5 章节
  - 状态: [ ]
- **G7 CI/CD**
  - 描述: 9 个 workflow
  - 方案: test_x86/aarch64/riscv + benchmark + xfstests + lint + docs
  - 状态: [ ]

### 第三阶段: P2 (14-20 周)
- **G8 fsx 集成测试**
  - 描述: 文件系统压力测试
  - 方案: 移植 fsx-linux + CI 集成
  - 状态: [ ]
- **G9 ext2 + exfat**
  - 描述: 传统磁盘 FS
  - 方案: ~5500 行 services 层
  - 状态: [X] (ext2 已完成)
- **G10 TDX 机密计算**
  - 描述: 可信执行环境
  - 方案: arch/x86/tdx/ + QEMU TDX 验证
  - 状态: [ ]

### 第四阶段: P3 (7-10 周)
- **G11 容器辅助 FS**
  - 描述: devpts + cgroupfs + configfs + virtiofs
  - 方案: ~2300 行
  - 状态: [ ]
- **G12 systree**
  - 描述: 动态系统树
  - 方案: ~2500 行 services 层
  - 状态: [ ]

## 总工作量估算
- **工作量总览**
  - 描述: 12 项任务分阶段工作量
  - 方案: P0 4-6 周 + P1 10-14 周 + P2 14-20 周 + P3 7-10 周 = 总计 35-50 周 (8-12 个月)
  - 状态: [X]

## 引用
- **引用清单**
  - 描述: 评估依据
  - 方案: tmp/asterinas-main/ 源码 (Asterinas 0.18.0) + docs/plan/ 子系统规划 + docs/README.md 工程规范
  - 状态: [X]