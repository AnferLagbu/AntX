# AntX 已知问题与待解决项

> 最后更新: 2026-05-31
> 全面代码审计完成 — 详见 [审计报告](../changelog/AUDIT_REPORT_2026-05-30.md)

---

## 🔴 审计新发现 - Critical 问题 (首要修复)

> 以下为 2026-05-30 全面代码审计中发现的最严重问题，按优先级排列。

### A1. HvFS 假 SHA256 校验和 (P0)
- **文件**: `src/kernel/fs/hvfs/checksum.rs`
- **问题**: HvFS 声称使用 SHA256 进行数据完整性校验，实际使用简化的/非标准哈希函数
- **影响**: 静默数据损坏无法检测、恶意篡改无法识别
- **修复**: 复用 `credo/sha256.rs` 的标准 SHA256 实现

### A2. NVMe PRP2 始终为零 (P0)
- **文件**: `src/kernel/driver/storage/nvme.rs`
- **问题**: 多页 DMA 传输的第二页 PRP2 始终为 0，数据写入物理地址 0
- **影响**: 覆盖 IVT/中断向量表等关键低地址内存
- **修复**: 正确填充 PRP2 为第二页的物理地址

### A3. lwIP virt_to_phys 假恒等映射 (P0)
- **文件**: `src/kernel/net/sys_arch.rs`
- **问题**: `virt_to_phys()` 返回 `addr as u32`（恒等），在高半内核中完全错误
- **影响**: DMA 写入错误的物理地址 → 内核内存损坏
- **修复**: 实现真正的 virt_to_phys 转换

### A4. Barrier tick() 递归死锁 (P0)
- **文件**: `src/kernel/barrier/recovery.rs`
- **问题**: `tick()` 获取 RECOVERY_LOCK 后调用 `bsr_try_recover()` 再次获取同一锁
- **影响**: 任何心跳丢失触发 100% 死锁
- **修复**: tick() 中设置标志后释放锁，由独立内核线程执行恢复

### A5. 内存分配器 alloc/dealloc 路径不一致 (P0)
- **文件**: `src/rust/src/memory_allocator.rs`
- **问题**: alloc 在 kmalloc 失败时回退到 pmm_alloc_page，但 dealloc 无法区分来源
- **影响**: slab 元数据损坏
- **修复**: 保存来源标记，dealloc 时据此选择释放路径

### A6. 链接脚本 .rela.dyn 缺失 PHDR (P0)
- **文件**: `src/kernel/link/x86_64.ld`, `src/link.ld`
- **问题**: 动态重定位段无 PHDR 声明，运行时不被加载
- **影响**: 内核 -fPIC 重定位失败
- **修复**: 为 .rela.dyn/.dynsym/.dynstr 添加 `:kernel` PHDR

### A7. SpinLock/Mutex Guard Drop 缺少内存屏障 (P1)
- **文件**: `src/kernel/sync/spinlock.rs`, `src/kernel/sync/mutex.rs`
- **问题**: 锁释放时不包含 Release 语义，CPU 可能延迟临界区写操作
- **影响**: 竞态条件、多核数据不一致
- **修复**: Drop 中添加 `atomic::fence(Ordering::Release)`

### A8. COW 引用计数竞态 (P1)
- **文件**: `src/kernel/mm/cow.rs`
- **问题**: 非原子 refcount 递减，多核并发可致下溢
- **影响**: 物理内存泄漏或 UAF
- **修复**: 使用 `fetch_sub` 原子操作

### A9. PMM 伙伴分配器无锁 (P1)
- **文件**: `src/kernel/mm/pmm.rs`
- **问题**: 全局 buddy bitmap 无锁，SMP 并发可致双分配
- **影响**: 两个 CPU 分到同一物理页
- **修复**: 使用 SpinLock 或原子操作保护

### A10. syscall.md 完全过时 (P1)
- **文件**: `docs/api/syscall.md`
- **问题**: 文档 syscall 编号与实际 POSIX 实现完全不同
- **影响**: 依赖文档的开发者无法编写用户态程序
- **修复**: 从 `src/kernel/syscall/types.rs` 重新生成

### A11. 全局 `#![allow(dead_code)]` 抑制死代码检测 (P1) ✅ 已修复
- **文件**: `src/rust/src/lib.rs`
- **修复日期**: 2026-06-28
- **修复**: 移除全局 `#![allow(dead_code)]`，替换为精确 `#[allow]` 列表（stable_features/static_mut_refs/naming/improper_ctypes/unused_unsafe），编译 0w/0e

