# queenx 命名与生态兼容立场书

> 2026-06-08 多轮讨论沉淀. 定义 queenx 内核在命名/syscall 编号/工具链/libc/路径/linuxulator 等关键设计点上的立场. 后续 Phase D 实施时所有命名/编号/路径决策以此为准.

## 一、queenx 哲学
- **核心立场 (3 句话)**
  - 描述: queenx 工程哲学的 3 个核心信条
  - 方案: (1) 不盲从任何 OS, 包括 Linux, 必要才参考业界惯例; (2) 追求 POSIX 100% 兼容, 但 syscall 编号/路径/命名一律自主; (3) 务实而非教条, 工具链用现成 (GCC/LLVM), libc 复用 musl, 不重造轮子
  - 状态: [X]
  - 详情: queenx 不假装自己是 Linux, 也不假装自己不是 Linux, 走 BSD/XNU 拼装路线 — 用 GNU 工具链但不被它绑架命名, 用 musl 但不被它绑架 libc ABI, 用 POSIX 兼容但不被 Linux 私有语义绑架

## 二、命名规则 (NetBSD 风格中性)
- **文件与目录命名**
  - 描述: 6 类核心路径命名 (NetBSD 风格中性, 0 OS 商标)
  - 方案: 动态链接器 `/usr/libexec/elfld.so` / libc `queenx_libc.musl-<arch>.so.1` / pthread `elfthreads.so` / 内核镜像 `/queenx/queenx.elf` / 用户目录 `/usr/libexec /usr/lib /usr/bin` / PT_INTERP `/usr/libexec/elfld.so`
  - 状态: [X]
  - 详情: elfld/elfthreads 是纯 ELF 规范术语无商标; queenx_libc.musl 坦白说是 qx 的, 不冒充系统 libc; 不抄 `/boot/vmlinuz`

- **路径层级 (XNU 风格, 不抄 Linux 私货)**
  - 描述: 7 类根路径 (XNU 风格, 不抄 Linux 私货)
  - 方案: `/queenx/` 根 (kernel image + 启动文件) + `/usr/libexec/` 动态链接器 + `/usr/lib/` 用户态库 + `/usr/bin/` 用户程序 + `/usr/include/` 头文件 + `/etc/` 配置 + `/proc/ /sys/` 挂载点
  - 状态: [X]
  - 详情: 关键不抄: ❌ `/lib64` (Linux 私货) / ❌ `/usr/lib/x86_64-linux-gnu/` (Debian 私货) / ❌ `/sys/class/*` 等 Linux sysctl 子树 / ❌ `/proc/cpuinfo` Linux 私货字段

- **命名禁词表**
  - 描述: 通用 .so 命名禁词 + 允许列表
  - 方案: 禁止 linux/freebsd/darwin/gnu/queenx/qx 等 OS 商标词; 允许 `queenx_libc.musl-*` (坦白说是 qx 的) / `elfld`/`elfthreads` (纯 ELF 规范术语) / `<func>.so.<major>` (ELF 通用版本号约定)
  - 状态: [X]

## 三、syscall 编号 (XNU 风格主动错开)
- **编号设计**
  - 描述: queenx syscall 编号设计 (XNU 风格主动错开)
  - 方案: (1) 不抄任何 OS 编号; (2) 按功能分区, 64 个一组, 留扩展空间; (3) 0-499 保留给未来 linuxulator, 编号 1:1 映射 Linux
  - 状态: [X]
  - 详情:

    ```rust
    pub const SYS_EXIT: u64   = 501;
    pub const SYS_WRITE: u64  = 502;
    pub const SYS_READ: u64   = 503;
    pub const SYS_OPEN: u64   = 504;
    pub const SYS_CLOSE: u64  = 505;
    pub const SYS_MMAP: u64   = 510;
    pub const SYS_BRK: u64    = 511;
    pub const SYS_GETPID: u64 = 520;
    pub const SYS_FORK: u64   = 521;
    pub const SYS_EXECVE: u64 = 522;
    pub const SYS_SOCKET: u64 = 600;
    pub const SYS_BIND: u64   = 601;
    pub const SYS_SENDTO: u64 = 610;
    ```

- **编号空间分配**
  - 描述: 5 段编号空间分配
  - 方案: 0-499 保留给未来 linuxulator (与 Linux 1:1 映射) / 500-599 进程+内存+文件基础 / 600-699 网络+IPC / 700-799 设备+IOCTL / 800-899 扩展
  - 状态: [X]

- **关键认知**
  - 描述: 4 个关键认知澄清
  - 方案: (1) POSIX 规定 API, 不规定 syscall 编号 (read() 是 libc 函数); (2) 用户程序走 libc, libc 适配编号; (3) 编号错开 ≠ 不兼容 POSIX; (4) 错开是护城河, 维护设计自主权
  - 状态: [X]
  - 详情: 业界对照: macOS/iOS 错开 30+ 年仍是 POSIX 认证; FreeBSD/NetBSD/OpenBSD/Solaris/AIX/Hurd 全部错开; Linux 自己也跟 BSD 编号完全不同

