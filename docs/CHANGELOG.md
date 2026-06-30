# Changelog

> 记录 AntX 内核工程的**面向用户/接手人**的可见变更. 内文按"## YYYY-MM-DD"分节, 时间倒序, 每节内按"新增 / 变更 / 修复 / 移除"4 类聚合. 完整 commit 索引见 `git log`.

## 2026-07-01

### 修复
- 修复 KPTI 共享页表污染导致的 Triple Fault: `unmap_page` 和 `protect_page` 添加内核高半区安全门 (PML4[256..511] 跳过), 防止清零/修改共享的 PDPT/PD 页表项
- 修复 VMM 1GB→2MB 巨页拆分丢失 HUGE_PAGE 标志: `get_or_create_table_entry` 在 PDPT→PD 拆分时保留 HUGE_PAGE 位, 避免 CPU 将 PD 项误解释为 PT 指针
- 修复 PMM bitmap 与 kmalloc 堆重叠: 添加 2MB 隔离间隙 (BITMAP_GAP_SIZE), 防止堆扩展拆分巨页时覆盖 bitmap 页表项

## 2026-06-26

### 新增
- 新增 `tests/integration/run_driver2_qemu_vga_test.py` — DRIVER-2 QEMU virtio-vga 真机增强验证脚本 (commit 6139769)
- 新增 `tests/integration/run_driver1_qemu_xhci_test.py` — DRIVER-1 QEMU qemu-xhci 真机验证脚本 (commit 00761ee)
- 新增 `tests/integration/run_driver2_qemu_vga_basic_test.py` — DRIVER-2 QEMU virtio-vga 双层验证脚本 (commit 6f76fcf)
- 新增 `tests/integration/run_reval6_epoll_qemu_test.py` — REVAL-6.3 QEMU epoll 三层验证脚本 (commit c0c7e6f)
- 新增 `tests/integration/run_legacy4_block_qemu_test.py` — LEGACY-4.4 QEMU virtio-blk 双层验证脚本 (commit bee6a44)
- 新增 `host-tests/tests/test_runner_init_test.rs` — test_runner_init 缺 init_global 修复防回归 (commit 4df24a5)

### 变更
- 变更文档写作规则为"标题 + 章节 + 条目(描述+方案+状态)"; 新增详情字段; 支持一文档多工程计划 (commits 88ce6ce, df8b882, eb334d0)
- 同步 `docs/plan/maintenance-cycle-2026-06-19.md` 正文章节状态, 0 处 [ ] 残留, 维护周期 100% 收口 (commit 36f0fb0)
- 切换构建架构为 x86_64 (commit ca8f4e0)
- 同步 §9.1 文档状态至 2026-06-25 真实状态 (commit cd9e1f1)
- 更新 2026Q2 维护周期任务状态 (commit b38d83e)

### 修复
- 修复 test_runner_init 缺 init_global 导致的 DevFS::mount panic, 解锁 127 个 FS/epoll 测试 (commit 4df24a5)
- 修复 smoltcp W5 transmute 移除遗漏, smoltcp 100% 收口 (commit 2be10ed)

## 2026-06-25