### A12. Cargo.toml 中 19 个死 Feature Flags (P1) ✅ 已修复
- **修复日期**: 2026-06-28
- **状态**: Cargo.toml 现仅含 2 个 feature flag（`default`, `kernel_test`），19 个已清理

### A13. 编译错误：`smp::is_smp_enabled()` 不存在 (P1)
- **文件**: `src/kernel/mm/vmm.rs:919`

### A14. 编译错误：`send_tlb_invalidate_ipi` 类型不匹配 (P1)
- **文件**: `src/kernel/mm/vmm.rs:920`

### A15. 580 个被忽略的返回值 (P2) ✅ 基本解决
- **修复日期**: 2026-06-28
- **状态**: `RUSTFLAGS="-W unused_must_use" cargo check` 产出 0 warnings。580 项已全部通过类型系统或 `let _ =` 处理。

---

## ⚠️ 未解决问题 (4项)

### 5. 核心子系统 SAFETY 注释覆盖率 (P2) ✅ 基本完成

**状态**: 🟢 100% 覆盖已达成 (2026-06-28)

**影响**: unsafe 代码缺乏形式化 safety justification，审计困难。

**最终统计**:
| 模块 | unsafe blocks | SAFETY 注释 | 覆盖率 |
|------|--------------|-------------|--------|
| cow.rs | 23 | ✅ 23 | 100% |
| page_fault.rs | 12 | ✅ 12 | 100% |
| vmm.rs | 39 | ✅ 39 | 100% |
| rcu.rs | 17 | ✅ 17 | 100% |
| pmm.rs | 24 | ✅ 24 | 100% |
| rwlock.rs | 7 | ✅ 7 | 100% |
| dynamic.rs | 4 | ✅ 4 | 100% |
| devtree.rs | 3 | ✅ 3 | 100% |
| **总计** | **129** | **129** | **100%** |

**完成历史**: Round 1 (cow+page_fault: 35), Round 2 (vmm: 39), Round 3 (rcu+pmm: 31), Round 4 (pmm余+rwlock+dyn+devtree: 24)

---

### 4. HvFS get_obj_mut 返回拷贝致修改无声丢失 (P1) ✅ 已修复

**状态**: 🟢 已修复 (2026-06-28)

**修复**: 5 处调用点在修改后添加 `ds.objset.update_obj(&obj)` 写入副本。
移除所有 `#[allow(unused_assignments)]` 和 FIXME 注释。

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

## 🔧 技术债务 (6项)

| 项 | 说明 | 优先级 | 状态 |
|----|------|--------|------|
| **系统调用 `int 0x80` → `syscall` 迁移** | x86_64 长模式下使用遗留 `int 0x80` 而非 `syscall`/`sysret`，性能差 2-4 倍 | medium | ✅ |
| lwIP NO_SYS=1 单线程 | 迁移到 `tcpip_thread` 可根除问题 #1 的长期方案 | medium | 🟡 |
| VFS `/` 根目录依赖用户态 mount | 磁盘引导时需内核先挂 HvFS | medium | 🟡 |
| `axsh` help 文本与 BUILTINS 不同步 | help() 函数已改为从 TABLE 动态生成，`general.rs` 不再硬编码命令列表。新增命令只需修改 `mod.rs` TABLE 一处 | low | ✅ |
| `userlib::*` 全局导出 syscall | 已移除 `pub use sys::*`，~80 项不再注入 `userlib::` 命名空间。消费者通过 `use userlib::sys::*` 显式导入 | low | ✅ |
| `syscall_entry` SMP 内核栈竞态 | 已修复：使用 `swapgs` + `[gs:0]` per-CPU 数据访问，每个 CPU 独立 `SyscallPerCpu` + `syscall_stack`。`IA32_KERNEL_GS_BASE` 在 `gdt_init`/`gdt_init_ap` 中分别设置 | high | ✅ |

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
| host-tests (182 passed) | ✅ |
| kernel tests (254/256, 1 pre-existing failure, 1 skip) | 🟡 barrier::undo_log::rollback |

---

## 📖 相关文档

- [开发规划](./roadmap.md)
- [内核架构](../architecture/kernel-architecture.md)
- [启动流程](../architecture/boot-process.md)
- [构建系统](./build-system.md)
