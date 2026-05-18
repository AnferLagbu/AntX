# AntX 已知问题与待解决项

> 最后更新: 2026-05-19 01:30

---

## ⚠️ 未解决问题 (5项)

### 1. lwIP `lwip_init()` 间歇性卡死内核启动 (P0)

**状态**: 🔴 临时绕过 — 内核启动时跳过网络初始化 | **发现**: 2026-05-18

**现象**:
- `kernel_init()` → `qx_net_init()` → `lwip_init()` 时串口输出停止
- 概率约 40%~60%，卡在 `[NET] Step3: init lwIP core` 后无任何输出
- 系统完全无响应 (无 panic、无 watchdog)

**根因** (3层):
1. `lwip-src/core/init.c:lwip_init()` 内部注册协议栈定时器 (`dhcp_tmr`, `tcp_tmr`, `arp_tmr` 等)
2. `NO_SYS=1` 单线程模式下，定时器由 timer ISR 的 `sys_check_timeouts()` 驱动
3. timer ISR 在 lwIP 半初始化状态闯入 → 访问未初始化的 `netif_list`/`tcp_pcbs` → busy-loop/死锁

**绕过方案** (commit `30b0f20`):
- `net/types.rs:L99` — 新增 `NET_READY: AtomicBool`
- `net/init.rs:L174` — FullyInitialized 时设 `NET_READY = true`
- `timer/irq.rs:L43` — ISR 检查 `NET_READY` 后才调 lwIP 回调
- `lib.rs:L196` — **不调用** `qx_net_init()` — 网络完全延迟

**副作用**: QEMU 无 E1000/DHCP, 用户态无 socket API

**修复路线**:
1. **短期**: `qx_net_init()` 改为用户态 syscall 触发 (`SYS_NET_INIT = 120`)
2. **中期**: `lwip_init()` 前后 `cli`/`sti` 保护临界区
3. **长期**: lwIP 定时器从 ISR 分离到内核线程

---

### 2. 用户态 init 进程进入 Ring 3 后无任何输出 (P0)

**状态**: 🔴 定位中 | **发现**: 2026-05-19

**现象**:
```
0.152755 [BOOT] [USER] Launching init process...
0.169990 [BOOT] [USER] Entering Ring 3 (init pid=2)...
                                                     ← 之后无任何输出
```
期望: `[init] AntX init process started` → 安装向导 banner

**排查矩阵**: `include_bytes!` 嵌入路径变化 / `load_elf_from_memory` 用户栈段选择子 / init ELF 链接基址 `0x400000` / `switch.S` 上下文 / VFS fd 表可见性

**下一步验证**:
1. `hexdump -C build/user/init.bin | head -4` 确认 ELF magic
2. `-serial file:qemu_full.log -d int,cpu_reset` 抓完整日志
3. 对比 `include_bytes!` 前后 init ELF 的 md5
4. 在 `load_elf_from_memory` 中打印 ELF header 偏移

---

### 3. HvFS 磁盘挂载路径未经验证 (P1)

**状态**: 🟡 代码已写，端到端未测试

**问题链**: `sys_disk_format` → `hvfs.format_disk()` 是否写 VDEV label? / config sector LBA 2046 / 重启后 `hvfs.init()` 能否从磁盘读取

**涉及文件**: `syscall/mod.rs:L589`, `syscall/mod.rs:L548`, `fs/vfs/ffi.rs:L64`, `fs/hvfs/spa.rs:L189`, `fs/hvfs/hvfs.rs:L118`, `lib.rs:L207`

**验证清单**:
- [ ] format → 立即回读校验
- [ ] `disk_present=true` → `hvfs.init()` → 打开磁盘 VDEV
- [ ] 安装向导写 `/mnt/.antx_installed` → umount → mount → 文件存在
- [ ] 完整安装 → 重启 → 内核自动 mount → 跳过向导

---

### 4. install crate 跨 crate 依赖复杂 (P2)

**状态**: 🟡 可工作，需简化

**当前依赖**: `init → install(lib) → userlib`, `install(bin) → install(lib)`

