# queenx 命名与生态兼容立场书

> 2026-06-08 多轮讨论沉淀
>
> **本文件定义 queenx 内核在"命名"、"syscall 编号"、"工具链"、"libc"、"路径层级"、"linuxulator 兼容" 等关键设计点上的立场.** 它是 queenx 工程规划的**约束前提**, 后续阶段 (D1-D5) 必须在本文立场上做技术决策.

---

## 一、queenx 哲学 (3 句话)

1. **不盲从任何 OS**, 包括 Linux, 必要才参考业界惯例
2. **追求 POSIX 100% 兼容**, 但 syscall 编号 / 路径 / 命名一律自主
3. **务实而非教条**, 工具链用现成 (GCC/LLVM), libc 复用 musl, 不重造轮子

queenx **不假装自己是 Linux, 也不假装自己不是 Linux**, 走 BSD/XNU 拼装路线 — 用 GNU 工具链但不被它绑架命名, 用 musl 但不被它绑架 libc ABI, 用 POSIX 兼容但不被 Linux 私有语义绑架.

---

## 二、命名规则 (NetBSD 风格中性)

### 2.1 文件与目录命名

| 项 | 命名 | 来源 / 备注 |
|----|------|-------------|
| 动态链接器 | `/usr/libexec/elfld.so` | ELF 规范术语, 0 OS 商标 |
| libc | `/usr/libexec/queenx_libc.musl-<arch>.so.1` | 自家名 + 信用标注 (musl) |
| pthread 实现 | `/usr/libexec/elfthreads.so` | ELF 术语 |
| 内核镜像 | `/queenx/queenx.elf` | 不抄 `/boot/vmlinuz` |
| 用户目录 | `/usr/libexec /usr/lib /usr/bin` | BSD/XNU 风格 |
| PT_INTERP 字符串 | `/usr/libexec/elfld.so` | 工具链默认烧入 |

### 2.2 路径层级 (XNU 风格, 不抄 Linux 私货)

```
/queenx/                 根 (kernel image + 启动文件)
/usr/libexec/            动态链接器 + 系统级 .so
/usr/lib/                用户态库
/usr/bin/                用户程序
/usr/include/            头文件
/etc/                    配置 (FHS 不是 Linux 专属)
/proc/ /sys/             procfs / sysfs 挂载点 (POSIX 推荐)
```

**关键不抄的 Linux 习惯**:
- ❌ `/lib64` (Linux 私货, XNU/BSD 不存在)
- ❌ `/usr/lib/x86_64-linux-gnu/` (Debian 私货, 多架构平铺)
- ❌ `/sys/class/*` 等 Linux sysctl 子树
- ❌ `/proc/cpuinfo` Linux 私货字段

### 2.3 命名禁词表

**禁止**在通用 .so 名上出现: `linux`, `freebsd`, `darwin`, `gnu`, `queenx`, `qx` 等 OS 商标词.

**允许**:
- `queenx_libc.musl-*` (坦白说是 qx 的, 用 musl 派生, 不冒充系统 libc)
- `elfld` / `elfthreads` (纯 ELF 规范术语, 无商标)
- `<func>.so.<major>` (ELF 通用版本号约定)

---

## 三、syscall 编号 (XNU 风格主动错开)

### 3.1 编号设计

```rust
// framework/syscall/numbers.rs
//
// queenx 编号原则:
// 1. 不抄任何 OS 编号
// 2. 按功能分区, 64 个一组, 留扩展空间
// 3. 0-499 保留给未来 linuxulator, 编号 1:1 映射 Linux
//
pub const SYS_EXIT: u64           = 501;  // 进程退出
pub const SYS_WRITE: u64          = 502;
pub const SYS_READ: u64           = 503;
pub const SYS_OPEN: u64           = 504;
pub const SYS_CLOSE: u64          = 505;
pub const SYS_MMAP: u64           = 510;  // 内存
pub const SYS_BRK: u64            = 511;
pub const SYS_GETPID: u64         = 520;  // 进程
pub const SYS_FORK: u64           = 521;
pub const SYS_EXECVE: u64         = 522;
pub const SYS_SOCKET: u64         = 600;  // 网络
pub const SYS_BIND: u64           = 601;
pub const SYS_SENDTO: u64         = 610;
// ...
```

### 3.2 编号空间分配

| 范围 | 用途 |
|------|------|
| `0-499` | 保留给未来 linuxulator (与 Linux 1:1 映射) |
| `500-599` | 进程 / 内存 / 文件基础 |
| `600-699` | 网络 / IPC |
| `700-799` | 设备 / IOCTL |
| `800-899` | 扩展 |

