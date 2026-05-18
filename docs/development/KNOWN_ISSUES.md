# AntX 已知问题与待解决项

> 最后更新: 2026-05-19

---

## ⚠️ 未解决问题

### 1. lwIP `lwip_init()` 间歇性卡死启动 (P0)

**状态**: 🔴 临时绕过 (kernel 启动时跳过网络初始化)

**现象**:
- `kernel_init()` 调用 `qx_net_init()` → `lwip_init()` 时，内核停止输出
- 间歇性发生，约 50% 概率
- 卡在 `[NET] Step3: init lwIP core` 之后无任何输出

**根因分析**:
- `lwip_init()` 内部可能触发 DHCP 或其他协议栈定时器回调
- 在 `NO_SYS=1` 单线程模式下，某些回调路径可能导致 busy-loop 或死锁
- timer ISR 的 `sys_check_timeouts()` 调用可能在 lwIP 内部状态未完全初始化时闯入

**当前绕过方案** (2026-05-19):
- `lib.rs`: kernel_init 中不调用 `qx_net_init()`
- `timer/irq.rs`: 添加 `NET_READY` 原子门，在 lwIP 就绪前不调用 `sys_check_timeouts()` / `e1000_poll_rx()`
- 网络子系统初始化推迟到用户态 init 完成后再触发

**正确修复方向**:
- 将 `lwip_init()` 改为在用户态通过 syscall 触发 (如 `sys_net_init`)
- 或者在 `lwip_init()` 调用前确保所有 timer 回调路径不会干扰初始化

**影响**:
- QEMU 启动时无网络 (E1000 DHCP 不运行)
- 用户态程序暂时无法使用 socket API

---

### 2. 安装向导 init 进程无输出 (P1)

**状态**: 🔴 待定位

**现象**:
- 内核成功进入 Ring 3 (`[USER] Entering Ring 3 (init pid=2)...`)
- 之后无任何用户态输出
- 看不到 `[init] AntX init process started` 

**可能原因**:
- init ELF 的 entry point 或内存布局问题 (修复了 embed 方式后需验证)
- `proc_exec` / `load_elf_from_memory` 的用户栈或 GDT 段选择子不正确
- init 二进制过大或链接脚本偏移不兼容

**验证方法**:
- 用 `-serial file:` 模式捕获完整输出查是否有 panic 消息
- 检查 `include_bytes!` 路径和 ELF 内容完整性
- 对比 embed 方式变更前后的 init ELF md5

---

### 3. HvFS mount 逻辑缺少磁盘 LBA 参数 (P1)

**状态**: 🟡 架构设计中

**现象**:
- 内核 `kernel_init()` 中的 HvFS mount 逻辑硬编码了读写 `ata_read_sector(0, 2046, ...)`
- 但 HvFS 的 `spa.disk_present` 和 `init()` 是否真的能从原始磁盘挂载未经验证
- HvFS 格式化时调用的是 `hvfs.format_disk()`，但该函数是否接受 `disk_id` 参数不清楚
- 安装完成后重启时，内核能否正确识别已安装的 HvFS 分区未经端到端测试

**需要实现**:
- HvFS 的 `spa` 初始化需要知道磁盘号 (目前 `disk_format(disk_id)` 可能只是格式化 Live RamFS)
- `vfs_mount_internal` 中的 HvFS 路径需要接收磁盘参数
- boot config sector 中的 `hvfs_lba` 需要被 `hvfs.init()` 正确使用

---

## ✅ 已解决问题

### 4. `KERNEL_TEST_OBJS` 缺少 `user_init_bin.o` → 测试链接失败

**修复**: Makefile 添加 `build/user/embedded/user_init_bin.o` 到 `KERNEL_TEST_OBJS`
**提交**: `4c2faf3`

---

### 5. 网络初始化顺序错误 → 系统启动挂死

**根因**: `lwip_init()` 在 `sys_init()` 之前调用
**修复**: 调整顺序为 `e1000_probe → sys_init → lwip_init`
**提交**: `04a3c28`

---

### 6. `make test-host` 日志目录缺失 + tee 退出码

**修复**: 添加 `mkdir -p tests/reports` + `; true`
**提交**: `04a3c28`

---

### 7. 定时器 ISR 与 lwIP 的竞态条件 (部分修复)

**根因**: timer ISR 在 lwIP 初始化前调用 `sys_check_timeouts()`
**修复**: 添加 `NET_READY` 原子标志位，ISR 端检查后再调用
**提交**: 待推送 (与问题 #1 一起)

---

### 8. `gen_embed.py` + `embedded/` 冗余中间层

**根因**: 用户态 ELF 通过 Python → C → gcc → ld 链嵌入内核
**修复**: Rust `include_bytes!` 直接嵌入，删除 1265 行死代码
**提交**: `1e32381`

---

### 9. Makefile 依赖顺序: Rust 内核编译时 `build/` 可能为空

**根因**: `make clean` 后 `cargo build` 先于 `stage1.bin` / `init.bin` 生成，`include_bytes!` 找不到文件
**修复**: `$(RUST_LIB)` 添加依赖 `build/user/init.bin $(STAGE1_BIN)`
**提交**: 待推送

---

## 🔧 技术债务

| 项 | 说明 | 优先级 |
|----|------|--------|
| lib/fs.rs 未使用 `O_TRUNC` 导入 | 仅 `deploy/fput` 使用，但导入在 `fs.rs` 顶层 | low |
| lwIP C 源码与 Rust 内核集成 | NO_SYS=1 单线程模式, 未来可考虑迁移到 Rust 实现 | medium |
| VFS 根挂载依赖用户态 init | 内核不挂载 `/`, 由 init 进程负责; 磁盘引导时需先在内核挂 HvFS | medium |
| 网络 barrier domain 注册 | 因 qx_net_init 推迟, NET=5 barrier 暂未注册 | low |

---

## 📋 安装流端到端验证清单

| 步骤 | 预期 | 状态 |
|------|------|------|
| 1. QEMU -kernel 启动 | 内核日志完整, 进入 Ring 3 | 🟡 内核到 Ring 3, 用户态无输出 |
| 2. init 打印 banner | `[init] AntX init process started` | 🔴 未验证 |
| 3. 磁盘选择 | 显示 64MB 磁盘, 选择 0 | 🔴 未验证 |
| 4. 分区/格式化 | 无 fatal error | 🔴 未验证 |
| 5. HvFS mount → /mnt | mount 成功 | 🔴 未验证 |
| 6. 文件复制 | 4/4 OK | 🔴 未验证 |
| 7. 安装完成重启 | 写 `/.antx_installed` → reboot | 🔴 未验证 |
| 8. 二次引导 | BIOS → Stage1 → HvFS mount → 跳过向导 → shell | 🔴 未验证 |