### 新增
- 完成 smoltcp Framekernel 包装 W7-E: DHCP 内部状态追踪 + dhcp_decide_at 集成 (commit ace1ad8)
- 完成 smoltcp REVAL-W W4.4+W4.5+W5+W6: trait 线协议翻译 + transmute 移除 + DHCP 策略抽象 (commit 5d60a4e)
- 完成 smoltcp REVAL-W W4.2.3.5: 启用 next_smol_idx 严格分配 + 验证 (commit 2101364)
- 完成 smoltcp REVAL-W W4.2.3.4 步骤 2+3: SmoltcpNetStack::socket_open 实际化 (commits 9a74582, 13d703f)
- 完成 smoltcp REVAL-W W4.2.3.3: sm_socket 路径迁移到 socket_open_stub (commit 737e213)
- 完成 smoltcp REVAL-W W4.2.3.2: socket_open_stub Tcp/Udp 实装 (commit 1599646)
- 完成 smoltcp REVAL-W W4.2.3.1: 数组大小扩展 [T; MAX_SM_FD] → [T; TOTAL_SLOTS] (commit 36d1ecd)
- 完成 smoltcp REVAL-W W4.2.2: 桥接实装 socket_close + dhcp_state 翻译 (commit a5f4ff7)
- 完成 smoltcp REVAL-W W4.2: 桥接 raw helpers 阶段 1 (commit ec26c0f)
- 完成 smoltcp REVAL-W W4.1: framework/net/init.rs 集成 SmoltcpNetStack 实例 (commit 999d550)
- 完成 smoltcp Framekernel 包装 W3.2: SmoltcpNetStack trait 骨架 (commit 560bd82)
- 完成 smoltcp 0.13.1 升级与 REVAL-W W1-W2 落地 (commit 0b10913)
- 完成 USB 第 4 组工程, 实现 xHCI 设备层全功能 (commit 63cf80b)
- 完成 USB-1.1/1.2/1.3/1.4 核心功能实装 (commit a8f5d23)
- 完成 P0-2/P0-3 等多阶段 HDMI/DP 驱动优化 (commit e708420)
- 完成 DISPLAY-2.3c HDMI 同步极性和 TMDS 输出使能实现 (commit fe5dedf)
- 完成 DISPLAY-2.3b HDMI 时序参数配置实装 (commit 8efb4f6)
- 完成 HDMI pixel clock configuration phase 1 (commit 0a6e862)
- 实现 DP HPD 真实读取 + 修复遗留 API 标记与测试 (commit 3c4ed99)
- 实现 HDMI HPD 真实读取 (commit 8988266)
- 完成 LEGACY-5 最后一批 ZIL trait 化实现 (commit 847c73a)
- 完成 LEGACY-5 最后两子系统 RAID-Z 和 ARC 的 trait 抽象 (commit b028f98)
- 完成 LEGACY-5 剩余 DMU+SPA trait 抽象 (commit 4d2ff07)
- 实现 ZAP 和 TXG 模块的 trait 抽象 (commit 0c2013d)
- 拆分 epoll 轮询为机制与策略层 (commit d5c4910)
- 补齐 4 个策略模块的单元测试, 覆盖 PMM/Slab/Swap/Sched (commit e7d09cd)
- 完成 LEGACY-4 BlockOps thunk 移除 (commit 9d9ce6e)
- 完成 virtio-blk 中断驱动 IO 与多实例支持 (commit d5c2093)

### 变更
- 迁移 smoltcp 从 framework/ 到 services/ (决策 3-B) (commit 627a8c1)
- 批量更新中文注释与文档字符串 (commit 2e365e5)
- 跟随 smoltcp 路径迁移更新路径常量 (commit 63120e1)
- 更新 2026-06-22 维护周期文档 (commit 31e0193)
- 更新 DISPLAY-2.4 任务状态 (commit 3c4ed99)
- 更新 TODO 跟踪号并完成部分计划项 (commit 079b8b8)

## 2026-06-24

### 新增
- 完成 T1-2 信号投递策略从 framework 提取到 services (commit 99f2dd4)
- 完成 IPC 策略从 framework 到 services 的迁移 (T6-1) (commit 4546b33)
- 完成 T2-2/T2-3/T2-4 策略提取与 T5-1 系统调用分发迁移 (commit 8fe3d07)
- 完成 L-01~L-03 中间层架构落地, 迁移 syscall 到 services 层 (commit 49745b5)
- 完成 T-01~T-05 策略-机制分离抽象落地 (commit d5443bc)
- 完成 proc/fd_alloc 与 net 子系统的架构迁移 (commit e55d31a)
- 完成 2026-06-19 维护周期的代码清理与常量统一 (commit 5bafff7)
- 统一替换魔术数字与重复定义, 标准化页大小等常量 (commit ae317dd)
- 完成内核子系统解耦与架构重构 (commit e26ad74)
- 完成 T6 系列代码模块迁移与重构 (commit ff40d12)