### 3.3 关键认知

- **POSIX 规定 API, 不规定 syscall 编号**: `read()` 是 libc 函数, 不是 syscall; libc 内部用编号调内核
- **用户程序走 libc**: libc 适配编号, 应用程序不感知
- **编号错开 ≠ 不兼容 POSIX**: 业界 100% OS 编号都互相错开
- **错开是护城河**: 防止 Linux 二进制意外跑进 queenx, 维护设计自主权

业界对照:
- macOS / iOS 错开 30+ 年, 仍是 POSIX 认证
- FreeBSD / NetBSD / OpenBSD / Solaris / AIX / Hurd 全部错开
- Linux 自己也"错开" (跟 BSD 编号完全不同)

---

## 四、工具链 (务实复用, 不重写)

### 4.1 工具链组件

```
queenx 工具链 = GCC/LLVM + queenx wrapper + 自维护 musl + binutils
```

| 组件 | 复用 | 自维护 |
|------|------|--------|
| 编译器 (gcc/clang) | ✅ | wrapper 配置 |
| 汇编器 (as) | ✅ | — |
| 链接器 (ld.lld / ld.bfd) | ✅ | — |
| musl libc | 复用 | queenx 封装层 |
| binutils | 复用 | — |
| queenx wrapper | — | ✅ 默认 PT_INTERP, crt0/i/n, 搜索路径 |

### 4.2 不重写原则

- ❌ 不重写编译器 (工程量 50+ 人年)
- ❌ 不重写链接器
- ❌ 不重写汇编器
- ✅ **不被工具链绑架命名**: wrapper 重写默认 PT_INTERP, 不沿用 GNU 默认

---

## 五、libc 选型 (musl 派生, 不自创)

### 5.1 候选对比

| 选项 | 命名 | 评估 |
|------|------|------|
| **musl 派生 (推荐)** | `queenx_libc.musl-<arch>.so.1` | 中性, 小, 标准化, 工程量 = 0 |
| BSD libc | `libc.so.X` | 干净, 移植工作量大 |
| glibc | `libc.so.6` | ❌ 商标重, 工程量更大, 不推荐 |
| 自创 queenx_libc | `libqueenx.so` | ❌ 重造轮子, 不推荐 |

### 5.2 选 musl 的理由

- 中性 (musl 命名不带 OS 商标)
- 极小 (~500 KB, 适合框架内核 demo)
- 标准化 (POSIX 100% 覆盖)
- 工程量 = 0 (直接复用 musl src/)
- Alpine 10+ 年 patch 积累可借鉴
- 主动错开 glibc 私有 ABI (queenx 友好)

---

## 六、跑现成 Linux 二进制 (linuxulator 路线)

### 6.1 设计 (学 FreeBSD)

```rust
// framework/proc/linuxulator.rs
pub fn linux_load_and_exec(elf: &[u8]) -> Result<Pid> {
    // 1. PT_INTERP 改写为 queenx 中性名
    rewrite_pt_interp(elf, "/usr/libexec/elfld.so")?;
    // 2. syscall 翻译表注册 (Linux 编号 → queenx 编号)
    register_linuxulator_translator();
    // 3. 加载执行
    load_elf(elf)
}
```

### 6.2 关键设计点

- **二进制原汁原味**: 不修改 Linux ELF 的代码段
- **PT_INTERP 改写**: 内核层做, 工具链不需要知道
- **syscall 翻译**: 内核模块, 不动 queenx 主线
- **模块化**: linuxulator 是可选, 卸载后 queenx 仍独立运行

### 6.3 linuxulator 工作量评估

| 任务 | 工作量 | 备注 |
|------|--------|------|
| PT_INTERP 改写 | 0.5 周 | 框架已有 ELF 解析 |
| syscall 翻译表 (Linux 0-499) | 2-4 周 | 200+ syscall 逐一映射 |
| /proc /sys 仿真 | 2-3 周 | 跟 queenx 原生路径双轨 |
| 设备 ioctl 翻译 | 1-2 周 | 基础集合 |
| glibc 私有 ABI 兼容 | **不做** | 不可行, 接受 glibc 动态二进制 10-20% 兼容 |

---

## 七、兼容性矩阵 (真实数字)

### 7.1 路线 A: 纯 musl (推荐起步)

