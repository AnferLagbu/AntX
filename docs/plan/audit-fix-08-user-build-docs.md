# 审计修复分册 08：用户态、构建、文档与测试

> 修复 src/user（链接脚本/汇编）、build（stage1.bin/build.rs）、src/rust 布局、docs（ref-naming）、tests（陈旧日志）与 host-tests（解耦/平行实装）的审计缺陷。来源：[code-audit-final-summary.md](./code-audit-final-summary.md) 第 3.5 节 + 附录 H（H.3.6/H.3.7/H.4.6/H.5.3）+ 附录 E。

## 工程计划 A: 构建与源码布局

### 背景

- **构建产物与布局异常**
  - 描述：stage1.bin 全 0x00、build.rs 主动生成全 0 占位符、src/rust/lib.rs 空文件共存。
  - 方案：核实产物来源，删除占位符，显式声明 lib 路径。
  - 状态：[]

### 待办

- **build/stage1.bin 全 0x00（P0-18）**
  - 描述：`build/stage1.bin`（440 字节）hexdump 全 0，multiboot2 头缺失；Makefile:217-218 依赖 `src/kernel/framework/boot/stage1.asm`。
  - 方案：核实 stage1.asm 汇编产物；与 H.5.3（build.rs 全 0 占位）联动确认 stage1.bin 是否为 build.rs 伪造；若 unused 则删除。
  - 状态：[]

- **src/rust/build.rs 全 0 占位符（H.5.3 P0-33）**
  - 描述：`src/rust/build.rs` 主动创建全 0x00 占位符（P0-18 的 stage1.bin 可能由此产生）。
  - 方案：改为 `panic_missing` 或真实构建逻辑（DECISION-H15）。
  - 状态：[]

- **src/rust/lib.rs 空文件（P0-19）**
  - 描述：`src/rust/lib.rs`（0 字节）与 `src/rust/src/lib.rs`（33KB）共存，Cargo 解析路径依赖 target-dir/manifest。
  - 方案：删除空文件，显式 `[lib] path = "src/lib.rs"`。
  - 状态：[]

- **用户态链接脚本 _user_start/_user_end（P0-17）**
  - 描述：`src/user/link.x`、`link_aarch64.x`、`init/link_aarch64.x` 均无 `_user_start/_user_end` 边界符号，ELF loader 无法获取用户进程内存边界。
  - 方案：见分册 02 工程计划 A F-04（KPTI 布局）一并实施；本分册负责 ELF loader 侧消费验证。
  - 状态：[]

- **src/user/init/src/arch/aarch64.S 死代码（H.4.6 P1-C）**
  - 描述：aarch64.S 死代码。
  - 方案：核实引用后删除或实装使用路径。
  - 状态：[]

## 工程计划 B: 文档与测试目录

### 背景

- **文档漂移 + 陈旧产物**
  - 描述：ref-naming.md 立场与代码不符、tests/reports 164 个陈旧日志散落。
  - 方案：文档立场修正 + 仓库清理。
  - 状态：[]

### 待办

- **ref-naming.md 500+ 立场与代码不符（P0-20）**
  - 描述：[ref-naming.md:48-49](file:///home/anfer/Code/QueenX/docs/explain/ref-naming.md#L48-L49) 称 QueenX 私有扩展 500+，但用户态 sys.rs 实际 SYS_CREDO_* 在 400-437，内核态 types.rs 在 700+。
  - 方案：编号统一后（分册 05 DECISION-050）同步修正 ref-naming.md 表述。
  - 状态：[]

- **tests/reports/ 陈旧日志清理（P0-21）**
  - 描述：`tests/reports/` 散落 164 个 .log（含 6 个 driver 报告子目录），历史上曾误提交。
  - 方案：本地清理 + `.gitignore` 追加 `tests/reports/**/*.log` 强约束；远程已跟踪的用 `git rm --cached`。
  - 状态：[]

## 工程计划 C: host-tests 与测试基建

### 背景

- **host-tests 与内核解耦**
  - 描述：host-tests 与内核完全解耦（P0-26），且 host-tests/src/hvfs/ 平行实装使缺陷隐性双倍严重（P0-27）。
  - 方案：建立解耦声明与覆盖映射，消除平行实装。
  - 状态：[]

### 待办

- **host-tests 与内核解耦处理（H.3.6 P0-26）**
  - 描述：host-tests 与内核完全解耦，测试覆盖率虚标（838 passed 不反映内核状态）。
  - 方案：每份 host-test 标注覆盖的内核模块与真实接线；建立内核↔测试映射表；缺接线的测试显式标记。
  - 状态：[]

- **host-tests/src/hvfs/ 平行实装消除（H.3.7 P0-27）**
  - 描述：host-tests/src/hvfs/ 平行实装使 G.4 P0-29/30/31 隐性双倍严重。
  - 方案：平行实装合并回内核源码引用（消除双源），或标注为独立参考实现。
  - 状态：[]

- **Makefile 跨架构清理（ISSUE-TOOL-001）**
  - 描述：`build/boot.o` 残留上次 aarch64 产物导致 x86_64 链接报错。
  - 方案：Makefile `all` 目标自动清理异架构产物，或加 `make clean-arch`。
  - 状态：[]

- **kernel.flat 陈旧未自动重建（ISSUE-TOOL-002）**
  - 描述：lint 修复后旧 kernel.flat 仍存在，QEMU 启动日志为空。
  - 方案：Makefile 加文件 mtime 检查，或 QEMU 脚本加图像陈旧检测。
  - 状态：[]

### 验证门槛

- **构建回归**
  - 描述：build.rs/lib.rs/link 脚本改动后跑 `./ci/build.sh all`。
  - 方案：`./ci/build.sh all` + `make test-host`。
  - 状态：[]

- **文档同步**
  - 描述：ref-naming.md 修正后与代码编号一致。
  - 方案：grep 验证 sys.rs/types.rs/ref-naming.md 三源一致。
  - 状态：[]
