# QueenX 期望目标 (Hope and Utopia)

> 截至 2026-06-28, 这是 QueenX 项目的期愿描述文档, 非任务规划, 仅记录长远期望与战略愿景.

## 核心定位

QueenX 是一个从零开发的自研操作系统内核, 采用 framekernel 架构, 以蚁后-蚁巢-工蚁的命名体系 (QueenX 内核 / HiveFS 文件系统 / AntX 发行版) 体现项目的世界观.

项目的核心定位是 **专注内核深度**, 不在用户态层与既有生态 (Linux/macOS/BSD) 正面竞争.

## 期望目标

短期目标是内核功能完整度达到生产可用, 完成 Phase A-D 路线图, 即从可启动用户态 → 可运行真实程序 → 高性能网络服务 → 企业级 (容器/安全/观测).

中期目标是双架构 (x86_64 + aarch64) + RISC-V 支持, 同时实现 Linux 工具链兼容、POSIX 兼容、musl libc 静态二进制. 通过 linuxulator 提供 Linux ABI 兼容层, musl 静态二进制无须修改即可运行, elfld.so 是自研动态加载器.

长期目标是与 OpenHarmony 的上层用户态空间相结合, 而**不是自建完整的 GNU/Linux 用户态**. OpenHarmony 提供完整的上层生态 (应用框架、HDF 驱动框架、HUKS 安全子系统等), QueenX 仅需提供兼容的内核 ABI. 这样 QueenX 可以避开用户态开发的巨大工作量, 专心内核; 同时获得生态环境的曝光度 (OpenHarmony 设备生态).

最终目标是 **蚁群世界观的完整呈现**: QueenX (蚁后内核) + HiveFS (蚁巢文件系统) + AntX (工蚁发行版) + OpenHarmony (外部生态) 四方协同. 形成一个完整的自研操作系统栈, 既保持技术独立性又获得生态入口. 这是 QueenX 与 Asterinas/Redox/Hurd 等纯技术性内核项目的根本区别.

## 战略选择

**不开发完整用户态** — 仅开发部分用户态程序作为内核功能验证. 用户态程序仅作为内核 e2e 测试用例, 不追求通用性; 不开发 shell、包管理器、桌面环境等 GNU/Linux 必备组件.

**借力 OpenHarmony 生态** — 不自建 GNU 兼容层, 而是适配 OpenHarmony 用户态空间. 相比自己拼一套完整的用户态, OpenHarmony 提供的整套上层空间更加务实. POSIX 接口的实现支持运行 GNU 软件是基础但非目标, 真正目标是与 OpenHarmony 用户态无缝对接.

**专注内核深度** — 将工程资源集中在内核子系统 (调度/同步/文件系统/网络/安全/虚拟化) 上. 这是 QueenX 与绝大多数 OS 项目 (都试图从内核到用户态完整自建) 的差异点; 通过生态对接而非自建完整用户态, 实现 5-10 倍的工程效率提升.

## 哲学依据

**务实复用 (ref-naming.md §4.2)** — 不重复造轮子, 借用既有生态. smoltcp 借上游, OpenHarmony 用户态借上游, 仅自研核心 TCB 与差异化部分.

**蚁群世界观** — QueenX (蚁后) 统治 HiveFS (蚁巢), AntX (工蚁) 分发, OpenHarmony 是外部生态. 生态系统中每个角色都有明确分工, 内核不必全包全揽.

**长期主义** — 5-10 年视角的项目演进. 不追求短期功能完整度, 而是构建可演进的基础架构 (framekernel 组件化 + 栏栈故障恢复 + 策略 trait 化).

## 实施路径

当前 (2026-06-28): Phase A-D 全部完成, 进入 Asterinas 差距补齐阶段 (12 项任务, 35-50 周工作量). 优先 P0: Cargo workspace 组件化 + overlayfs + tmpfs.

2026 H2: 完成 P1 (syscall 补全 + RISC-V + mdBook + CI/CD), 内核完整度达到 Asterinas 0.18 同等水平.

2027 H1: 完成 P2 (fsx 测试 + ext2/exfat + TDX), 内核达到企业级生产可用.

2027 H2: 启动 OpenHarmony 用户态适配工作, 在 QueenX 内核之上运行 OpenHarmony liteos_a/liteos_m 上层, 验证 ABI 兼容性.

2028+: 形成完整的 QueenX + OpenHarmony 操作系统栈, 通过 OpenHarmony 生态获得实际应用场景, QueenX 内核作为差异化技术核心.

