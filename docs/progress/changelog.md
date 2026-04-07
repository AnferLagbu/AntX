# AntX 变更日志

本文件记录 AntX 操作系统的重要变更历史。

---

## [Unreleased]

### Added - 新增功能
- 测试框架初步实现
  - `tests/scripts/diagnose_user_process.py` - ELF一致性检查与自动修复工具
  - `tests/scripts/test_user_process.py` - QEMU自动化测试脚本

### Changed - 变更
- 重写 `process_start_user_asm` (switch.asm)
  - 修复 iretq 栈帧构建顺序
  - 使用 rbx/r12 保存关键寄存器

### Fixed - 修复
- 修复 `user_init_bin.c` ELF 入口点不匹配问题
  - 旧入口点: 0x400C7E (无效指令位置)
  - 新入口点: 0x400C02 (正确的 init_main 函数)

---

## [0.1.0] - 2026-04-06

### Added - 新增功能
- 基础内核架构
  - GDT/IDT 初始化
  - 物理内存管理 (PMM)
  - 虚拟内存管理 (VMM)
  - 进程管理与调度器
- PWID 权限模型
  - 三级权限体系 (ROOT/TRUSTWORTHY/UNTRUSTWORTHY)
  - SHA256 密码哈希
- 文件系统
  - VFS 虚拟文件系统层
  - RamFS 内存文件系统
  - DiskFS 磁盘文件系统
- 用户进程支持
  - ELF 加载器
  - 用户模式切换

### Known Issues - 已知问题
- 用户进程启动后无输出 (Issue #9)
- 开机调试信息缺乏真正的错误检测 (Issue #12)

---

## 版本说明

遵循 [语义化版本](https://semver.org/lang/zh-CN/) 规范：

- **主版本号**: 不兼容的 API 修改
- **次版本号**: 向下兼容的功能性新增
- **修订号**: 向下兼容的问题修正

---

*最后更新: 2026-04-07*
