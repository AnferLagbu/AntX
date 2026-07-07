# Gap Analysis 优先级重排与实施方案

> 基于 2026-07-04 代码现状,对 `asterinas-gap-analysis.md` 12 项任务重新排序.
> 原文按 Asterinas 对标维度分类 (P0/P1/P2/P3),本文按**实施价值 × 依赖关系 × ROI** 重排.

## 当前代码基础

| 指标 | 值 |
|------|-----|
| framework LoC | 107K |
| services LoC (excl. smoltcp) | 53K |
| 已有文件系统 | ramfs / hvfs / devfs / procfs / initramfs (5 种) |
| 已有 syscall | 187 个 QX_* 编号 |
| 已有架构 | x86_64 + aarch64 |

---

## 优先级排序 (从高到低)

### Tier 1 — 最高优先 (容器落地关键路径)

#### 1. G3: tmpfs 实装 ⭐ 推荐首个实施

| 维度 | 评估 |
|------|------|
| 价值 | /dev/shm 是 POSIX 共享内存基础, 容器临时文件系统必需 |
| 工作量 | ~500 行 services 层, 1 周 |
| 风险 | 低 — 复用 ramfs inode/dentry 基础设施 |
| 依赖 | 无 |
| 建议 | **立即启动**. 在 `services/fs/` 新建 `tmpfs.rs`, FsType 枚举新增 TmpFs 变体, VFS mount 添加分支 |

**实施步骤**:
1. `services/fs/vfs_types.rs`: FsType 枚举新增 `TmpFs`
2. `services/fs/tmpfs.rs`: 新建, 实现 TmpFsData + size 限制 + 统计
3. `framework/fs/vfs/api.rs`: vfs_mount_internal 添加 FsType::TmpFs 分支
4. `host-tests/`: 5+ 测试 (mount/size limit/read/write/swap interaction)

#### 2. G2: overlayfs 实装

| 维度 | 评估 |
|------|------|
| 价值 | 容器镜像核心, Docker/containerd 必需 |
| 工作量 | ~1500 行 services 层, 2-3 周 |
| 风险 | 中 — copy_up 机制较复杂 |
| 依赖 | **G3 tmpfs 完成后启动** |
| 建议 | G3 完成后立即启动. 上层 (upperdir) + 下层 (lowerdir) + 工作层 (workdir) 三目录合并 |

**实施步骤**:
1. `services/fs/overlayfs.rs`: 新建, 实现 OvFsData + inode/dentry 层
2. copy_up 机制: 写时复制 lower 到 upper
3. readdir 合并去重: lower + upper 目录合并
4. `host-tests/`: 6+ 测试 (mount/copy_up/read/write/rm/readdir)

#### 3. G4: 现代 syscall 补全

| 维度 | 评估 |
|------|------|
| 价值 | systemd/runc/docker 强依赖 pidfd + memfd |
| 工作量 | 8-10 个 syscall, ~4 周 |
| 风险 | 中 — 需扩展 Process 数据结构 |
| 依赖 | 无 (可与 G2/G3 并行) |
| 建议 | 优先实现 pidfd 套件 + memfd, signalfd/timerfd 复用已有 signal/posix_timer |

**推荐实施顺序**:
1. **pidfd_open/pidfd_getfd/pidfd_send_signal** (1 周) — 扩展 fd 表 + Process 结构
2. **memfd_create** (3 天) — 基于 tmpfs (依赖 G3)
3. **signalfd4/timerfd_create** (1 周) — 复用 signal/posix_timer
4. **name_to_handle_at/open_by_handle_at** (1 周) — 文件句柄 API

---

### Tier 2 — 高优先 (架构与工程基础)

#### 4. G1: Cargo workspace 组件化拆分

| 维度 | 评估 |
|------|------|
| 价值 | TCB 从 64% 降至 <30%, 编译时间优化 |
| 工作量 | ~15 个 crate 拆分, 2-3 周 |
| 风险 | **高** — 循环依赖修复 + import 路径重构 |
| 依赖 | 无, 但建议在 G2/G3 之后 (避免与 FS 开发冲突) |
| 建议 | 分阶段拆分: 先拆独立模块 (timer/idt/klog/boot), 再拆核心模块 (mm/sync/proc) |

**分阶段策略**:
- Phase 1 (低风险): timer / idt / klog / boot / debug / config → 6 个独立 crate
- Phase 2 (中风险): sync / credo / barrier / ipc → 4 个 crate
- Phase 3 (高风险): mm / proc / fs / net / driver / syscall → 6 个 crate
- 每 Phase 完成后双架构编译验证

#### 5. G7: CI/CD 流水线完善

| 维度 | 评估 |
|------|------|
| 价值 | 自动化测试 + 多架构验证 + 覆盖率 |
| 工作量 | 1-2 周 |
| 风险 | 低 |
| 依赖 | 无 (可立即启动) |
| 建议 | 拆分为独立 workflow: test_x86 / test_aarch64 / lint / benchmark |

