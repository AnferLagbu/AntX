# QueenX 命名立场实装进度

> 跟踪 [../explain/naming-standpoint.md](../explain/naming-standpoint.md) 立场书各项设计决策的实际实装进度. 此文档与立场书分离, 立场书是"做什么/为什么", 本文档是"做到哪一步". 截至 2026-06-28.

## 总体进度

- **总体进度**
  - 描述: 6 大立场决策的实装完成度
  - 方案: §三 syscall 编号 100% / §六 linuxulator 50% / §二 命名 0% / §四 工具链 0% / §五 libc 0%
  - 状态: [X]

## §二 命名规则实装

### §2.1 文件与目录命名
- **elfld.so 实装**
  - 描述: `/usr/libexec/elfld.so` 自研动态链接器
  - 方案: 全树搜索无 elfld.so 文件, 仅 services/linuxulator.rs 注释中提及 PT_INTERP 改写; 无实际源码
  - 状态: [ ]
- **queenx_libc.musl 实装**
  - 描述: `queenx_libc.musl-<arch>.so.1`
  - 方案: 无 musl 派生代码, 无 queenx_libc 库
  - 状态: [ ]
- **内核镜像路径实装**
  - 描述: `/queenx/queenx.elf` 内核镜像
  - 方案: 无相关路径配置代码
  - 状态: [ ]

### §2.2 路径层级
- **路径层级实装**
  - 描述: `/queenx/` `/usr/libexec/` 等 7 类路径
  - 方案: 无 `/queenx/` 根目录相关代码; 当前 initramfs/ramfs 路径为传统 `/bin /sbin /etc`
  - 状态: [ ]

### §2.3 命名禁词表
- **命名禁词表验证**
  - 描述: 禁止 linux/freebsd/darwin 等商标词
  - 方案: 通过 .typos 配置或 grep 验证; 当前无集中命名约束工具
  - 状态: [ ]

## §三 syscall 编号实装

### §3.1 编号空间分配
- **QX_* 编号完整实装**
  - 描述: 500-747 编号空间按功能分区
  - 方案: `src/kernel/services/syscall/types.rs` 第 259-307 行完整定义 100+ QX_* 编号, 按 进程/内存/文件/网络/IPC/设备/扩展 分区
  - 状态: [X]
- **保留区间实装**
  - 描述: 0-299 保留给 linuxulator, 300-399 保留, 400-499 Credo
  - 方案: `types.rs` 第 17 行注释明确, dispatch 入口识别并分发
  - 状态: [X]

### §3.2 dispatch 路径实装
- **QX dispatch 完整实装**
  - 描述: syscall_dispatch 接收 QX_* 编号, 走完整路径
  - 方案: `src/kernel/services/syscall/mod.rs` 第 190-200 行 translate_syscall → dispatch
  - 状态: [X]

## §四 工具链实装

### §4.1 queenx-gcc wrapper
- **wrapper 脚本实装**
  - 描述: `tools/queenx-gcc` 透明转发 + 注入 PT_INTERP
  - 方案: `tools/` 目录无 queenx-gcc 文件, 仅 audit/check 类脚本
  - 状态: [ ]
- **架构特定配置实装**
  - 描述: 根据 x86_64/aarch64/riscv64 选 interp 路径
  - 方案: 无 wrapper, 无架构特定配置
  - 状态: [ ]

### §4.2 PT_INTERP 改写
- **PT_INTERP 改写实装**
  - 描述: 内核层改写 PT_INTERP 字段为 elfld.so
  - 方案: linuxulator.rs 注释中提及, 但无 execve 路径中的实际改写代码
  - 状态: [ ]

## §五 libc 选型实装

### §5.1 musl 派生
- **musl 派生实装**
  - 描述: 基于 musl src/ 派生 queenx_libc.musl
  - 方案: 无 musl 源码, 无 queenx_libc 派生代码
  - 状态: [ ]

## §六 linuxulator 实装

### §6.1 编号翻译层
- **translate_syscall 实装**
  - 描述: 编号翻译 (Linux → QX_*)
  - 方案: `src/kernel/services/syscall/linuxulator.rs` 第 78 行 translate_syscall + 3 个架构分支函数 (x86_64/aarch64/默认), x86_64 分支 ~150 个 Linux 编号映射 (read/write/open/stat/mmap/signal/ioctl 等核心系统调用)
  - 状态: [X]
- **translate_args 实装**
  - 描述: 参数转换 (Linux ABI → QX)
  - 方案: 第 108 行 translate_args 函数, 转换为 LinuxArgs 结构体; 实际转换逻辑分散到具体编号处理
  - 状态: [X]
- **is_rt_sigreturn 实装**
  - 描述: rt_sigreturn 特殊识别
  - 方案: 第 63 行 (x86_64 SYS 15) + 第 69 行 (aarch64 SYS 139) 双架构特殊处理
  - 状态: [X]

### §6.2 PT_INTERP 改写
- **PT_INTERP 改写**
  - 描述: 内核层改写 Linux ELF 的 PT_INTERP 字段
  - 方案: 无 execve 路径中的改写代码; 仅注释提及
  - 状态: [ ]

