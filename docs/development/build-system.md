# 构建系统

> Makefile与构建流程

---

## 🎯 构建目标

### 主要目标

```bash
make all          # 编译所有组件
make kernel       # 编译内核
make user         # 编译用户程序
make clean        # 清理构建
make iso          # 生成ISO镜像
```

### 测试目标

```bash
make test         # 单元测试
make test-unit    # 单元测试
make test-integration # 集成测试
make test-stress  # 压力测试
make test-chaos   # 混沌测试
```

### 运行目标

```bash
make run          # 运行内核
make debug        # 调试模式
make log          # 查看日志
```

---

## 📦 构建流程

```
1. 编译C代码
   ├─ 内核C代码
   ├─ 驱动C代码
   └─ 用户程序C代码

2. 编译Rust代码
   └─ cargo build --release

3. 编译汇编代码
   ├─ boot.asm
   ├─ entry.asm
   └─ isr.asm

4. 链接
   └─ ld -T link.ld

5. 生成ISO
   └─ grub2-mkrescue
```

---

## 🔧 Makefile结构

```makefile
# 编译器设置
CC = x86_64-linux-gnu-gcc
LD = x86_64-linux-gnu-ld
AS = nasm

# 编译标志
CFLAGS = -std=c11 -m64 -Wall -O2

# 目标
all: kernel user

kernel: $(KERNEL_OBJS) $(RUST_LIB)
	$(LD) $(LDFLAGS) -o $@ $^

user: $(USER_OBJS)
	$(LD) $(USER_LDFLAGS) -o $@ $^
```

---

## 🐛 常见问题

### Q: 编译失败：找不到Rust编译器

```bash
rustup default nightly
rustup target add x86_64-unknown-none
```

### Q: 链接失败：未定义引用

检查是否所有依赖都已编译。

---

**最后更新**: 2026-05-18