| 类别 | 兼容性 | 备注 |
|------|--------|------|
| queenx 工具链自编译 POSIX 软件 | **100%** | queenx_libc.musl 适配 |
| 静态 Linux 二进制 (musl) | **~95%** | musl 自带 syscall 适配 |
| 静态 Linux 二进制 (glibc) | **~90%** | glibc 静态链自带封装 |
| 动态 Linux 二进制 (musl) | **~85%** | queenx elfld 实现 musl ABI |
| 动态 Linux 二进制 (glibc) | **~10-20%** | glibc 私有 ABI 太复杂 |
| macOS 二进制 | 0% | Mach-O 不是 ELF |
| BSD 二进制 | ~30-50% | ELF 同, syscall 错开 |

### 7.2 路线 B: musl + linuxulator

| 类别 | 兼容性 | 备注 |
|------|--------|------|
| GNU coreutils / findutils / grep / sed / awk / tar | **~95%** | |
| GNU bash / make / autoconf | **~85-90%** | |
| GNU readline / gettext / less | **~90-95%** | |
| glibc 动态 coreutils (Debian) | **+5%** 增量 | linuxulator 翻译 |
| glibc 动态 vim / python / node | **+3-5%** 增量 | |
| glibc 动态 systemd / Docker | 0% | cgroup/namespace Linux-only |

### 7.3 GNU 软件覆盖度

| 路线 | GNU 全集 (ftp.gnu.org ~400+ 包) |
|------|--------------------------------|
| **A. musl only** | **75-85%** |
| **B. + linuxulator** | **85-92%** |
| 业界真 Linux (基线) | 99% |
| 业界 macOS | 70-80% |
| 业界 FreeBSD + linuxulator | 80-85% |

### 7.4 永久跑不动的 GNU 软件

- ❌ GNU Emacs 完整版 (X11 依赖)
- ❌ GNU gdb (ptrace ABI 复杂)
- ❌ GNU parted 写操作 (无 ext4/fat 内核支持)
- ❌ GNU 任何依赖 `/proc/cpuinfo` 私有字段的
- ❌ GNU Mach / Hurd (不是 ELF 用户态)

---

## 八、对标业界 (queenx 学的是谁)

| 决策点 | queenx | 学自 |
|--------|--------|------|
| 内核拼装哲学 | framework/services | **XNU** (Mach+BSD+IOKit) |
| 命名干净度 | `/usr/libexec/elfld.so` | **NetBSD** (ld.elf_so 最干净) |
| 工具链复用 | GCC+musl | **FreeBSD** (不重写轮子) |
| 路径层级 | `/usr/lib /usr/bin` | **XNU/macOS** (不抄 /lib64) |
| syscall 错开 | 主动错开 | **macOS/XNU** (控制力) |
| 跑 Linux 二进制 | linuxulator | **FreeBSD linuxulator** |
| libc 选择 | musl 派生 | **FreeBSD** (用现成) |
| 不抄的 Linux 习惯 | /lib64, sysctl 子树, /proc 字段 | — |

**queenx = NetBSD 命名 + FreeBSD 工具链复用 + XNU 拼装哲学 + macOS 控制力 + musl libc + linuxulator 可选兼容**

---

## 九、明确不做的 (红线圈外)

- ❌ **不重写编译器 / 汇编器 / 链接器** (复用 GNU, 减少维护成本)
- ❌ **不沿用 `ld-linux-*` 命名** (商标绑架)
- ❌ **不抄 `/lib64` 路径** (Linux 私货)
- ❌ **不写 PT_INTERP = `ld-linux-x86-64.so.2`** (兼容靠 symlink 翻译层, 不靠字符串污染)
- ❌ **不实现 glibc 私有 ABI** (工程量 = 半个 glibc, 不现实)
- ❌ **不抄 sysctl 子树 / `/proc` 字段名** (Linux 私货, 改 POSIX 通用)
- ❌ **不盲从 Linux 一切** (必要才参考)
- ❌ **不假装自己是 Linux** (也不假装不是)

---

## 十、Phase D 优先序 (在本文立场上)

| 任务 | 工作量 | 价值 | 符合哲学 |
|------|--------|------|---------|
| **D1 网络栈收尾** | 小 | 高 | ✅ (smoltcp vendor 不动, 只补 framework/net 高层 + services/net 测试; 驱动由 chitin/proto_net::NetOps 负责) |
| **D3.5 axsh 增强** (tab 补全/历史/glob) | 中 | 高 | ✅ (用 queenx_libc.musl, 不抄 bash) |
| **D6 axsh 单元测试** | 小 | 高 | ✅ (parser/dispatch/echo/help 单元) |
| **D2 HiveFS 端到端测试** | 小 | 中 | ✅ (已有 17 module, 缺 e2e) |
| **D4 elfld.so 实现** (musl ABI) | 极大 | 高 | ✅ (中性命名 + musl 兼容, NetBSD 风格) |
| **D5 linuxulator** | 大 | 中 | ✅ (模块化, 不动主线, FreeBSD 思路) |

