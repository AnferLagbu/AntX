# QueenX 内核工程路线图

> 基于 2026-07-07 代码现状，识别 QueenX 相对 Asterinas 的结构性差距，作为工程路线图。

## 总体进度

- **总体进度**
  - 描述: 内核功能完整度与工程化水平
  - 方案: 12 项工程任务，已完成 2 项 (G9 ext2+exfat, G12 systree 部分)，完成率 16.7%
  - 状态: []

---

## P0 — 容器落地关键路径 (3 项)

### G1: Cargo workspace 组件化拆分
- **描述**
  - framework 单体 107K 行，TCB 占比 64%；Asterinas 通过 48 个 workspace members 拆解，TCB 占比 19%
  - 方案: 拆分为 ~15 个独立 crate: arch/mm/sched/sync/fs/net/driver/credo/barrier/syscall/ipc/timer/idt/klog/boot
  - 状态: []

- **工作量**
  - 描述: 每个 crate 拆解 + 调整依赖图 + 修复循环依赖 + 重构 import 路径
  - 方案: 预计 2-3 周；分阶段: Phase 1 低风险 (timer/idt/klog/boot) → Phase 2 中风险 (sync/credo/barrier) → Phase 3 高风险 (mm/proc/fs/net)
  - 状态: []

- **验收**
  - 描述: TCB 占比 + 编译时间 + workspace 结构
  - 方案: framework 107K → 拆分后最大 crate < 15K; TCB 64% → <30%; cargo clean 后冷编译 <60s; 双架构 cargo check 0 error 0 warning
  - 状态: []

### G2: overlayfs 实装
- **描述**
  - 容器镜像核心，Docker/containerd/podman/CRI-O 均通过 overlayfs 提供 rootfs
  - 方案: 上层 (upperdir) + 下层 (lowerdir) + 工作层 (workdir) 三目录合并；copy_up 机制；readdir 合并去重
  - 依赖: G3 tmpfs 完成后启动
  - 状态: []

- **工作量**
  - 描述: 参考 Linux ovl_lookup/ovl_copy_up/ovl_iterate 路径
  - 方案: services 层实现 ~1500 行，包含 5-8 个 host-tests；预计 2-3 周
  - 状态: []

- **验收**
  - 描述: overlayfs 功能完整度
  - 方案: mount -t overlay overlay -o lowerdir=A,upperdir=B,workdir=C /merged 工作；touch/read/write/rm 行为正确；双架构编译 0w0e；6+ host-test
  - 状态: []

### G3: tmpfs 实装 ✅
- **描述**
  - 基于内存的文件系统，关键用途 /dev/shm (POSIX 共享内存)、容器临时文件系统、tmp 目录
  - 方案: 基于 ramfs 扩展，加入 size 限制 + 统计已用/可用；FsType 枚举新增 TmpFs 变体
  - 状态: [X]

- **工作量**
  - 描述: services/fs/tmpfs.rs 新建
  - 方案: ~500 行 services 层实装，复用 ramfs inode/dentry 基础设施；1 周工作量
  - 状态: [X]

- **验收**
  - 描述: tmpfs 功能完整度
  - 方案: mount -t tmpfs tmpfs /dev/shm 工作；df -h 显示容量限制；5+ host-test；FsType 枚举与 mount 注释一致
  - 状态: [X]

---

## P1 — 功能性缺口 (4 项)

### G4: 现代 syscall 补全 (8-10 个)
- **描述**
  - 缺失的关键 syscall: pidfd_open/pidfd_getfd/pidfd_send_signal/signalfd4/timerfd_create/timerfd_settime/memfd_create/name_to_handle_at/open_by_handle_at
  - 方案: pidfd 套件 1 周 + signalfd/timerfd 1 周 + memfd 3 天 + name_to_handle 1 周
  - 状态: []

- **验收**
  - 描述: syscall 补全完整度
  - 方案: 8-10 个 syscall 全部实装；Linux 兼容号 + QX 原生号双号；服务层 100% safe 代理；每 syscall ≥1 host-test
  - 状态: []

### G5: RISC-V 架构支持
- **描述**
  - Asterinas 支持 x86_64 + riscv64 + loongarch64；QueenX 仅 x86_64 + aarch64
  - 方案: RISC-V 64 启动 (OpenSBI) + 页表 (Sv39) + 异常 (stvec/sepc/scause) + 中断 (PLIC/CLINT) + 调度切换
  - 状态: []

- **工作量**
  - 描述: arch/riscv/ 全新目录
  - 方案: ~3000-5000 行；启动 + 异常 + 中断 + 调度切换 4 子项；预计 6-8 周
  - 状态: []

### G6: mdBook 文档体系
- **描述**
  - Asterinas 有完整 book/ (内核手册 + OSTD 手册 + OSDK 手册 + RFC + to-contribute)
  - 方案: docs/book/ 拆分 kernel-handbook + services-handbook + architecture + rfcs + contributing 5 部分
  - 状态: []

