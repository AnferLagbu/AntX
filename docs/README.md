# AntX 内核文档

> **AntX** - 一个具有创新故障恢复机制的现代化宏内核操作系统

---

## 📚 文档导航

### 🏗️ [系统架构](./architecture/)
- [系统概述](./architecture/overview.md) - AntX的整体设计理念
- [内核架构](./architecture/kernel-architecture.md) - 内核模块组织与设计
- [启动流程](./architecture/boot-process.md) - 从BIOS到用户态的完整流程

### 🔧 [子系统](./subsystems/)
- [内存管理](./subsystems/memory/) - PMM/VMM/堆管理
- [进程管理](./subsystems/process/) - 进程、调度、上下文切换
- [文件系统](./subsystems/filesystem/) - VFS/RamFS/HvFS/DevFS/ProcFS
- [安全子系统](./subsystems/security/) - PWID/Session/权限模型
- [栏栈恢复](./subsystems/barrier/) - BBR/BSR/BHR故障恢复
- [驱动框架](./subsystems/driver/) - 统一驱动模型
- [网络栈](./subsystems/network/) - LWIP集成与网络服务

### 📖 [API参考](./api/)
- [系统调用](./api/syscall.md) - 用户态系统调用接口
- [内核API](./api/kernel-api.md) - 内核内部接口
- [驱动API](./api/driver-api.md) - 驱动开发接口

### 🛠️ [开发指南](./development/)
- [快速开始](./development/getting-started.md) - 环境搭建与编译
- [构建系统](./development/build-system.md) - Makefile与构建流程
- [编码规范](./development/coding-style.md) - 代码风格指南
- [调试指南](./development/debugging.md) - 调试技巧与工具

### 🧪 [测试文档](./testing/)
- [测试框架](./testing/test-framework.md) - 单元/集成/压力/混沌测试
- [测试覆盖](./testing/test-coverage.md) - 当前测试覆盖情况
- [测试报告](./testing/test-reports/) - 历史测试报告

### 📝 [变更记录](./changelog/)
- [变更日志](./changelog/CHANGELOG.md) - 版本变更历史
- [里程碑](./changelog/milestones.md) - 重要里程碑记录

### 🔬 [研究文档](./research/)
- [栏栈论文](./research/barrier-stack-paper.md) - 栏栈机制学术论文
- [实验记录](./research/experiments/) - 性能与对比实验

---

## 🚀 快速链接

### 新手入门
1. [系统概述](./architecture/overview.md) - 了解AntX是什么
2. [快速开始](./development/getting-started.md) - 编译运行第一个内核
3. [编码规范](./development/coding-style.md) - 开始贡献代码

### 核心特性
- **栏栈恢复**: [BBR/BSR/BHR三层恢复策略](./subsystems/barrier/)
- **PWID安全**: [基于能力的权限模型](./subsystems/security/)
- **HvFS文件系统**: [混合文件系统设计](./subsystems/filesystem/hvfs.md)

### 开发者资源
- [系统调用列表](./api/syscall.md)
- [内核API参考](./api/kernel-api.md)
- [测试框架使用](./testing/test-framework.md)

---

## 📊 项目状态

| 子系统 | 状态 | 测试覆盖 | 文档完整度 |
|--------|------|----------|-----------|
| 内存管理 | ✅ 稳定 | 95% | 90% |
| 进程管理 | ✅ 稳定 | 90% | 85% |
| 文件系统 | ✅ 稳定 | 92% | 88% |
| 安全子系统 | ✅ 稳定 | 88% | 85% |
| 栏栈恢复 | ✅ 稳定 | 95% | 90% |
| 驱动框架 | ⚠️ 开发中 | 75% | 70% |
| 网络栈 | ⚠️ 开发中 | 70% | 65% |

---

## 🤝 贡献指南

1. Fork本仓库
2. 创建特性分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 创建Pull Request

详见 [编码规范](./development/coding-style.md)

---

## 📜 许可证

本项目采用 MIT 许可证 - 详见 [LICENSE](../LICENSE) 文件

---

## 📧 联系方式

- 项目主页: https://gitee.com/anfer/antx
- 问题反馈: https://gitee.com/anfer/antx/issues

---

**最后更新**: 2026-05-18
**文档版本**: v2.0
