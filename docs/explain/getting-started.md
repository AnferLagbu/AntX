# 快速开始指南

> 5分钟快速上手AntX内核开发

---

## 📋 环境要求

### 必需工具

| 工具 | 版本 | 说明 |
|------|------|------|
| GCC | x86_64-linux-gnu-gcc | C编译器 |
| Rust | nightly | Rust编译器 |
| NASM | 2.15+ | 汇编器 |
| LD | x86_64-linux-gnu-ld | 链接器 |
| Make | GNU Make | 构建工具 |
| QEMU | 6.0+ | 虚拟机 |

### 可选工具

- GDB: 调试工具
- GRUB2: 引导加载程序
- xorriso: ISO镜像生成

---

## 🚀 快速开始

### 1. 克隆仓库

```bash
git clone https://gitee.com/anfer/antx.git
cd antx
```

### 2. 安装依赖

```bash
# Ubuntu/Debian
sudo apt-get install nasm qemu-system-x86 grub2-common xorriso

# Arch Linux
sudo pacman -S nasm qemu-headless grub xorriso

# 安装Rust nightly
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default nightly
rustup target add x86_64-unknown-none
```

### 3. 编译内核

```bash
# 编译所有组件
make all

# 或分步编译
make kernel  # 仅编译内核
make user    # 仅编译用户程序
```

### 4. 运行内核

```bash
# 在QEMU中运行
make run

# 或使用ISO镜像
make iso
qemu-system-x86_64 -cdrom build/antx.iso
```

### 5. 运行测试

```bash
# 运行所有测试
make test-all

# 或分步测试
make test-unit        # 单元测试
make test-integration # 集成测试
make test-stress      # 压力测试
make test-chaos       # 混沌测试
```

---

## 📂 项目结构

```
antx/
├── src/              # 源代码
│   ├── kernel/       # 内核代码
│   ├── rust/         # Rust模块
│   ├── user/         # 用户程序
│   └── include/      # 头文件
├── docs/             # 文档
├── tests/            # 测试
├── build/            # 构建输出
└── Makefile          # 构建脚本
```

---

## 🔧 常用命令

### 构建命令

```bash
make all          # 编译所有
make clean        # 清理构建
make kernel       # 编译内核
make user         # 编译用户程序
make iso          # 生成ISO镜像
```

### 运行命令

```bash
make run          # 运行内核
make run-net      # 运行（启用网络）
make debug        # 调试模式
make log          # 查看日志
```

### 测试命令

```bash
make test         # 单元测试
make test-unit    # 单元测试
make test-integration # 集成测试
make test-stress  # 压力测试
make test-chaos   # 混沌测试
make test-all     # 所有测试
```

---

## 🐛 调试

### 使用GDB调试

```bash
# 终端1：启动QEMU（等待GDB连接）
qemu-system-x86_64 -cdrom build/antx.iso -s -S

# 终端2：启动GDB
gdb build/kernel.bin
(gdb) target remote localhost:1234
(gdb) break kernel_main
(gdb) continue
```

### 查看日志

```bash
# 运行并保存日志
make run 2>&1 | tee kernel.log

# 查看特定模块日志
grep "\[BARRIER\]" kernel.log
```

---

## 📝 开发流程

### 1. 创建特性分支

```bash
git checkout -b feature/my-feature
```

### 2. 修改代码

```bash
# 编辑代码
vim src/kernel/xxx.c

# 或Rust代码
vim src/rust/src/kernel/xxx.rs
```

### 3. 编译测试

```bash
# 编译
make clean && make all

# 运行测试
make test
```

### 4. 提交代码

```bash
git add .
git commit -m "feat: add my feature"
git push origin feature/my-feature
```

### 5. 创建Pull Request

在Gitee上创建Pull Request，等待审核。

---

## 🎯 下一步

- [阅读系统概述](../architecture/overview.md)
- [了解内核架构](../architecture/kernel-architecture.md)
- [学习编码规范](./coding-style.md)
- [探索子系统](../subsystems/)

---

## ❓ 常见问题

### Q: 编译失败：找不到Rust编译器

**A**: 安装Rust nightly：
```bash
rustup default nightly
rustup target add x86_64-unknown-none
```

### Q: QEMU运行失败：找不到内核

**A**: 先编译内核：
```bash
make all
```

### Q: 测试失败：权限不足

**A**: 某些测试需要root权限：
```bash
sudo make test
```

### Q: 如何添加新系统调用？

**A**: 参考 [系统调用文档](../api/syscall.md)

---

## 📚 学习资源

- [AntX架构文档](../architecture/)
- [内核开发指南](./)
- [API参考](../api/)
- [测试框架](../testing/)

---

**最后更新**: 2026-05-18
