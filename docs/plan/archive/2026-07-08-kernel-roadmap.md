# QueenX 内核工程路线图

> 基于 2026-07-08 代码现状，作为近期工程路线图。
>
> 远期任务 (RISC-V / TDX / NFS / mdBook) 独立至 [future-roadmap.md](future-roadmap.md)。

## 总体进度

- **总体进度**
  - 描述: 内核功能完整度与工程化水平
  - 方案: 8 项近期任务，已完成 8 项 (G2/G3/G4/G5/G6/G7/G8/G9)
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

### G5: CI/CD 流水线完善 ✅
- **描述**: 拆分为 ci-x86.yml + ci-aarch64.yml + ci-lint.yml + ci-bench.yml
- **状态**: [X]

---

## P2 — 工程化提升 (2 项)

### G6: fsx 集成测试 ✅
- **描述**: 引入 fsx-linux 测试程序，针对 ext2/exfat/overlayfs/tmpfs 各 1 轮
- **状态**: [X]

### G7: ext2 + exfat 磁盘 FS ✅
- **描述**: ext2 ~2500 行 + exfat ~3000 行
- **状态**: [X]

---

## P3 — 锦上添花 (2 项)

### G8: 容器辅助 FS 补全 ✅
- **描述**: devpts + cgroupfs + configfs + virtiofs
- **状态**: [X]

### G9: systree 动态系统树 ✅
- **描述**: Tree + Node + Attr 三元组；节点支持属性读写回调
- **状态**: [X]

---

## 实施优先级路线图

```text
全部完成:
  G2 overlayfs ✅
  G3 tmpfs ✅
  G4 syscall 补全 ✅
  G5 CI 拆分 ✅
  G6 fsx ✅
  G7 ext2 + exfat ✅
  G8 容器 FS ✅
  G9 systree ✅
```

## 工作量汇总

| 优先级 | 任务 | 状态 | 预计工期 |
|--------|------|------|----------|
| P0 | G2 + G3 | ✅ 完成 | 0 周 |
| P1 | G4 + G5 | ✅ 完成 | 0 周 |
| P2 | G6 + G7 | ✅ 完成 | 0 周 |
| P3 | G8 + G9 | ✅ 完成 | 0 周 |
| **总计** | | **8 项完成** | **0 周** |

## 引用
- 评估依据: tmp/asterinas-main/ 源码 (Asterinas 0.18.0) + docs/plan/ 子系统规划
