# QueenX 内核工程路线图

> 基于 2026-07-08 代码现状，作为工程路线图。
>
> **G1 (workspace 组件化拆分)** 已于 2026-07-08 验证后搁置: 拆分 framework crate 不会降低 TCB (62.4% → 63.4%)，应转向 services 组件化。详见 archive/2026-07-07-workspace-refactor.md。

## 总体进度

- **总体进度**
  - 描述: 内核功能完整度与工程化水平
  - 方案: 11 项工程任务，已完成 4 项 (G2 overlayfs, G3 tmpfs, G4 syscall, G9 ext2+exfat)
  - 状态: [X]

---

## P0 — 容器落地关键路径 (2 项)

### G2: overlayfs 实装 ✅
- **描述**
  - 容器镜像核心，Docker/containerd/podman/CRI-O 均通过 overlayfs 提供 rootfs
  - 方案: 上层 (upperdir) + 下层 (lowerdir) + 工作层 (workdir) 三目录合并；copy_up 机制；readdir 合并去重
  - 状态: [X]

### G3: tmpfs 实装 ✅
- **描述**
  - 基于内存的文件系统，关键用途 /dev/shm (POSIX 共享内存)、容器临时文件系统、tmp 目录
  - 方案: 基于 ramfs 扩展，加入 size 限制 + 统计已用/可用
  - 状态: [X]

---

## P1 — 功能性缺口 (4 项)

### G4: 现代 syscall 补全 (15+ 个) ✅
- **描述**
  - 已实装: pidfd_open/pidfd_getfd/pidfd_send_signal, memfd_create, copy_file_range
  - 已实装: *at() 系列 9 个 (openat/newfstatat/unlinkat/renameat/linkat/symlinkat/readlinkat/fchmodat/faccessat)
  - 已实装: pipe2/dup3/epoll_create1
  - Credo syscall 重编号 (400-438 → 460-498) 解决 6 个编号冲突
  - 暂存根: name_to_handle_at/open_by_handle_at (返回 ENOSYS)
  - 状态: [X]
  - 状态: []

### G5: RISC-V 架构支持
- **描述**
  - Asterinas 支持 x86_64 + riscv64 + loongarch64；QueenX 仅 x86_64 + aarch64
  - 方案: RISC-V 64 启动 (OpenSBI) + 页表 (Sv39) + 异常 (stvec/sepc/scause) + 中断 (PLIC/CLINT) + 调度切换
  - 状态: []

- **工作量**: ~3000-5000 行；预计 6-8 周

### G6: mdBook 文档体系
- **描述**
  - 方案: docs/book/ 拆分 kernel-handbook + services-handbook + architecture + rfcs + contributing 5 部分
  - 状态: []

- **工作量**: 预计 2 周

### G7: CI/CD 流水线完善
- **描述**
  - QueenX 仅 .github/workflows/ci.yml 1 个
  - 方案: 拆分为 ci-x86.yml + ci-aarch64.yml + ci-lint.yml + ci-bench.yml 4 个
  - 状态: []

- **工作量**: 预计 1-2 周

---

## P2 — 工程化提升 (3 项)

### G8: fsx 集成测试
- **描述**
  - 方案: 引入 fsx-linux 测试程序，集成到 host-tests/；针对 ext2/exfat/overlayfs/tmpfs 各 1 轮
  - 状态: []

- **验收**: fsx-linux 跑通；100 万次操作无崩溃；数据校验通过

### G9: ext2 + exfat 磁盘 FS ✅
- **描述**
  - ext2 ~2500 行 + exfat ~3000 行；复用 VFS 与 page cache
  - 状态: [X]

### G10: TDX 机密计算支持
- **描述**
  - 方案: TDX module 检测 (CPUID 0x21) + tdcall 指令封装 + attest quote + 内存加密
  - 状态: []

- **工作量**: ~2300 行；预计 4-6 周

---

## P3 — 锦上添花 (2 项)

### G11: 容器辅助 FS 补全
- **描述**
  - devpts + cgroupfs + configfs + virtiofs
  - 方案: devpts ~400 行 + cgroupfs ~600 行 + configfs ~500 行 + virtiofs ~800 行
  - 状态: []

- **工作量**: 总计 4 周

### G12: systree 动态系统树 (部分完成)
- **描述**
  - 方案: Tree + Node + Attr 三元组；节点支持属性读写回调
  - 状态: []

- **工作量**: ~2500 行；预计 3-4 周

### G13: NFS 内核模块 (远期)
- **描述**
  - 网络文件共享支持 (FreeBSD 风格, 非 Linux 通用 syscall 方式)
  - 前提: 需要内核模块系统 + 用户态通信接口
  - 文件句柄由 NFS 模块内部管理, 不暴露给用户态
  - 不需要 name_to_handle_at / open_by_handle_at syscall
  - 状态: []

- **前提条件**
  - 内核模块加载/卸载机制
  - 用户态通信接口 (nfsiod 通道)
  - OpenFile 基础设施 ✅

- **工作量**: 预计 6-8 周 (含内核模块框架)

---

## 实施优先级路线图

```text
已完成:
  G2 overlayfs ✅
  G3 tmpfs ✅
  G4 syscall 补全 ✅
  G9 ext2 + exfat ✅

待实施:
  Week 1-2:   G7 CI 拆分 (1w) + G6 mdBook (2w)  ← 并行
  Week 3-4:   G8 fsx (1w)
  Week 5-8:   G11 容器 FS (4w)
  Week 9-12:  G12 systree (3-4w)
  Week 13+:   G5 RISC-V / G10 TDX  ← 远期
  Week 20+:   G13 NFS 内核模块  ← 远期 (需内核模块框架)
```

## 工作量汇总

| 优先级 | 任务 | 状态 | 预计工期 |
|--------|------|------|----------|
| P0 | G2 + G3 | ✅ 完成 | 0 周 |
| P1 | G4 + G5 + G6 + G7 | G4✅ | 10-13 周 |
| P2 | G8 + G9 + G10 | G9✅ | 6-10 周 |
| P3 | G11 + G12 | G12 部分 | 7-10 周 |
| **总计** | | **4 项完成** | **23-33 周** |

## 引用
- 评估依据: tmp/asterinas-main/ 源码 (Asterinas 0.18.0) + docs/plan/ 子系统规划 + docs/README.md 工程规范