### 变更
- 评估 T1-2 信号投递策略/T2-1 VMA 策略, 更新缩减计划 (commit bd9df3f)
- 完成框架层跨子系统访问治理与代码清理 (commit 7ec557a)
- 整理模块导入路径, 统一 API 访问方式 (commit 0c57efc)
- 批量清理内部路径引用, 统一模块导出 (commit f1c1afc)
- 全面清理跨模块直接内部访问, 统一使用公共 API (commit 0f221ff)
- 完成阶段 3.5 内部访问违规治理, 清理跨模块引用 (commit 6ce587e)
- 统一重构模块导入与公共 API 导出 (commit 3cb5d26)

## 2026-06-23

### 新增
- 完成 Phase D TRACK-XXX Backlog 批量同步, 47 条修改 (commit 38f34a3)
- 完成 Phase D 两行冗余占位关闭, 指向 D2/D3 详细行 (commit ba3aee6)
- 关闭 TD-22 三处违规回归, 新增 is_migration_note 豁免 + 续行状态机 (commit 5fb3da1)
- 大规模迁移系统调用策略到 services 层 (commit 5df66aa)

## 2026-06-13

### 新增
- 新增工程纪律性规范文档, 约束后续新代码 (commit 56df7b7)
- 跨日换机工作交接文档 (commit b6cc9ea)

## 2026-06-12

### 新增
- 完成全项目注释中文化 + CI 审计规则升级 (commit ae3dbff)
- 修复多项内核子系统缺陷并优化日志与 execve 逻辑 (commit 9280ab1)

### 变更
- 调整默认构建架构为 x86_64, 并清理冗余 allow(dead_code) (commit f159ea2)

## 2026-06-11

### 新增
- 新增 2026-06-19 维护计划 (commit 14e92fc)
- 归档旧工程文档并更新进度表 (commit 346e567)
- 清理大量 TODO 标记并实现多项核心功能 (commit 781bdfd)

## 2026-06-08

### 新增
- C3 Unix Domain Socket 设计决策 (commit 关联 DECISION-006)
- Priority Inheritance Mutex 设计决策 (commit 关联 DECISION-009/010/011)
- queenx 命名与生态兼容立场书 (2026-06-08 多轮讨论沉淀)

## 2026-06-04 及之前

### 初始化
- 启动 QueenX 内核工程, 定义内核/用户态/工具链基础

### 关键 commit 索引

| 日期 | commit | 主题 |
|------|--------|------|
| 2026-06-26 | 6139769 | DRIVER-2 真机增强验证 (display_init + framebuffer self_test) |
| 2026-06-26 | 00761ee | DRIVER-1 QEMU xHCI 双层验证 (USB 真机 100% 收口) |
| 2026-06-26 | 4df24a5 | test_runner_init 缺 init_global — DevFS::mount panic 修复 |
| 2026-06-26 | c0c7e6f | REVAL-6.3 QEMU epoll 三层验证 (VfsPollPolicy 收口) |
| 2026-06-26 | bee6a44 | LEGACY-4.4 QEMU virtio-blk 双层验证 (BlockOps thunk 收口) |
| 2026-06-25 | ace1ad8 | W7-E DHCP 内部状态追踪 + dhcp_decide_at 集成 (REVAL-W) |
| 2026-06-25 | 5d60a4e | W4.4+W4.5+W5+W6 - trait 线协议翻译 + transmute 移除 + DHCP 策略抽象 |
| 2026-06-25 | 63cf80b | 完成 USB 第 4 组工程, 实现 xHCI 设备层全功能 |
| 2026-06-25 | 847c73a | LEGACY-5 最后一批 ZIL trait 化实现 |
| 2026-06-25 | 9d9ce6e | LEGACY-4 BlockOps thunk 移除 |
| 2026-06-24 | 99f2dd4 | T1-2 信号投递策略从 framework 提取到 services |
| 2026-06-24 | 4546b33 | T6-1 IPC 策略从 framework 到 services 迁移 |
| 2026-06-24 | e55d31a | 完成 proc/fd_alloc 与 net 子系统的架构迁移 |
| 2026-06-24 | e26ad74 | 完成内核子系统解耦与架构重构 |

### 决策记录 (DECISION-NNN)

- DECISION-006 — UDS 设计决策 (2026-06-08)
- DECISION-009/010/011 — PI Mutex 设计决策 (2026-06-08)
- 决策 3-B — smoltcp 物理位置选择: services/net/ 而非 framework/ (2026-06-25)
