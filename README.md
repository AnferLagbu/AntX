# AntX

AntX 是一个从零构建的 x86_64 操作系统，目标是成为一个功能相对完整的自研 OS。

> **AntX = QueenX 内核 + 任意用户态**

---

## 内核架构

| 层级 | 组件 | 语言 |
|------|------|------|
| 引导 | Multiboot2 → 长模式过渡 → 4GB 恒等映射 | NASM |
| 内核核心 | QueenX (`libqueenx.a`) — 静态库, 无 `main.rs` | Rust + 少量 C |
| 协议栈 | lwIP 2.2.1 — DHCP / TCP / UDP / HTTP / DNS / mDNS / MQTT | C |
| 驱动 | E1000 网卡 + COM1 串口 | Rust |
| 用户态 | init / axsh (交互 shell, 19 个内置命令) / install | C |

## 特色子系统

- **HVFS** — 类 ext2 的原生文件系统, 支持三级间接块, LRU 块缓存, FSCK, 磁盘持久化
- **PWID** — 基于能力的权限模型, 支持令牌委托/信任链/域隔离
- **Barrier** — 故障恢复屏障, VFS 快照与级联回滚
- **KLog** — 自举日志系统, 内建串口驱动, RDTSC 时间戳, `[INFO] [NET]` 格式化输出
- **MLFQ 调度器** — 4 级反馈队列 + RT (FIFO/RR), 256 进程槽位
- **IDT/PIC/IRQ** — 完整中断框架, 32 ISR + 16 IRQ + syscall + recovery stub

## 构建与运行

```bash
make all          # 编译内核 + 用户态
make iso          # 生成 bootable ISO (GRUB2)
make run          # QEMU 运行

make run-net      # QEMU 带 E1000 网卡 (DHCP + HTTP)
```

内核输出到串口 (`-serial stdio`), 启动日志格式:

```
0.721 [INFO] [BOOT] KLog v2.0 initialized
0.722 [INFO] [BOOT] AntX kernel starting queenx 0.1.0
0.723 [INFO] [BOOT] IDT+PIC ready
0.724 [INFO] [BOOT] PIT timer configured
0.924 [INFO] [NET]  E1000 initialized, IRQ registered
0.925 [INFO] [NET]  DHCP client started successfully
1.026 [INFO] [BOOT] --- Network Subsystem Ready ---
```

## 项目状态

| 指标 | 数值 |
|------|------|
| 内核代码量 | ~35000 行 (Rust + C + ASM) |
| 合规检查 | 800+ / 800+, 100% |
| Cargo 依赖 | 2 (`spin`, `bitflags`) |
| Nightly features | 2 (`asm`, `alloc_error_handler`) |
| TODO/FIXME | ~105 |

---

> 个人理念与兴趣驱动的实验性项目。持续演进中。
