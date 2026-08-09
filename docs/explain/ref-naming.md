# queenx 命名与生态兼容立场书

> 2026-06-08 初版，2026-07-05 修订。定义 queenx 内核在命名/syscall 编号/工具链/libc/路径等关键设计点上的立场。直接实现 Linux ABI 作为务实路径。

## 一、queenx 哲学
- **核心立场（3 句话）**
  - 描述：queenx 工程哲学的 3 个核心信条
  - 方案：(1) 不盲从任何 OS，包括 Linux，必要才参考业界惯例；(2) 直接实现 Linux ABI 以获得最大兼容性，内核内部实现保持 Rust 纯净；(3) 务实而非教条，工具链用现成（GCC/LLVM），libc 复用 glibc/musl，不重造轮子
  - 状态：[X]
  - 详情：queenx 内核 100% Rust（framework unsafe + services safe），但 ABI 层面直接兼容 Linux。学 Asterinas 路线：内核实现与 ABI 兼容性不冲突，syscall 编号是 ABI 约定而非内核实现细节

## 二、命名规则（NetBSD 风格中性）
- **文件与目录命名**
  - 描述：核心路径命名（NetBSD 风格中性，0 OS 商标）
  - 方案：动态链接器 `/lib64/ld-linux-x86-64.so.2`（Linux 标准）/ libc `glibc` 或 `musl`（直接复用）/ 内核镜像 `/boot/kernel.bin` / 用户目录 `/bin /usr/bin /usr/lib /etc`（Linux 标准）
  - 状态：[X]
  - 详情：采用 Linux 标准路径，不自创路径。与 Asterinas 一致：直接兼容比自建命名更有价值

- **路径层级**
  - 描述：采用 Linux 标准路径层级
  - 方案：`/bin/` + `/usr/bin/` + `/usr/lib/` + `/usr/lib64/` + `/etc/` + `/proc/` + `/sys/` + `/dev/` — 与 Linux 完全一致
  - 状态：[X]

- **命名禁词表**
  - 描述：通用 .so 命名约束
  - 方案：内核内部代码禁止 OS 商标词；用户态文件名直接使用 Linux 标准命名（ld-linux-*.so.2, libc.so.6）
  - 状态：[X]

## 三、syscall 编号（直接 Linux ABI）
- **编号设计**
  - 描述：直接使用 Linux syscall 编号，无需翻译层
  - 方案：(1) 标准 POSIX/Linux syscall 使用 Linux 原始编号（0-299）；(2) QueenX 私有扩展使用 500+ 编号；(3) 无需 linuxulator 翻译层
  - 状态：[X]
  - 详情：

    ```rust
    // Linux 标准编号（直接使用）
    pub const SYS_READ: u64    = 0;
    pub const SYS_WRITE: u64   = 1;
    pub const SYS_OPEN: u64    = 2;
    pub const SYS_CLOSE: u64   = 3;
    pub const SYS_MMAP: u64    = 9;
    pub const SYS_BRK: u64     = 12;
    pub const SYS_GETPID: u64  = 39;
    pub const SYS_FORK: u64    = 57;
    pub const SYS_EXECVE: u64  = 59;
    pub const SYS_SOCKET: u64  = 41;

    // QueenX 私有扩展（500+）
    pub const QX_CAPABILITY: u64 = 500;
    // ...
    ```

- **编号空间分配**
  - 描述：2 段编号空间
  - 方案：0-299 Linux 标准（直接兼容）/ 500+ QueenX 私有扩展（能力系统/弹性恢复等）
  - 状态：[X]

- **关键认知**
  - 描述：3 个关键认知澄清
  - 方案：(1) Asterinas 已验证"内核 100% Rust + ABI 完全兼容 Linux"可行；(2) syscall 编号是 ABI 约定，不影响内核内部 Rust 实现；(3) 直接 ABI 比翻译层更简单、更可靠
  - 状态：[X]
  - 详情：业界对照：Asterinas 直接使用 Linux 编号（240+ syscall）/ Redox 使用自有编号但生态有限 / FreeBSD linuxulator 需要翻译层

## 四、工具链（务实复用，不重写）
- **工具链组件**
  - 描述：复用标准 Linux 工具链
  - 方案：GCC/LLVM + binutils + glibc/musl — 直接使用 Linux 标准工具链，无需 wrapper
  - 状态：[X]

- **不重写原则**
  - 描述：3 条不重写原则
  - 方案：❌ 不重写编译器 / ❌ 不重写链接器 / ❌ 不重写汇编器
  - 状态：[X]

## 五、libc 选型（直接复用，不派生）
- **选型决策**
  - 描述：直接使用 glibc 或 musl，不派生
  - 方案：Linux 二进制自带 glibc 依赖，直接提供 glibc 运行时；QueenX 原生程序可用 musl 静态编译；不需要 queenx_libc 派生
  - 状态：[X]
  - 详情：直接 ABI 兼容意味着不需要自定义 libc。Linux 的 glibc/musl 在 QueenX 上直接运行，因为 syscall 编号一致

