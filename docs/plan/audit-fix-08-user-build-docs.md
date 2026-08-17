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

- **lib.rs 模块结构注释不含 aarch64/chitin/wasm（H.4.11 P2-B）**
  - 描述：`src/rust/src/lib.rs` 的"模块结构"注释不含 aarch64 + chitin/wasm，与代码现状不符。
  - 方案：同步模块结构注释。
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

- **host-tests 与内核解耦根治（H.3.6 P0-26）**
  - 描述：host-tests 与内核完全解耦（838 passed 不反映内核状态），根因是"纯算法与平台机制未分离"——内核 no_std/裸机，host 侧无法引用内核源码，只能重建平行实现。经用户决策（DECISION-052），采用**路线 C 彻底根治**：内核 crate 增加 `host-test` feature + framework std 桩，host-tests 直接引用内核 services 真实源码，消除全部 7 处平行实现。
  - 方案：详见 [eliminate-parallel-implementations.md](./eliminate-parallel-implementations.md)（工程计划 A 宿主基建 / B framework std 桩 / C 迁移删除）；本条目作为该工程的承接登记。
  - 状态：[]
  - 详情：根治完成前，本文档其余 host-tests 相关条目的"标注覆盖映射表"仍为过渡手段。

- **host-tests/src/hvfs/ 平行实装差异登记（H.3.7 P0-27）**
  - 描述：实测 `host-tests/src/hvfs/`（19 文件 ~6,000 行）与内核 `services/fs/hvfs/`（29 文件 ~12,000 行）**不是同一实现的双份拷贝，而是两套独立实现的平行演化**。测试版不验证内核真实代码，838 项 host-tests 通过无法为内核 hvfs 提供正确性背书。
  - 方案：登记以下差异，作为合并实施（下条）的输入。
  - 状态：[]
  - 详情：
    - **架构差异**：内核版含 8 个 trait 抽象（arc/dmu/raidz/spa/txg/zap/zil/zil_persist `_trait.rs`，策略-机制分离，checksum 经 `Checksum` trait 支持 mock 注入）；测试版无 trait 层、拍平实现，核心为单一 `hvfs.rs`(1596 行)。内核版 `hvfs_data.rs`(1881) + `hvfs_inode.rs`(424) + `hvfs.rs`(47) 拆分；测试版集中单文件。
    - **磁盘布局不兼容（最严重）**：`HvDva`（块指针）字段序不同——内核版 `offset(u64), asize(u32), vdev_id(u16), gang(u8), _pad[1]`；测试版 `vdev_id(u16), offset(u64), asize(u32), gang(bool), _pad[3]`。字段序 + `gang` 类型（u8 vs bool）均不同，测试验证的磁盘格式与内核不兼容。
    - **功能缺失（测试版）**：缺 `hotplug_add_disk`/`hotplug_remove_disk`（热插拔）、`chown_ext`（与 credo 集成）、`zil_persist.rs`（ZIL 持久化）、`hvfs_inode.rs` 拆分。
    - **命名漂移**：`mount_drive`→`mount_disk`、`format_drive`→`format_disk`。
    - **实现漂移**：同名文件 diff 巨大——`bp.rs` 170 处、`dedup.rs` 153 处、`checksum.rs` 87 处；checksum 的 SHA-256 内核版走 `framework::credo::sha256`，测试版为独立实现。
    - **安全/合规**：内核版 `#![deny(unsafe_code)]`（0 unsafe）；测试版 `#![allow(unused_variables, unused_assignments)]`（违反 F9 零容忍）+ `ffi.rs` unsafe extern 垫片模拟内核 API。

- **host-tests/src/hvfs/ 合并回内核源码引用（H.3.7 P0-27 实施）**
  - 描述：消除平行双源，使 host-tests 直接引用内核 `services/fs/hvfs` 真实实现。**完成标准 = `host-tests/src/hvfs/` 下全部 19 个平行实现文件删除**（双源彻底消除），测试用例（tests/ 226 处引用）保留并改指内核实现。**不能简单 diff 合并**（两套架构不同），须以内核版（含 trait 层）为基准逐步对齐。
  - 方案：
    0. **前置依赖（阻塞项）**：内核 host 可编译基建（H.3.6 P0-26 根治，DECISION-052）——见 [eliminate-parallel-implementations.md](./eliminate-parallel-implementations.md) 工程计划 A/B；内核 hvfs 经 `host-test` feature 暴露 host 可编译入口，否则平行实现无法被替代、删除无从谈起；
    1. **先统一 `HvDva` 布局**：以内核版字段序为准（`offset, asize, vdev_id, gang(u8), _pad[1]`），否则布局依赖测试（块指针序列化）无意义；
    2. **迁移不变量类测试**：checksum 自洽、raidz 恢复、snapshot 语义等不依赖布局的测试，改为调用内核 API（经 host 可编译入口）；
    3. **对齐命名与 API**：测试版 `mount_disk`/`format_disk` 改回 `mount_drive`/`format_drive`，补齐 `hotplug_*`/`chown_ext` 的测试覆盖或显式标注缺失；
    4. **删除平行实现**：`host-tests/src/hvfs/` 19 文件随测试迁移完成逐批删除，删除前确认 tests/ 无残留 `queenx_host_tests::hvfs` 引用；`ffi.rs` 垫片同步清理；
    5. 桩机制处理：`hvfs_mock.rs` 的虚拟内核树**保留**（属测试基建，非被测对象），内核实现经其 kernel 树暴露；内核版 trait 抽象保持不动。
  - 状态：[]
  - 详情：若前置依赖（步骤 0）短期无法完成，**降级方案**为——为每个测试文件标注"覆盖内核模块 + 接线状态"，缺接线的显式标记；但删除平行实现仍是目标，不允许以"标注独立参考实现"替代，也不允许保留任何 `#![allow(unused_variables, unused_assignments)]` 豁免（违反 F9）。

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