## 四、工具链 (务实复用, 不重写)
- **工具链组件**
  - 描述: 6 类组件复用策略
  - 方案: queenx 工具链 = GCC/LLVM + queenx wrapper + 自维护 musl + binutils. 编译器/汇编器/链接器/binutils 复用; musl 复用+queenx 封装层; queenx wrapper 自维护 (默认 PT_INTERP, crt0/i/n, 搜索路径)
  - 状态: [X]

- **不重写原则**
  - 描述: 4 条不重写原则
  - 方案: ❌ 不重写编译器 (50+ 人年) / ❌ 不重写链接器 / ❌ 不重写汇编器 / ✅ 不被工具链绑架命名 (wrapper 重写默认 PT_INTERP)
  - 状态: [X]

## 五、libc 选型 (musl 派生, 不自创)
- **候选对比**
  - 描述: 4 个候选评估 (推荐 musl 派生)
  - 方案: musl 派生 (中性, 小, 标准化, 工程量=0) / BSD libc (干净, 移植工作量大) / glibc (❌ 商标重) / 自创 queenx_libc (❌ 重造轮子)
  - 状态: [X]

- **选 musl 的理由**
  - 描述: 6 条选 musl 的理由
  - 方案: 中性 (命名不带 OS 商标) + 极小 (~500 KB, 适合框架内核 demo) + 标准化 (POSIX 100% 覆盖) + 工程量=0 (直接复用 musl src/) + Alpine 10+ 年 patch 积累可借鉴 + 主动错开 glibc 私有 ABI (queenx 友好)
  - 状态: [X]

## 六、跑现成 Linux 二进制 (linuxulator 路线)
- **设计 (学 FreeBSD)**
  - 描述: linuxulator 3 步设计
  - 方案: (1) PT_INTERP 改写为 queenx 中性名; (2) syscall 翻译表注册 (Linux 编号 → queenx 编号); (3) 加载执行. 二进制原汁原味, 不修改 Linux ELF 代码段
  - 状态: [X]

- **关键设计点**
  - 描述: 4 个关键设计点
  - 方案: (1) 二进制原汁原味不修改代码段; (2) PT_INTERP 改写内核层做; (3) syscall 翻译内核模块; (4) 模块化可选, 卸载后 queenx 仍独立运行
  - 状态: [X]

- **linuxulator 工作量评估**
  - 描述: 5 类任务工作量
  - 方案: PT_INTERP 改写 0.5 周 / syscall 翻译表 2-4 周 (200+ syscall) / /proc /sys 仿真 2-3 周 / 设备 ioctl 翻译 1-2 周 / glibc 私有 ABI 兼容 不做
  - 状态: []

## 七、兼容性矩阵 (真实数字)
- **路线 A: 纯 musl**
  - 描述: 路线 A 兼容性矩阵
  - 方案: queenx 工具链自编译 POSIX 软件 100% / 静态 Linux 二进制 (musl) ~95% / 静态 Linux 二进制 (glibc) ~90% / 动态 Linux 二进制 (musl) ~85% / 动态 Linux 二进制 (glibc) ~10-20% / macOS 二进制 0% / BSD 二进制 ~30-50%
  - 状态: [X]

- **路线 B: musl + linuxulator**
  - 描述: 路线 B 兼容性矩阵
  - 方案: GNU coreutils/findutils/grep/sed/awk/tar ~95% / GNU bash/make/autoconf ~85-90% / GNU readline/gettext/less ~90-95% / glibc 动态 coreutils (Debian) +5% 增量 / glibc 动态 vim/python/node +3-5% 增量 / glibc 动态 systemd/Docker 0%
  - 状态: [X]

- **GNU 软件覆盖度**
  - 描述: 路线 A vs B GNU 全集覆盖度
  - 方案: 路线 A 75-85% / 路线 B 85-92% / 业界真 Linux 99% (基线) / 业界 macOS 70-80% / 业界 FreeBSD+linuxulator 80-85%
  - 状态: [X]

- **永久跑不动的 GNU 软件**
  - 描述: 永久跑不动的 GNU 软件清单
  - 方案: ❌ GNU Emacs 完整版 (X11) / ❌ GNU gdb (ptrace ABI 复杂) / ❌ GNU parted 写操作 / ❌ GNU 任何依赖 /proc/cpuinfo 私有字段 / ❌ GNU Mach/Hurd
  - 状态: [X]

## 八、对标业界 (queenx 学的是谁)
- **对标矩阵**
  - 描述: queenx 8 类决策点对标业界
  - 方案: 内核拼装 → XNU (Mach+BSD+IOKit) / 命名干净度 → NetBSD / 工具链复用 → FreeBSD / 路径层级 → XNU/macOS / syscall 错开 → macOS/XNU / 跑 Linux 二进制 → FreeBSD linuxulator / libc → musl (用现成) / 不抄 Linux 习惯 → 自定义
  - 状态: [X]
  - 详情: queenx = NetBSD 命名 + FreeBSD 工具链复用 + XNU 拼装哲学 + macOS 控制力 + musl libc + linuxulator 可选兼容