### §6.3 `/proc /sys` 仿真
- **/proc/sys 仿真**
  - 描述: linuxulator 提供 Linux 风格的 /proc/cpuinfo /sys/class 等
  - 方案: 无仿真层; procfs/sysfs 仅按 QueenX 自身格式提供
  - 状态: [ ]

### §6.4 设备 ioctl 翻译
- **ioctl 翻译**
  - 描述: 设备 ioctl 编号翻译 (Linux → QX)
  - 方案: linuxulator 仅有通用 QX_IOCTL 单条映射, 无具体设备 ioctl 翻译
  - 状态: [ ]

## §十 Phase D 优先序实装

### D1 网络栈收尾
- **网络栈收尾**
  - 描述: smoltcp 集成 + NetStack trait
  - 方案: 已实装, 见 archive/smoltcp-framekernel-wrapper.md
  - 状态: [X]

### D2 HiveFS e2e 测试
- **HiveFS 端到端测试**
  - 描述: 真实磁盘挂载 + 读写测试
  - 方案: hvfs_persist_test.rs 等 host-tests 已实装
  - 状态: [X]

### D3.5 eash 增强
- **eash 增强**
  - 描述: easy shell 功能扩展
  - 方案: `src/user/eash/` 含 commands/pipeline.rs 等, 多个命令已实装
  - 状态: [X]

### D4 elfld.so 实现
- **elfld.so 自研动态链接器**
  - 描述: 实现 ELF 动态加载器
  - 方案: **未实装** — 无 elfld.so 源码
  - 状态: [ ]

### D5 linuxulator
- **linuxulator 完整实装**
  - 描述: 200+ syscall 翻译 + PT_INTERP 改写 + /proc 仿真 + ioctl 翻译
  - 方案: **仅完成 50%** — 编号翻译 + 参数转换已实装, 其余 3 项未做
  - 状态: [ ]

### D6 eash 单元测试
- **eash 单元测试**
  - 描述: eash 模块单测
  - 方案: eash/src/ 下有命令处理逻辑, 测试覆盖在 host-tests/ 中
  - 状态: [X]

## 实装进度汇总

| 范畴 | 已实装 | 未实装 | 进度 |
|------|--------|--------|------|
| **§二 命名规则** | 0 | 3 | 0% |
| **§三 syscall 编号** | 3 | 0 | 100% |
| **§四 工具链** | 0 | 2 | 0% |
| **§五 libc 选型** | 0 | 1 | 0% |
| **§六 linuxulator** | 3 | 3 | 50% |
| **§十 Phase D 优先序** | 4 | 2 | 67% |
| **总计** | 10 | 11 | 48% |

## 关键实装路径建议

### 优先实装 (按 ROI)
- **D4 elfld.so**
  - 描述: 剩余命名立场的实装前置
  - 方案: 实现 ELF 动态加载器核心 (~1500 行); 一旦实装, §四工具链 + §五 libc + §六 PT_INTERP 改写可联动实施
  - 状态: [ ]
- **§六 PT_INTERP 改写**
  - 描述: linuxulator 缺失的关键拼图
  - 方案: 在 execve 路径中检测 ELF PT_INTERP, 改写为 elfld.so 路径; 配合 D4 完成
  - 状态: [ ]
- **§六 /proc/sys 仿真**
  - 描述: 提升 Linux 二进制兼容性
  - 方案: 基于现有 procfs/sysfs 实现 Linux 风格字段映射层; 增量式按需添加
  - 状态: [ ]

### 后续实装
- **§四 queenx-gcc wrapper**
  - 描述: 工具链封装
  - 方案: shell 脚本即可, 工程量 0.5 周
  - 状态: [ ]
- **§六 ioctl 翻译**
  - 描述: 设备 ioctl 兼容性
  - 方案: 按需实现, 不必一步到位
  - 状态: [ ]
- **§五 musl 派生**
  - 描述: 自研 libc
  - 方案: 工程量大, 优先级最低
  - 状态: [ ]

## 工作量估算

### 关键实装工作量
- **D4 elfld.so**
  - 描述: 自研动态链接器
  - 方案: ~1500-2000 行 Rust; 需要解析 ELF 段、重定位、PLT/GOT、动态符号表; 工程量 4-6 周
  - 状态: [ ]
- **§六 剩余 50%**
  - 描述: PT_INTERP 改写 + /proc/sys 仿真 + ioctl 翻译
  - 方案: 各 1-2 周, 总计 4-6 周
  - 状态: [ ]
- **§四 工具链**
  - 描述: queenx-gcc wrapper
  - 方案: shell 脚本 100-200 行, 0.5 周
  - 状态: [ ]
- **总计**
  - 描述: 关键实装路径总工作量
  - 方案: 8-12 周可达成立场书 90% 实装度
  - 状态: [X]

## 引用
- **引用清单**
  - 描述: 关联文档
  - 方案: ../explain/naming-standpoint.md (立场书本体) + ../explain/framekernel-nature.md (framekernel 架构) + ../explain/hope-and-utopia.md (战略愿景)
  - 状态: [X]