- **工作量**
  - 描述: 文档整理 + 章节重构 + mdBook 配置 + CI
  - 方案: 预计 2 周
  - 状态: []

### G7: CI/CD 流水线完善
- **描述**
  - QueenX 仅 .github/workflows/ci.yml 1 个；Asterinas 有 18 个 workflow
  - 方案: 拆分为 ci-x86.yml + ci-aarch64.yml + ci-lint.yml + ci-bench.yml 4 个
  - 状态: []

- **工作量**
  - 描述: 配置 CI runner + 集成 QEMU + xfstests 集成 + codecov
  - 方案: 预计 1-2 周
  - 状态: []

---

## P2 — 工程化提升 (3 项)

### G8: fsx 集成测试
- **描述**
  - Asterinas 有 test_xfstests_full.yml 在 CI 中跑 fsx/seek-flood 等文件系统压力测试
  - 方案: 引入 fsx-linux 测试程序，集成到 host-tests/；针对 ext2/exfat/overlayfs/tmpfs 各 1 轮
  - 状态: []

- **验收**
  - 描述: fsx 测试完整度
  - 方案: fsx-linux 在 host-tests 下跑通；100 万次操作无崩溃；数据校验通过
  - 状态: []

### G9: ext2 + exfat 磁盘 FS ✅
- **描述**
  - ext2 是 Linux 互操作基础，exfat 是大容量 U 盘/SD 卡标准
  - 方案: ext2 ~2500 行 + exfat ~3000 行；复用 VFS 与 page cache
  - 状态: [X]

- **验收**
  - 描述: ext2/exfat 完整度
  - 方案: mount -t ext2 /dev/sda1 /mnt 工作；mount -t exfat /dev/sdb1 /mnt 工作；与 Linux 互操作；双架构编译 0w0e
  - 状态: [X]

### G10: TDX 机密计算支持
- **描述**
  - Asterinas 有完整 TDX guest 支持；QueenX 无
  - 方案: TDX module 检测 (CPUID 0x21) + tdcall 指令封装 + attest quote + 内存加密
  - 状态: []

- **工作量**
  - 描述: arch/x86/tdx/ ~1500 行 + services/credo/tdx/ ~800 行 + QEMU TDX 验证
  - 方案: 预计 4-6 周
  - 状态: []

---

## P3 — 锦上添花 (2 项)

### G11: 容器辅助 FS 补全
- **描述**
  - devpts + cgroupfs + configfs + virtiofs
  - 方案: devpts ~400 行 + cgroupfs ~600 行 + configfs ~500 行 + virtiofs ~800 行
  - 状态: []

- **工作量**
  - 描述: 总计 4 周
  - 方案: 与 G2/G3 同步实施最佳
  - 状态: []

### G12: systree 动态系统树 (部分完成)
- **描述**
  - Asterinas aster-systree 是动态系统树，比传统 procfs/sysfs 更灵活
  - 方案: Tree + Node + Attr 三元组；节点支持属性读写回调；替代部分 procfs/sysfs 实现
  - 状态: []

- **工作量**
  - 描述: ~2500 行 services/fs/systree.rs + 迁移 5-10 个 procfs/sysfs 节点
  - 方案: 预计 3-4 周
  - 状态: []

---

## 实施优先级路线图

```text
Week 1-2:   G3 tmpfs (1w) + G7 CI 拆分 (1w)           ← 并行
Week 3-5:   G2 overlayfs (3w)                          ← 依赖 G3
Week 4-7:   G4 syscall pidfd+memfd (2w) + signalfd/timerfd (1w) + name_to_handle (1w)
Week 6-8:   G1 workspace Phase 1-2 (3w)                ← 低/中风险模块
Week 9-14:  G9 ext2 (6w)                               ← 已完成
Week 12-14: G6 mdBook (2w)                             ← 可与 G9 并行
Week 15-16: G8 fsx (1w) + G11 容器 FS (4w 并行部分)
Week 17-20: G1 workspace Phase 3 (4w)                  ← 高风险模块
Week 21+:   G5 RISC-V / G10 TDX / G12 systree         ← 远期
```

## 工作量汇总

| 优先级 | 任务 | 状态 | 预计工期 |
|--------|------|------|----------|
| P0 | G1 + G2 + G3 | 0% | 8-10 周 |
| P1 | G4 + G5 + G6 + G7 | 0% | 12-15 周 |
| P2 | G8 + G9 + G10 | 33% | 6-10 周 |
| P3 | G11 + G12 | 10% | 7-10 周 |
| **总计** | | **16.7%** | **33-45 周** |

## 引用
- **引用清单**
  - 描述: 评估依据
  - 方案: tmp/asterinas-main/ 源码 (Asterinas 0.18.0) + docs/plan/ 子系统规划 + docs/README.md 工程规范
  - 状态: [X]