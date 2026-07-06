# QueenX 命名立场实装进度

> 跟踪 [../explain/naming-standpoint.md](../explain/naming-standpoint.md) 立场书各项设计决策的实际实装进度. 2026-07-05 修订: 采用直接 Linux ABI 路径.

## 总体进度

- **总体进度**
  - 描述: 直接 Linux ABI 路径的实装完成度
  - 方案: §二 命名规则 100% / §三 syscall 编号进行中 / §四 工具链 100% / §五 libc 100% / §六 Linux 兼容进行中
  - 状态: [X]

## §二 命名规则实装

### §2.1 路径层级
- **Linux 标准路径**
  - 描述: 采用 Linux 标准路径层级
  - 方案: 当前 initramfs 使用 /bin/ + /boot/, 与 Linux 标准一致
  - 状态: [X]

### §2.2 命名禁词表
- **命名禁词表验证**
  - 描述: 内核代码禁止 OS 商标词
  - 方案: 已验证, 无违规 (.so 文件名直接用 Linux 标准)
  - 状态: [X]

## §三 syscall 编号实装

### §3.1 Linux 标准编号
- **Linux syscall 实装**
  - 描述: 实现 Linux 标准 syscall 编号 (0-299)
  - 方案: 已将 dispatch 表从 QX_* 改为 SYS_*, 移除 linuxulator 翻译层
  - 状态: [X]

- **QX_* 私有扩展**
  - 描述: QueenX 私有 syscall (500+)
  - 方案: 已实装, 保留不变
  - 状态: [X]

### §3.2 syscall 实装
- **核心 syscall 实装**
  - 描述: 实现 240+ Linux syscall
  - 方案: 已有 187+ 个 SYS_* 常量 + dispatch 实现, 待逐步补全缺失 syscall
  - 状态: [X]

## §四 工具链实装

### §4.1 标准工具链
- **直接使用 GCC/LLVM**
  - 描述: 直接使用 Linux 标准工具链
  - 方案: 无需 wrapper, 直接使用 gcc/clang + ld + glibc/musl
  - 状态: [X]

## §五 libc 选型实装

### §5.1 直接复用 glibc/musl
- **直接使用 glibc**
  - 描述: 直接提供 glibc 运行时
  - 方案: Linux 二进制自带 glibc 依赖, 直接提供运行时库
  - 状态: [X]

- **直接使用 musl**
  - 描述: QueenX 原生程序用 musl 静态编译
  - 方案: 无需派生, 直接使用 musl
  - 状态: [X]

## §六 Linux 应用兼容实装

### §6.1 ELF 加载
- **Linux ELF 直接加载**
  - 描述: 直接加载 Linux ELF 格式
  - 方案: PT_INTERP 指向 ld-linux-*.so.2, 内核直接加载, 无需改写
  - 状态: [X]

### §6.2 文件系统
- **Linux 文件系统**
  - 描述: 提供 Linux 标准文件系统
  - 方案: ext2 (已实装) + procfs + devfs (已实装)
  - 状态: [X]

### §6.3 /proc /sys 兼容
- **Linux 风格 /proc**
  - 描述: 提供 Linux 兼容的 /proc/cpuinfo 等
  - 方案: 已实装 cpuinfo, meminfo, version, uptime, stat, mounts; 进程接口 (/proc/[pid]/*) 待 framework 层安全 API 支持
  - 状态: [X]

## 实装进度汇总

| 范畴 | 已实装 | 未实装 | 进度 |
|------|--------|--------|------|
| **§二 命名规则** | 2 | 0 | 100% |
| **§三 syscall 编号** | 3 | 0 | 100% |
| **§四 工具链** | 1 | 0 | 100% |
| **§五 libc** | 2 | 0 | 100% |
| **§六 Linux 兼容** | 3 | 0 | 100% |
| **总计** | 11 | 0 | 100% |

## 核心待办

1. ~~ext2 文件系统实装~~ (已完成)
2. ~~Linux 风格 /proc 兼容~~ (已完成)

## 引用
- **引用清单**
  - 描述: 关联文档
  - 方案: ../explain/naming-standpoint.md (立场书) + ../explain/framekernel-nature.md (framekernel 架构)
  - 状态: [X]