# AntX 已知问题与待解决项

> 最后更新: 2026-06-28

---

## ⚠️ 未解决问题 (3项)

### 4. HvFS get_obj_mut 返回拷贝致修改无声丢失 (P1) 🆕

**状态**: 🟡 已定位，待修复

**根因**: `dmu.rs:HvDmuObjSet::get_obj_mut()` → `get_obj()` → `.cloned()`
返回的是 `HvDmuObject` 的**拥有副本**，而非可变引用。`hvfs.rs` 中 5 处调用点修改该副本后原对象不更新。

**影响函数**: `symlink`, `link`, `setxattr`, `getxattr` (对象元数据修改不持久化)

**正确用法**: 修改后须调用 `ds.objset.update_obj(&obj)` 将副本写回

**涉及文件**:
- `kernel/fs/hvfs/dmu.rs:L218` — get_obj_mut 定义 (别名, 不提供真正 mutable 访问)
- `kernel/fs/hvfs/hvfs.rs:L804,819,881,977,1104` — 5 处调用点 (均有 FIXME 标注)

**修复方案**:
1. 将 `get_obj_mut` 改为返回 `Option<&mut HvDmuObject>` (需要改锁模型)
2. 或在每个调用点添加 `ds.objset.update_obj(&obj)` 调用

---

### 3. HvFS 磁盘挂载路径未经验证 (P1)

**状态**: 🟡 代码已写，端到端未测试

**问题链**:
- `sys_disk_format(disk_id)` → `hvfs.format_disk()` — 是否真的向磁盘 #0 写入 VDEV label？
- `sys_boot_install` 写 config sector (LBA 2046, `"ANTX"` + hvfs_lba)
- 重启后 `kernel_init()` 读 LBA 2046 → `hvfs.spa.disk_present = true` → `hvfs.init()` → mount `/`
- `hvfs.init()` 的 SPA 层是否知道从磁盘 #0 读取 VDEV？

**涉及文件**:
- `syscall/mod.rs:L589` — `sys_boot_install` (config sector 写入)
- `syscall/mod.rs:L548` — `sys_disk_format`
- `fs/vfs/ffi.rs:L64` — `vfs_mount_internal` HvFS 分支
- `fs/hvfs/spa.rs:L189` — `spa.init()` VDEV 扫描
- `fs/hvfs/hvfs.rs:L118` — `hvfs.init()`
- `lib.rs:L207` — 磁盘引导检测

**验证清单**:
- [ ] format → 立即回读校验
- [ ] `disk_present=true` → `hvfs.init()` → 打开磁盘 VDEV
- [ ] 安装向导写 `/mnt/.antx_installed` → umount → mount → 文件存在
- [ ] 完整安装 → 重启 → 内核自动 mount → 跳过向导

---

---

## ✅ 已解决问题 (10项)

### 1. lwIP `lwip_init()` 间歇性卡死内核启动 (P0)
**日期**: 2026-05-19
**根因**: timer ISR 在 `lwip_init()` 半初始化状态闯入，访问未初始化的 `netif_list`/`tcp_pcbs`
**修复**: 在 `lwip_init()` 和 `qx_netif_register_e1000()` 前后添加 `cli`/`sti` 临界区保护，并在 `sti` 之前设置 `NET_READY = true`；恢复 `kernel_init()` 中的网络初始化调用
**涉及文件**: `net/init.rs`, `lib.rs`

---

### 2. 用户态 init 进程进入 Ring 3 后无任何输出 (P0)
**日期**: 2026-05-19
**根因**: `pmm_alloc_pages()` 返回物理地址，但 `user_proc.rs` 和 `process.rs` 将其直接作为虚拟地址存储为内核栈指针（TSS RSP0）。用户页表仅映射高半区内核空间（PML4[256..511]），物理地址在用户页表中不可访问。当用户态进程触发中断时，CPU 通过 TSS RSP0 切换内核栈失败 → Triple Fault
**修复**:
1. `user_proc.rs::create()` — 内核栈地址加 `KERNEL_BASE` 转换为高半区虚拟地址
2. `user_proc.rs::destroy()` — 释放时减去 `KERNEL_BASE` 还原物理地址
3. `process.rs::allocate_kernel_stack()` — 同样加 `KERNEL_BASE` 转换
4. `user_proc.rs::enter()` — 合并两个 `asm!` 块为一个，防止编译器在 CR3 切换和 iretq 之间插入栈操作
**涉及文件**: `proc/user_proc.rs`, `proc/process.rs`

---

### 4. install crate 路径硬编码 (P2)
**日期**: 2026-05-19
**修复**: 提取 `TARGET_PREFIX` 为配置参数，`MANIFEST` 中 `dst` 改为 `dst_rel`（相对路径），`build_dst()` 在运行时拼接前缀+相对路径
**涉及文件**: `install/src/wizard/deploy.rs`

---