**推荐 workflow 拆分**:
1. `ci-x86.yml` — cargo check + clippy + 审计 + host-tests
2. `ci-aarch64.yml` — cargo check + QEMU 集成测试
3. `ci-lint.yml` — 注释语言 + 边界 + 安全覆盖审计
4. `ci-bench.yml` — 性能基线回归检测

---

### Tier 3 — 中优先 (功能扩展)

#### 6. G9: ext2 + exfat 磁盘 FS ✅ ext2 已完成

| 维度 | 评估 |
|------|------|
| 价值 | Linux 互操作基础, 磁盘存储必需 |
| 工作量 | ext2 ~2500 行 + exfat ~3000 行, 8-12 周 |
| 风险 | 中 — ext2 设计复杂 (块组/inode 位图/目录项) |
| 依赖 | 无, 但建议在 G2/G3 之后 |
| 建议 | 先实现 ext2 (Linux 互操作核心), exfat 可延后 |

**ext2 实施要点**:
- 块组描述符 + inode 位图 + 数据位图
- 目录项 (dirent) + inode 读写
- 复用 VFS page cache 做缓冲
- 与 Linux ext2 互操作验证 (mount/umount/cross-read)

#### 7. G6: mdBook 文档体系

| 维度 | 评估 |
|------|------|
| 价值 | 项目文档化 + 社区贡献基础 |
| 工作量 | 2 周 |
| 风险 | 低 |
| 依赖 | 无 (可立即启动) |
| 建议 | 整合现有 docs/explain/ + docs/plan/ 到 mdBook 结构 |

---

### Tier 4 — 低优先 (锦上添花)

#### 8. G8: fsx 集成测试

| 维度 | 评估 |
|------|------|
| 价值 | 文件系统压力测试 |
| 工作量 | 1 周 |
| 风险 | 低 |
| 依赖 | 建议在 G9 ext2 完成后 (有更多 FS 可测试) |

#### 9. G11: 容器辅助 FS 补全

| 维度 | 评估 |
|------|------|
| 价值 | devpts/cgroupfs/configfs/virtiofs 补全容器路径 |
| 工作量 | ~2300 行, 4 周 |
| 风险 | 低 |
| 依赖 | 建议与 G2/G3 同步实施 |

#### 10. G12: systree 动态系统树

| 维度 | 评估 |
|------|------|
| 价值 | 替代部分 procfs/sysfs, 更灵活 |
| 工作量 | ~2500 行, 3-4 周 |
| 风险 | 低 |
| 依赖 | 无, 但优先级低 (现有 procfs/sysfs 可用) |

---

### Tier 5 — 远期 (高成本/低紧迫)

#### 11. G5: RISC-V 架构支持

| 维度 | 评估 |
|------|------|
| 价值 | 第三架构, 生态扩展 |
| 工作量 | ~3000-5000 行, 6-8 周 |
| 风险 | **高** — 全新架构, 需深入 RISC-V 特权规范 |
| 依赖 | 建议在 x86_64+aarch64 稳定后 |
| 建议 | 参考 aarch64 移植路径, 复用 framework/services 抽象层 |

#### 12. G10: TDX 机密计算支持

| 维度 | 评估 |
|------|------|
| 价值 | 可信执行环境, 安全计算 |
| 工作量 | ~2300 行, 4-6 周 |
| 风险 | 高 — 硬件依赖 + QEMU TDX 验证环境 |
| 依赖 | 建议在架构稳定后 |
| 建议 | 优先实现 CPUID 检测 + tdcall 封装, attestation 延后 |

---

## 推荐实施路线图

```text
Week 1-2:   G3 tmpfs (1w) + G7 CI 拆分 (1w)           ← 并行
Week 3-5:   G2 overlayfs (3w)                          ← 依赖 G3
Week 4-7:   G4 syscall pidfd+memfd (2w) + signalfd/timerfd (1w) + name_to_handle (1w)
Week 6-8:   G1 workspace Phase 1-2 (3w)                ← 低/中风险模块
Week 9-14:  G9 ext2 (6w)
Week 12-14: G6 mdBook (2w)                             ← 可与 G9 并行
Week 15-16: G8 fsx (1w) + G11 容器 FS (4w 并行部分)
Week 17-20: G1 workspace Phase 3 (4w)                  ← 高风险模块
Week 21+:   G5 RISC-V / G10 TDX / G12 systree         ← 远期
```

## 工作量汇总

| Tier | 任务 | 预计工期 |
|------|------|----------|
| Tier 1 | G3 + G2 + G4 | 8-10 周 |
| Tier 2 | G1 + G7 | 4-5 周 |
| Tier 3 | G9 + G6 | 8-10 周 |
| Tier 4 | G8 + G11 + G12 | 6-8 周 |
| Tier 5 | G5 + G10 | 10-14 周 |
| **总计** | | **36-47 周** |
