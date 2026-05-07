# QueenX / AntX 当前开发任务清单

> **命名规范**: QueenX (QX) = 内核 | AntX = QueenX + 用户态组件
> **最后更新**: 2026-05-07

---

## 🎯 当前版本: Git Dynamic Versioning (见 `sver` 命令)

**整体完成度**: 约 65%
**核心理念**: 极致简洁，独立创新

---

## ✅ 已完成任务 (截至 2026-05-07)

### 核心基础设施
| 功能 | 状态 | 说明 |
|------|------|------|
| Multiboot2 启动 + 双映射 | ✅ | boot.asm |
| GDT/IDT/中断处理 | ✅ | 含增强异常处理 |
| KLog 日志系统 | ✅ | 6级/12分类/环形缓冲区 |
| 动态版本系统 | ✅ | Git commit hash + 11模块注册 |
| PIC 位置无关代码 | ✅ | -fPIC -mcmodel=medium |

### 内存管理 (Rust 重写完成)
| 功能 | 状态 |
|------|------|
| PMM 位图分配器 | ✅ |
| VMM 四级页表 | ✅ |
| kmalloc 内核堆 | ✅ |
| Slab 分配器 | ✅ |
| 大页支持 (2MB/1GB) | ✅ |
| SMEP/NX 位 | ✅ |

### 进程管理 (Rust 重写完成)
| 功能 | 状态 |
|------|------|
| 进程创建/退出/等待 | ✅ |
| MLFQ + RT 调度器 | ✅ |
| 线程模型 | ✅ |
| 等待队列 | ✅ |
| ELF 加载器 + 用户态切换 | ✅ |
| 会话管理 | ✅ |

### PWID 权限 (Rust 重写完成)
| 功能 | 状态 |
|------|------|
| SHA-256 PWID 生成/验证 | ✅ |
| 三级权限 (Root/Trustworthy/Untrustworthy) | ✅ |
| 原 Root 锚点 | ✅ |
| 令牌提权 (token_create/use/revoke) | ✅ |
| 信任链 (trust_add/remove) | ✅ |
| 能力矩阵 (capability) | ✅ |
| 暴力破解防护 + 审计日志 | ✅ |

### IPC
| 功能 | 状态 |
|------|------|
| 管道 (Pipe) | ✅ |
| 信号 (Signal) | ✅ |
| 共享内存 (SHM) | ✅ |
| 消息队列 (MsgQ) | ✅ |
| 信号量 (Semaphore) | ✅ |

### 文件系统 (Rust 重写完成)
| 模块 | 状态 |
|------|------|
| VFS 抽象层 | ✅ |
| RamFS | ✅ |
| DiskFS | ✅ |
| HvFS (含持久化) | ✅ |
| DevFS | ✅ |
| ProcFS | ✅ |
| Smart Mount (3模式) | ✅ |

### 系统调用
| 分类 | 已注册 |
|------|--------|
| 进程管理 | 7 |
| 文件操作 | 13 |
| PWID 权限 | 17 (含token/trust) |
| 文件系统 | 2 (mount/unmount) |
| 磁盘操作 | 5 |
| 系统信息 | 6 |
| **总计** | **37** |

### 驱动与硬件
| 驱动 | 状态 |
|------|------|
| ATA PIO 磁盘 | ✅ |
| PS/2 键盘 | ✅ |
| PIT 定时器 | ✅ |
| Intel E1000 网卡 | ✅ |
| PCI 总线 | ✅ |
| DMA 引擎 (Rust) | ✅ |

### 同步原语
| 原语 | 状态 |
|------|------|
| Spinlock | ✅ |
| Atomic | ✅ |
| R/W Lock | ✅ |
| Mutex | ✅ |

### 网络栈
| 组件 | 状态 |
|------|------|
| lwIP 2.2.1 协议栈 | ✅ |
| DHCP/ICMP/DNS | ✅ |
| HTTP Server/Client | ✅ |
| mDNS/MQTT/SNMP/SNTP/TFTP | ✅ |

### 用户程序
| 程序 | 状态 |
|------|------|
| init 进程 | ✅ |
| antxsh Shell (17命令) | ✅ |
| 安装向导 | ✅ |

### 测试框架
| 类型 | 用例数 | 状态 |
|------|--------|------|
| 单元测试 | 192+ | ✅ ~90%通过 |
| QEMU 硬件测试 | 多个 | ✅ |
| Host CPU 测试 | 多个 | ✅ |

### Bug 修复 (2026-05-07)
| 问题 | 状态 |
|------|------|
| interrupt_frame 字段错位 | ✅ 已修复 |
| syscall arg3 参数丢失 | ✅ 已修复 |
| 调度器 ZOMBIE 死循环 | ✅ 已修复 |

---

## ⚡ 待完成任务 (P0-P2)

### P1: VFS/HvFS 稳定性修复
**状态**: ⏳ 待开始

单元测试中发现的 5 个失败用例:
- VFS: Create file, Write/read, File stat, Delete file, Large file
  (根因: 测试环境无磁盘，DiskFS 路径预期失败)

### P1: 用户程序完善
**状态**: ⏳ 进行中
- [ ] Shell 功能增强
- [ ] 更多用户态工具

### P2: 内存管理高级特性
- [ ] Buddy System (当前位图可满足)
- [ ] COW 写时复制
- [ ] mmap 内存映射

### P2: 网络 Socket API
- [ ] Socket 系统调用 (socket/bind/listen/connect)
  (lwIP 协议栈已就绪，需封装 syscall 接口)

### P2: SMP 多核支持
- [ ] 多核启动 (基础框架已就绪)
- [ ] 核间中断 IPI

---

## 📊 AntX vs Unix/Linux 核心差异

| 功能 | Unix/Linux | AntX | 本质区别 |
|------|-----------|------|----------|
| **身份标识** | UID/GID | PWID (密码+备注) | 无用户概念 |
| **权限提升** | sudo/su | Token 令牌 (基于密码) | 无用户切换 |
| **文件权限** | rwx (9位) | PWID 权限位 + capability | 无用户组概念 |
| **进程调度** | CFS/nice | MLFQ + RT | 多级反馈+实时 |
| **文件系统** | ext4/btrfs... | HvFS 专属 | 不兼容，专注自身 |

---

## ⚠️ 开发原则

1. **极简实现** - 保持代码简洁，拒绝过度设计
2. **独立探索** - 不盲目模仿其他系统
3. **实事求是** - 只解决真实存在的问题
4. **文档驱动** - 先写文档再写代码

---

*最后更新: 2026-05-07 (根据源码实现订正)*