## 六、Linux 应用兼容（直接 ABI）
- **设计（学 Asterinas）**
  - 描述：直接 ABI 兼容，无需翻译层
  - 方案：(1) 实现 Linux syscall 接口（编号一致）；(2) 提供 Linux 标准文件系统（ext2/procfs/devfs）；(3) 支持 Linux ELF 格式（PT_INTERP 直接加载 ld-linux.so）
  - 状态：[X]

- **关键设计点**
  - 描述：3 个关键设计点
  - 方案：(1) 二进制原汁原味，无需修改/重编译；(2) 无需 PT_INTERP 改写；(3) 无需 syscall 翻译层
  - 状态：[X]

- **不实现的（红线）**
  - 描述：明确不做的 Linux 兼容
  - 方案：❌ 不实现 glibc 私有 ABI（如 __堆栈_chk_fail）/ ❌ 不支持 Linux 内核模块加载 / ❌ 不抄 /proc/cpuinfo 私有字段
  - 状态：[X]

## 七、兼容性矩阵（直接 ABI）
- **直接 ABI 兼容性**
  - 描述：直接 Linux ABI 的预期兼容性
  - 方案：Linux 静态二进制（glibc）~95% / Linux 静态二进制（musl）~98% / Linux 动态二进制（glibc）~90% / Linux 动态二进制（musl）~95% / POSIX 软件 100% / macOS 二进制 0% / BSD 二进制 ~30-50%
  - 状态：[X]

- **永久不兼容的**
  - 描述：永远无法兼容的 Linux 软件
  - 方案：❌ 依赖 Linux 内核模块的软件 / ❌ 依赖 glibc 私有 ABI 的软件 / ❌ 依赖 /proc/cpuinfo 私有字段的软件 / ❌ systemd/Docker（深度依赖 Linux 内核接口）
  - 状态：[X]

## 八、对标业界（学 Asterinas）
- **对标矩阵**
  - 描述：queenx 决策对标业界
  - 方案：内核纯 Rust → Asterinas / 直接 Linux ABI → Asterinas / 文件系统 → Linux 标准（ext2/procfs/devfs）/ 命名中性 → 保留（不影响兼容性）/ libc 直接复用 → Asterinas
  - 状态：[X]
  - 详情：queenx = Asterinas 的 Rust 内核哲学 + 直接 Linux ABI + 中性命名保留

## 九、明确不做的（红线圈外）
- **红线圈外清单**
  - 描述：明确不做的设计决策
  - 方案：❌ 不重写编译器/汇编器/链接器 / ❌ 不实现 glibc 私有 ABI / ❌ 不支持 Linux 内核模块 / ❌ 不自建动态链接器（直接用 ld-linux.so）/ ❌ 不自建 libc（直接用 glibc/musl）
  - 状态：[X]

## 十、Phase D 优先序（修订）
- **D 任务优先序**
  - 描述：Phase D 任务优先序（直接 ABI 路径）
  - 方案：D1 网络栈收尾（已完成）/ D2 HiveFS e2e 测试（已完成）/ D3.5 eash 增强（已完成）/ D6 eash 测试（已完成）/ 实现 240+ Linux syscall（核心任务）/ 提供 glibc 运行时（关键）
  - 状态：[X]
  - 详情：核心路径：实现 Linux syscall → 提供 glibc → Linux 二进制直接运行

## 十一、一句话定位
- **一句话定位**
  - 描述：queenx 工程定位
  - 方案：queenx = 100% Rust 内核 + 直接 Linux ABI + 中性命名。内核内部实现保持 Rust 纯净，ABI 层面完全兼容 Linux，实现 Asterinas 级别的应用兼容性
  - 状态：[X]

## 附录 A: 业界对照表（修订）
- **业界 12 个 OS 对照**
  - 描述：业界 OS 在 syscall 编号/命名/libc/跑 Linux 二进制 4 维度对照
  - 方案：Linux/自己一套/ld-linux-*.so.2/glibc/99%；Asterinas/直接 Linux ABI/ld-linux-*.so.2/glibc/95%+；macOS/错开/dyld/libSystem/0%；FreeBSD/BSD 标准/ld-elf.so.1/BSD libc/~30%；NetBSD/BSD 标准/ld.elf_so/BSD libc/0%；OpenBSD/BSD 标准/ld.so/BSD libc/0%；Android/自有/linker64/Bionic/0%；Fuchsia/自有/fd.vmo/libmoneta/0%；Redox/自有/relibc/relibc/0%；queenx/直接 Linux ABI/ld-linux-*.so.2/glibc/90%+
  - 状态：[X]

## 附录 B: 兼容性测试用例（修订）
- **直接 ABI 测试用例**
  - 描述：直接 ABI 兼容性测试
  - 方案：Linux 静态 coreutils ls/cat/ls 100% / Linux 静态 bash 100% / Linux 动态 python3 90% / Linux 动态 nginx 95% / Alpine 静态 busybox 100%
  - 状态：[]

## 维护规则
- **维护者**
  - 描述：文档维护组
  - 方案：queenx 架构组
  - 状态：[X]
- **变更流程**
  - 描述：任何修改本立场书的设计决策流程
  - 方案：必须经架构组评审，不可单方改动
  - 状态：[X]
