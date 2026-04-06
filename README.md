# AntX

AntX 是一款**完全独立自研**的轻量级 x86_64 操作系统，定位为纯个人技术实验与探索项目。

## 核心设计理念

- **极简优先** — 拒绝冗余，聚焦想法验证
- **独立探索** — 从零设计，保持完全独立性
- **内核核心** — 所有创新（PWID 权限模型、多会话机制、临时提权）均在内核层面实现

## 特色功能

### PWID 权限模型

AntX 摒弃传统"用户"概念，采用「密码+备注→唯一 PWID」模型：
- 无需预先创建账户
- 同一密码 + 不同备注 = 不同身份
- 三级权限：Root / Trustworthy / Untrustworthy

### 多会话系统

- 多终端同时登录
- 会话级 PWID 权限绑定
- 身份切换即时生效

### 原 Root 锚点

- 全局唯一，不可删除
- 首次启动强制设置
- 身份通过内核硬编码保护

### 临时提权

- 向原 Root 提权
- 验证密码后执行单一命令
- 执行完毕自动恢复原身份

## 开发状态

- **运行环境**：QEMU x86_64 模拟器
- **开发工具**：MinGW-w64 + NASM + GDB
- **当前目标**：极简可运行内核

## 文档

详细设计文档见 [docs/](docs/) 目录：
- [内核架构](docs/kernel-architecture.md)
- [PWID 权限模型](docs/pwid-model.md)
- [进程与会话](docs/process-session.md)
- [文件系统](docs/hivefs.md)
- [内存管理](docs/memory-management.md)
- [系统调用](docs/syscall.md)