## 九、明确不做的 (红线圈外)
- **红线圈外清单**
  - 描述: 8 条明确不做的设计决策
  - 方案: ❌ 不重写编译器/汇编器/链接器 / ❌ 不沿用 ld-linux-* 命名 (商标绑架) / ❌ 不抄 /lib64 / ❌ 不写 PT_INTERP = ld-linux-x86-64.so.2 / ❌ 不实现 glibc 私有 ABI / ❌ 不抄 sysctl 子树 /proc 字段名 / ❌ 不盲从 Linux 一切 / ❌ 不假装自己是 Linux
  - 状态: [X]

## 十、Phase D 优先序 (在本文立场上)
- **D 任务优先序**
  - 描述: Phase D 6 个任务工作量/价值/符合哲学评估
  - 方案: D1 网络栈收尾 (小/高/✅) / D3.5 eash 增强 (中/高/✅) / D6 eash 单元测试 (小/高/✅) / D2 HiveFS 端到端测试 (小/中/✅) / D4 elfld.so 实现 (极大/高/✅) / D5 linuxulator (大/中/✅)
  - 状态: [X]
  - 详情: 推荐顺序: D1 收尾 → D3.5 eash 增强 + D6 eash 测试 → D2 HiveFS e2e → D4 elfld (musl ABI) → D5 linuxulator

## 十一、一句话定位
- **一句话定位**
  - 描述: queenx 工程定位
  - 方案: queenx = NetBSD 命名 + FreeBSD 工具链 + XNU 拼装 + macOS 控制力 + musl libc, 务实兼容 POSIX 但不盲从 Linux, 主动 syscall 错开作为护城河, linuxulator 作为可选桥接层
  - 状态: [X]
  - 详情: 命名仅用 ELF 规范术语不沾任何 OS 商标. 目标: 100% POSIX 软件 + 85% musl 二进制 + 90% 静态二进制, 命名绝对中性

## 附录 A: 业界对照表
- **业界 11 个 OS 对照**
  - 描述: 业界 11 个 OS 在 syscall 编号/命名/libc/跑现成 Linux 二进制 4 维度对照
  - 方案: Linux/自己一套/ld-linux-*.so.2/glibc/99%; macOS-iOS/故意错开/dyld/libSystem/0%; FreeBSD/BSD 标准/ld-elf.so.1/BSD libc/~30%; NetBSD/BSD 标准/ld.elf_so/BSD libc/0%; OpenBSD/BSD 标准/ld.so/BSD libc/0%; Solaris/SVID/ld.so.1/SunOS libc/0%; AIX/自有/内核内嵌/AIX libc/0%; Hurd/自有/ld.so/glibc/0%; Zephyr/自有/无(newlib)/0%; Android/自有/linker64/Bionic/0%; Fuchsia/自有/fd.vmo/libmoneta/0%; Redox/自有/relibc/relibc/0%; queenx 路线 A/主动错开(500+)/elfld.so/queenx_libc.musl/~10-20%; queenx 路线 B/主动错开(500+)/elfld.so/queenx_libc.musl/~30-50%
  - 状态: [X]

## 附录 B: queenx 工具链 wrapper 草案
- **queenx-gcc wrapper**
  - 描述: 透明转发 + 注入 PT_INTERP 的 wrapper
  - 方案: 根据架构 (x86_64/aarch64/riscv64) 选 interp=/usr/libexec/elfld.so, 然后透明转发到 /usr/bin/gcc, 注入 -dynamic-linker + -Wl,-rpath,/usr/libexec 和 /usr/lib
  - 状态: [X]
- **elfld.so 编译**
  - 描述: queenx 内部 elfld.so 编译参数
  - 方案: 编译 musl-gcc -shared -fPIC -o elfld.so elfld.c; 安装到 /usr/libexec/elfld.so
  - 状态: [X]

## 附录 C: 兼容性测试用例
- **9 类测试用例 (待 D 阶段实施)**
  - 描述: 兼容性测试用例 9 个, 按路线 A/B 分类
  - 方案: 路线 A: 编译并运行 coreutils ls/cat/git 100% / bash 100% / vim 95% / Alpine 静态 busybox 100%; 路线 B: Debian 动态 ls 30% / Ubuntu 动态 python3 50% / musl 动态 nginx 95%
  - 状态: []

## 维护规则
- **维护者**
  - 描述: 文档维护组
  - 方案: queenx 架构组
  - 状态: [X]
- **变更流程**
  - 描述: 任何修改本立场书的设计决策流程
  - 方案: 必须经架构组评审, 不可单方改动
  - 状态: [X]

## 变更历史
- **2026-06-26**
  - 描述: 按新文档规则重写 (标题+条目(描述+方案+状态)+详情)
  - 方案: 结构重组, 保留原意
  - 状态: [X]
