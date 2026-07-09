# QueenX 内核工程路线图

> 基于 2026-07-08 代码现状，作为工程路线图。
>
> **G1 (workspace 组件化拆分)** 已于 2026-07-08 验证后搁置: 拆分 framework crate 不会降低 TCB (62.4% → 63.4%)，应转向 services 组件化。
>
> **远期任务** (RISC-V / TDX / NFS) 已独立至 [future-roadmap.md](future-roadmap.md)。

## 总体进度

- **总体进度**
  - 描述: 内核功能完整度与工程化水平
  - 方案: 8 项近期任务，已完成 4 项 (G2/G3/G4/G9)
  - 状态: [X]

---

## P0 — 容器落地关键路径 (2 项)

### G2: overlayfs 实装 ✅
- **描述**: 容器镜像核心 (upperdir + lowerdir + workdir)
- **状态**: [X]

### G3: tmpfs 实装 ✅
- **描述**: 内存文件系统 (/dev/shm, 容器临时文件)
- **状态**: [X]

---

## P1 — 功能性缺口 (2 项)

### G4: 现代 syscall 补全 ✅
- **描述**: 15+ syscall 实装 (pidfd/*at/memfd/copy_file_range/pipe2/dup3/epoll_create1)
- **状态**: [X]

### G5: mdBook 文档体系
- **描述**: docs/book/ 拆分 kernel-handbook + services-handbook + architecture + rfcs + contributing
- **状态**: []

- **工作量**: 预计 2 周

### G6: CI/CD 流水线完善
- **描述**: 拆分为 ci-x86.yml + ci-aarch64.yml + ci-lint.yml + ci-bench.yml
- **状态**: []

- **工作量**: 预计 1-2 周

---

## P2 — 工程化提升 (2 项)

### G7: fsx 集成测试
- **描述**: 引入 fsx-linux 测试程序，针对 ext2/exfat/overlayfs/tmpfs 各 1 轮
- **状态**: []

- **验收**: fsx-linux 跑通；100 万次操作无崩溃

### G8: ext2 + exfat 磁盘 FS ✅
- **描述**: ext2 ~2500 行 + exfat ~3000 行
- **状态**: [X]

---

## P3 — 锦上添花 (2 项)

### G9: 容器辅助 FS 补全
- **描述**: devpts + cgroupfs + configfs + virtiofs
- **状态**: []

- **工作量**: 总计 4 周

### G10: systree 动态系统树 (部分完成)
- **描述**: Tree + Node + Attr 三元组；节点支持属性读写回调
- **状态**: []

- **工作量**: ~2500 行；预计 3-4 周

---

## 实施优先级路线图

```text
已完成:
  G2 overlayfs ✅
  G3 tmpfs ✅
  G4 syscall 补全 ✅
  G8 ext2 + exfat ✅

待实施:
  Week 1-2:   G6 CI 拆分 (1w) + G5 mdBook (2w)  ← 并行
  Week 3-4:   G7 fsx (1w)
  Week 5-8:   G9 容器 FS (4w)
  Week 9-12:  G10 systree (3-4w)
```

## 工作量汇总

| 优先级 | 任务 | 状态 | 预计工期 |
|--------|------|------|----------|
| P0 | G2 + G3 | ✅ 完成 | 0 周 |
| P1 | G4 + G5 + G6 | G4✅ | 3-4 周 |
| P2 | G7 + G8 | G8✅ | 1 周 |
| P3 | G9 + G10 | G10 部分 | 7-8 周 |
| **总计** | | **4 项完成** | **11-13 周** |

## 引用
- 评估依据: tmp/asterinas-main/ 源码 (Asterinas 0.18.0) + docs/plan/ 子系统规划