### 5. 系统无 panic 回溯/调试信息 (P2)
**日期**: 2026-05-19
**修复**: panic handler 中添加 16 个通用寄存器 dump（RAX-R15）+ CR2/CR3 输出，通过串口直接输出
**涉及文件**: `lib.rs`, `klog/mod.rs`（`serial_write_bytes` 改为 `pub`，`KLOG_INIT` 改为 `pub`）

---

### 6. `KERNEL_TEST_OBJS` 缺少 `user_init_bin.o` → 测试链接失败
**提交**: `4c2faf3` | **日期**: 2026-05-18

---

### 7. 网络初始化顺序错误 → 系统启动挂死
**提交**: `04a3c28` | **日期**: 2026-05-18
**根因**: `lwip_init()` 在 `sys_init()` 之前调用，OSAL 未初始化
**修复**: 重排 `e1000_probe → sys_init → lwip_init → netif_register`
**注意**: 此修复后仍有 timer ISR 竞态 (问题 #1)

---

### 8. `make test-host` 日志目录缺失 + tee 非零退出码
**提交**: `04a3c28` | **日期**: 2026-05-18

---

### 9. `gen_embed.py` + `embedded/` 冗余嵌入工具链
**提交**: `1e32381` | **日期**: 2026-05-19
**删除**: gen_embed.py (55行) + user_init_bin.c (1177行) + Makefile规则 (19行) = **-1251行**
**替换**: 2 个 Rust `include_bytes!` 调用

---

### 10. Makefile: `$(RUST_LIB)` 依赖缺失 → clean build 失败
**提交**: `30b0f20` | **日期**: 2026-05-19
**修复**: `$(RUST_LIB): build/user/init.bin $(STAGE1_BIN)`

---

### 11. 用户态目录结构重组为 workspace
**提交**: `073eaef`, `41091db`, `b1b353b` | **日期**: 2026-05-19
**重组**: `src/user/{lib,init,axsh,install}` 4 crate workspace

---

### 12. 安装向导持久化流 — HvFS mount + /mnt 部署 + 磁盘引导检测
**提交**: `a4ceff7`, `3c95656` | **日期**: 2026-05-19
**核心**: prepare 后 `fs_mount("hvfs", "/mnt")` → 所有部署路径 `/mnt/...` → config sector `"ANTX"`

---

## 🔧 技术债务 (5项)

| 项 | 说明 | 优先级 |
|----|------|--------|
| `lib/fs.rs` `O_TRUNC` unused import | 仅 `deploy` 模块使用，导入在 `fs.rs` 顶层 | low |
| lwIP NO_SYS=1 单线程 | 迁移到 `tcpip_thread` 可根除问题 #1 的长期方案 | medium |
| VFS `/` 根目录依赖用户态 mount | 磁盘引导时需内核先挂 HvFS | medium |
| `axsh` help 文本与 BUILTINS 不同步 | 新命令需改两处 | low |
| `userlib::*` 全局导出 syscall | `pub use sys::*` 污染命名空间 | low |

---

## 📋 安装流端到端验证矩阵

| # | 步骤 | 预期 | 状态 |
|---|------|------|------|
| 1 | QEMU `-kernel -drive` | 内核到 `Entering Ring 3` | 🟢 内核栈地址修复后应正常 |
| 2 | init banner | `[init] AntX init process started` | 🟡 待验证 |
| 3 | 安装向导 welcome | `AntX Installation Wizard` | 🔴 |
| 4 | 磁盘探测/选择 | 1 个 64MB 盘, 选 0, yes | 🔴 |
| 5 | 分区/格式化 | 无 fatal error | 🔴 |
| 6 | HvFS mount /mnt | mount 成功 | 🔴 |
| 7 | 应用部署 | 4/4 OK | 🔴 |
| 8 | PWID 创建 | 密码确认 → 成功 | 🔴 |
| 9 | 主机名 | 输入 → 成功 | 🔴 |
| 10 | 安装完成 | 写标记 → sync → umount → reboot | 🔴 |
| 11 | 磁盘引导 | BIOS→Stage1→kernel→mount HvFS→init | 🔴 |
| 12 | 第二次 init | marker 存在 → 跳过向导 → mount fstab | 🔴 |
| 13 | axsh Shell | `axsh> ` 提示符, help/fls/sver 可用 | 🔴 |

---

## 🔬 调试工具状态

| 功能 | 状态 |
|------|------|
| KLog 串口日志 | ✅ |
| 内核 panic handler | ✅ `int 0x82` → barrier |
| 用户态 panic handler | ✅ `proc_exit` + 文件/行号 |
| 寄存器 dump (panic) | ✅ RAX-R15 + CR2/CR3 |
| Backtrace | 🔴 |
| QEMU `-d int,cpu_reset` | ✅ |
| GDB stub | ✅ 基础 |
| host-tests (67 passed) | ✅ |
| kernel tests (256/256) | ✅ |

---

## 📖 相关文档

- [开发规划](./roadmap.md)
- [内核架构](../architecture/kernel-architecture.md)
- [启动流程](../architecture/boot-process.md)
- [构建系统](./build-system.md)