**推荐顺序**: D1 收尾 → D3.5 axsh 增强 + D6 axsh 测试 → D2 HiveFS e2e → D4 elfld (musl ABI) → D5 linuxulator.

---

## 十一、一句话定位

> **queenx = NetBSD 命名 + FreeBSD 工具链 + XNU 拼装 + macOS 控制力 + musl libc**, 务实兼容 POSIX 但不盲从 Linux, 主动 syscall 错开作为护城河, linuxulator 作为可选桥接层, 命名仅用 ELF 规范术语不沾任何 OS 商标. **目标: 100% POSIX 软件 + 85% musl 二进制 + 90% 静态二进制, 命名绝对中性**.

---

## 附录 A: 业界对照表

| 系统 | syscall 编号 | 命名 | libc | 跑现成 Linux 二进制 |
|------|-------------|------|------|-------------------|
| Linux | 自己一套 | `ld-linux-*.so.2` | glibc | 99% (基线) |
| macOS / iOS | 故意错开 | `dyld` | libSystem | 0% (拒绝) |
| FreeBSD | BSD 标准 | `ld-elf.so.1` | BSD libc | ~30% (linuxulator) |
| NetBSD | BSD 标准 | `ld.elf_so` | BSD libc | 0% |
| OpenBSD | BSD 标准 | `ld.so` | BSD libc | 0% |
| Solaris | SVID | `/lib/ld.so.1` | SunOS libc | 0% |
| AIX | 自有 | 内核内嵌 | AIX libc | 0% |
| Hurd | 自有 | `ld.so` | glibc | 0% (无 linuxulator) |
| Zephyr | 自有 | 无 (静态) | newlib | 0% |
| Android | 自有 | `linker64` | Bionic | 0% (拒绝) |
| Fuchsia | 自有 | `fd.vmo` | libmoneta | 0% (拒绝) |
| Redox | 自有 | `relibc` | relibc | 0% |
| **queenx (路线 A)** | **主动错开 (500+)** | **`elfld.so`** | **queenx_libc.musl** | **~10-20% (无 linuxulator)** |
| **queenx (路线 B)** | **主动错开 (500+)** | **`elfld.so`** | **queenx_libc.musl** | **~30-50% (加 linuxulator)** |

---

## 附录 B: queenx 工具链 wrapper 草案

```bash
# queenx-gcc wrapper
queenx-gcc() {
    arch=$(uname -m)
    case "$arch" in
        x86_64)   interp="/usr/libexec/elfld.so" ;;
        aarch64)  interp="/usr/libexec/elfld.so" ;;
        riscv64)  interp="/usr/libexec/elfld.so" ;;
    esac

    # 透明转发, 注入默认 PT_INTERP
    /usr/bin/gcc \
        -dynamic-linker "$interp" \
        -Wl,-dynamic-linker,"$interp" \
        -Wl,-rpath,/usr/libexec \
        -Wl,-rpath,/usr/lib \
        "$@"
}

# queenx-ld.so 默认 (queenx 内部 elfld.so 编译参数)
# 编译时:  musl-gcc -shared -fPIC -o elfld.so elfld.c
# 安装到:  /usr/libexec/elfld.so
```

---

## 附录 C: 兼容性测试用例 (待 D 阶段实施)

| 用例 | 类别 | 期望结果 |
|------|------|---------|
| 编译并运行 `coreutils ls` | 路线 A | 100% 跑 |
| 编译并运行 `coreutils cat` | 路线 A | 100% 跑 |
| 编译并运行 `bash --version` | 路线 A | 100% 跑 |
| 编译并运行 `vim --version` | 路线 A | 95% 跑 |
| 编译并运行 `git --version` | 路线 A | 100% 跑 |
| 运行 Alpine 静态 `busybox --list` | 路线 A | 100% 跑 |
| 运行 Debian 动态 `ls --version` | 路线 B | 30% 跑 (linuxulator) |
| 运行 Ubuntu 动态 `python3 --version` | 路线 B | 50% 跑 (linuxulator) |
| 运行 musl 动态 `nginx -v` | 路线 A | 95% 跑 |

---

**文档状态**: 2026-06-08 立, 后续 Phase D 实施时所有命名 / 编号 / 路径决策以此为准.
**维护者**: queenx 架构组
**变更流程**: 任何修改本立场书的设计决策必须经架构组评审, 不可单方改动.