**问题**: `wizard` 模块路径硬编码 `/mnt/...`，建议提取 `run_with_prefix(prefix: &str)`

---

### 5. 系统无 panic 回溯 / 无调试信息输出 (P2)

**状态**: 🔴 缺失 — panic 无寄存器 dump / 无 backtrace / 调试依赖日志推测

---

## ✅ 已解决问题 (7项)

### 6. `KERNEL_TEST_OBJS` 缺少 `user_init_bin.o` → 测试链接失败
**提交**: `4c2faf3` | **日期**: 2026-05-18

### 7. 网络初始化顺序错误 → 系统启动挂死
**提交**: `04a3c28` | **日期**: 2026-05-18 — `e1000_probe → sys_init → lwip_init → netif_register`

### 8. `make test-host` 日志目录缺失 + tee 非零退出码
**提交**: `04a3c28` | **日期**: 2026-05-18

### 9. `gen_embed.py` + `embedded/` 冗余嵌入工具链
**提交**: `1e32381` | **日期**: 2026-05-19 — **-1251行** 死代码，2个 `include_bytes!` 替代

### 10. Makefile: `$(RUST_LIB)` 依赖缺失 → clean build 失败
**提交**: `30b0f20` | **日期**: 2026-05-19

### 11. 用户态目录结构重组为 workspace
**提交**: `073eaef`, `41091db`, `b1b353b` | **日期**: 2026-05-19

### 12. 安装向导持久化流 — HvFS mount + /mnt 部署 + 磁盘引导检测
**提交**: `a4ceff7`, `3c95656` | **日期**: 2026-05-19

---

## 🔧 技术债务 (7项)

| 项 | 说明 | 优先级 |
|----|------|--------|
| `lib/fs.rs` `O_TRUNC` unused import | 仅 `deploy` 使用, 导入在 `fs.rs` 顶层 | low |
| lwIP NO_SYS=1 单线程 | 迁移到 `tcpip_thread` 可根除问题 #1 | medium |
| VFS `/` 根目录依赖用户态 mount | 磁盘引导时需内核先挂 HvFS | medium |
| NET=5 barrier domain 未注册 | `qx_net_init` 延迟导致 | low |
| `axsh` help 文本与 BUILTINS 不同步 | 新命令需改两处 | low |
| `userlib::*` 全局导出 syscall | `pub use sys::*` 污染命名空间 | low |
| `diagnose_user_process.py` 引用已删除的 `gen_embed.py` | 需清理 | low |

---

## 📋 安装流端到端验证矩阵

| # | 步骤 | 预期 | 状态 |
|---|------|------|------|
| 1 | QEMU `-kernel -drive` | 内核到 `Entering Ring 3` | 🟡 内核 OK, 用户态无输出 |
| 2 | init banner | `[init] AntX init process started` | 🔴 |
| 3 | 安装向导 welcome | `AntX Installation Wizard` | 🔴 |
| 4 | 磁盘探测/选择 | 1 个 64MB 盘, 选 0, yes | 🔴 |
| 5 | 分区/格式化 | 无 fatal error | 🔴 |
| 6 | HvFS mount /mnt | mount 成功 | 🔴 |
| 7 | 应用部署 | 4/4 OK | 🔴 |
| 8 | PWID 创建 | 密码确认 → 成功 | 🔴 |
| 9 | 主机名 | 输入 → 成功 | 🔴 |
| 10 | 安装完成 | 写标记 → sync → umount → reboot | 🔴 |
| 11 | 磁盘引导 | BIOS→Stage1→kernel→mount HvFS→init | 🔴 |
| 12 | 第二次 init | marker 存在 → 跳过向导 | 🔴 |
| 13 | axsh Shell | `axsh> ` prompt | 🔴 |

---

## 🔬 调试工具状态

| 功能 | 状态 |
|------|------|
| KLog 串口日志 | ✅ |
| 内核 panic handler | ✅ `int 0x82` → barrier |
| 用户态 panic handler | ✅ `proc_exit` + 文件/行号 |
| 寄存器 dump (panic) | 🔴 |
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