## 与其他项目的关系

**对比 Asterinas** — Asterinas 的体系是 **Asterinas 内核 + NixOS 上层用户态 + Asterinas NixOS 发行版** 三位一体 (内核源码注释中曾使用 "Aster-nix" 作为别名, 但项目正式名称统一为 Asterinas). 其核心动机与 QueenX 高度一致: 避开从零自建用户态的庞杂工作量, 借力既有上层生态 (NixOS 12 万 + 软件包). Asterinas 选 NixOS 是看重其 Nix 配置模型 + 可复现性 + 包覆盖广度; QueenX 选 OpenHarmony 是看重其在 IoT/嵌入式/移动设备领域的生态入口与设备覆盖. 两者都是"专注内核深度 + 借力上层生态"的务实路径, 区别仅在上层生态的取舍.

**对比 Linux** — Linux 是从内核到用户态的完整自建, QueenX 不复制这条路. 借 OpenHarmony 之力, 避免重复造轮子.

**对比鸿蒙/HarmonyOS** — OpenHarmony 上层空间 + LiteOS 内核, QueenX 替换 LiteOS 内核. QueenX 内核提供更强的 framekernel 安全模型 + HiveFS 持久化 + 栏栈故障恢复.

## 风险与缓解

**风险 1: OpenHarmony 演进路径变化** — OpenHarmony 仍在快速演进, 其 ABI/接口可能变化. 缓解: 保持与 OpenHarmony LTS 版本对齐, 每 6 个月重新评估.

## 项目姿态愿景

QX 若成功与 OpenHarmony 上层结合, 其项目姿态应**先将重心放在嵌入式与 IoT 场景**, 而**以通用操作系统内核的姿态面向微计算场景**. 具体含义:

- **场景重心**: 嵌入式设备 / IoT 终端 / 工业控制 / 智能家居 / 边缘计算节点. 这些场景计算资源有限但对实时性、可靠性、安全性要求高, 与 QueenX framekernel 的安全模型 + HiveFS 持久化 + 栏栈故障恢复高度契合.
- **姿态定位**: 通用操作系统内核, 而非嵌入式专用 RTOS. 通用内核姿态意味着保留完整的 POSIX/Linux ABI 兼容能力、虚拟内存、进程隔离、文件系统等所有通用能力, 仅在资源受限时裁剪或优化; 不因嵌入式场景而牺牲内核的通用性.
- **微计算聚焦**: 微计算 (micro-computing) 是嵌入式与 IoT 的本质特征 — 算力低、内存小 (MB 级)、存储小 (MB-GB 级)、网络间歇性强. QueenX 内核应针对这些约束做轻量化优化 (小内存 footprint、低功耗 idle、低带宽容忍), 但不放弃通用内核能力.
- **与 OpenHarmony 的契合**: OpenHarmony 本身面向的就是 IoT 与嵌入式设备, 其 HDF 驱动框架、HUKS 安全子系统、LiteOS 内核都已针对微计算优化. QueenX 替换 LiteOS 内核后, 可同时获得 framekernel 安全模型与 OpenHarmony 嵌入式生态.

这一姿态与 Asterinas NixOS 形成互补: Asterinas 走桌面/服务器/容器 (大计算资源), QueenX 走嵌入式/IoT/边缘 (小计算资源); 两个项目共同验证"通用内核 + 借力上层生态"路径在不同算力规模下的可行性.

**风险 2: 内核 ABI 差异 (DECISION-037 已收敛)** — 2026-08-03 决策: 0-299 段直接使用 Linux 标准 syscall 编号, 500+ 段作为 QueenX 自由扩展与 Linux 错开. OpenHarmony 用户态若需 Linux 编号兼容, 走用户态侧适配层 (类似 musl 静态二进制模式); QueenX 内核侧不再提供 syscall 翻译层.

**风险 3: 生态冷启动** — 即便与 OpenHarmony 结合, QueenX 仍是新进入者. 缓解: 以 OpenHarmony 设备为应用场景 (IoT/嵌入式), 避开与 Linux 直接竞争的服务器/桌面领域.

## 引用

- queenx-ref-naming.md (务实复用原则)
- kernel-roadmap.md / archive/subsystem-bootstrap-sequence-2026-06.md (Phase A-D 路线图)
- asterinas-gap-analysis.md (Asterinas 差距补齐)