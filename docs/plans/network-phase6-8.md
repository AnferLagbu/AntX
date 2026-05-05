# AntX 网络子系统后续任务规划

> 版本: 1.0  
> 日期: 2026-05-05  
> 状态: 进行中 (Phase 1-5 已完成)  
> 交接目标: 为后续模型/开发者提供 Phase 6-8 的完整任务清单

---

## 已完成阶段回顾 (Phase 1-5)

| Phase | 提交 | KB | 成果 |
|-------|------|-----|------|
| Phase 1 | `3823a68` | 878 | lwIP 2.2.1 嵌入式集成 (61 .o) |
| Phase 2 | `46466ae` | 882 | E1000 网卡驱动 + netif 适配 |
| Phase 3 | `3df38ef` | 887 | 动态页表映射 (CR3→PML4→pdpt_low[3]) + 1000Mbps Link Up |
| Phase 4 | `7a9343d` | 777 | NO_SYS=1 Raw API 切换 + Timer ISR + DHCP 获取 10.0.2.15 |
| Phase 5 | `6ac0a6d` | 773 | HTTP 服务器 + DNS + 静态 IP 10.0.2.15/24 |

### 关键架构决策

| 决策 | 说明 |
|------|------|
| `NO_SYS=1` | Raw API 单线程模式，不依赖 `sys_thread_new` |
| `e1000_probe()` 自主 PCI 扫描 | 绕过 Rust `pci_init()` 的 FFI 崩溃 |
| 静态 IP | 10.0.2.15/24, gw=10.0.2.2 (QEMU user-mode 默认) |
| 页表映射 | 2MB 大页 (PS=1+PCD) 映射 MMIO BAR 0xFEBC0000 |
| klog 统一 | 所有 net/ 输出经 klog，零 raw `serial_*` 残留 |

### 当前文件结构

```
src/net/
├── qx_net.h                  # 头文件声明
├── qx_net_init.c             # 子系统初始化 + qx_net_poll()
├── qx_netif.c                # netif 注册 (静态 IP)
├── qx_net_apps.c             # HTTP 服务器 + DNS 测试入口
├── qx_fsdata.c               # HTTP 自定义文件系统 (被 fs.c include)
├── arch/
│   ├── cc.h                  # 编译器抽象 + LWIP_PLATFORM_DIAG/ASSERT
│   ├── qx_hooks.h            # lwIP hook 宏
│   ├── sys_arch.h            # 类型定义 (NO_SYS 条件编译)
│   └── sys_arch.c            # 移植层实现 (信号量/邮箱/线程 stub)
├── driver/
│   ├── e1000.c               # Intel 82540EM 驱动 (自主 PCI + 页表 + IRQ)
│   ├── e1000.h               # 设备结构体 + 函数声明
│   └── e1000_regs.h          # 寄存器偏移宏
└── lwip/
    ├── lwipopts.h            # 内核定制 lwIP 配置
    ├── lwip-version.h        # 版本号定义
    └── src/                  # lwIP 2.2.1 源码 (不变)
```

---

## Phase 6: IRQ / 接收路径修复 + DHCP 恢复

### 6.1 PIC 中断屏蔽修复 (高优先级)

**问题**: E1000 中断处理器已注册 (`idt_register_irq`)，但 `irq_handler` 中的条件 `irq != 0 && irq != 7 && irq != 14 && irq != 15` 会静默丢弃未知 IRQ。同时 `pic_remap` 后未显式调用 `idt_enable_irq()`。

**根因分析** (见 [idt.c](file:///home/anfer/Code/C/AntX/src/kernel/idt.c)):
- `pic_remap()` 在 `idt_init()` 中被调用 (offset1=32, offset2=40)
- `pic_remap` 末尾写 `0x00` 到 PIC 掩码寄存器，**屏蔽了所有 IRQ**
- Timer 通过 `idt_register_irq(0, ...)` 成功处理是因为 BIOS/PIT 默认不屏蔽 IRQ0
- E1000 IRQ 11 被 PIC 从片 (0xA1) 屏蔽

**修复步骤**:

1. **在 `idt_init()` 末尾取消所有 IRQ 屏蔽**:
   ```c
   /* 在 pic_remap 后 */
   outb(0x21, 0x00);  // 主 PIC: 全部启用
   outb(0xA1, 0x00);  // 从 PIC: 全部启用
   ```

2. **或者在 `e1000_init()` 中调用 `idt_enable_irq()`**:
   ```c
   idt_register_irq(g_e1000.irq, e1000_irq_entry, "e1000", 0);
   idt_enable_irq(g_e1000.irq);  // 显式取消此 IRQ 的屏蔽
   ```

3. **验证**: QEMU 启动后，`grep "Spurious"` 应为 0，`E1000` ISR 应有调用日志。

### 6.2 接收路径端到端验证

**问题**: 当前 DHCP/ARP 发包后无响应，可能 IRQ 未触发 `e1000_isr()`。

**调试方法**:
1. 在 `e1000_isr()` 开头加 `klog_drv("E1000 ISR: ICR=0x%x", icr)` (需 `snprintf`)
2. 在 QEMU 侧用 `-netdev user,id=n0,dump=1` 或 `tcpdump` 抓包对比
3. 验证 `e1000_send()` 真正发出了帧：检查 TX descriptor writeback

### 6.3 DHCP 恢复

**前提**: IRQ 接收路径已修复。

**任务**:
1. 恢复 `qx_netif.c` 中 DHCP 调用:
   ```c
   /* 替换静态 IP */
   netif_add(&g_qx_netif, NULL, NULL, NULL, NULL, e1000_init, ethernet_input);
   netif_set_default(netif);
   netif_set_up(netif);
   netif_set_status_callback(netif, qx_netif_status);
   dhcp_start(netif);
   ```
2. 在 `qx_netif_status` 回调中检测 `ip != 0` → 触发 `qx_net_apps_init()`
3. 确认 `timer.c` 中 `sys_check_timeouts()` 每次 tick 被调用

### 6.4 idle 循环中的网络轮询

**当前状态**: `kernel_main()` 的 idle 循环仅调用 `interrupt_idle()` (hlt 指令)。
计时器 ISR 在每个 tick (10ms) 触发 `sys_check_timeouts()`。

**注意**: 如果 IRQ 被 PIC 屏蔽，`hlt` 永远不会被中断唤醒，导致 DHCP 超时永不触发。
修复 PIC 屏蔽后此问题应自然消失。

**备选**: 如果仍需轮询，可在 idle 中加入:
```c
while (1) {
    extern void sys_check_timeouts(void);
    sys_check_timeouts();
    for (volatile int w = 0; w < 100000; w++) __asm__ volatile("pause");
    interrupt_idle();
}
```

---

## Phase 7: ICMP Ping + TCP HTTP 端到端

### 7.1 ICMP Echo (Ping)

**前提**: IRQ 接收路径已修复。

**任务**:
1. 在 `qx_net_apps.c` 中恢复 `ping_gateway()`:
   ```c
   struct raw_pcb *pcb = raw_new(IP_PROTO_ICMP);
   if (!pcb) {
       klog_net_warn("Ping: raw_new failed (check MEMP_NUM_RAW_PCB)");
       return;
   }
   raw_recv(pcb, ping_recv, NULL);
   raw_bind(pcb, IP_ADDR_ANY);
   ```
2. 如果 `raw_new` 返回 NULL，检查 `lwipopts.h` 中 `MEMP_NUM_RAW_PCB >= 1` (当前值 8，OK)
3. 发送到 10.0.2.2 (QEMU 网关)，验证收到回复

### 7.2 HTTP 服务器端到端验证

**前提**: Ping 已成功 (证明接收路径工作)。

**任务**:
1. 在 QEMU 启动命令中添加端口转发:
   ```bash
   qemu-system-x86_64 ... \
     -netdev user,id=n0,hostfwd=tcp::8080-:80 \
     -device e1000,netdev=n0 ...
   ```
2. 从宿主机访问:
   ```bash
   curl http://localhost:8080/
   # 预期输出: AntX Kernel / lwIP TCP/IP stack is running.
   ```
3. 如果不通，分析:
   - `netstat` / lwIP stats 查看 TCP 连接状态
   - 在 `e1000_isr()` 中加 debug 日志
   - 检查 `httpd_init()` 是否成功绑定端口 80

### 7.3 TCP 连接调试

**注意**: `NO_SYS=1` 下 `httpd` 使用 Raw API TCP callback 模式。
- `tcp_accept()` → `tcp_recv()` → `tcp_sent()` callback 链
- 验证 `LWIP_TCP=1` (已启用) + `MEMP_NUM_TCP_PCB=32`
- 如果 SYN 到达但未处理，检查 `ethernet_input()` 是否被 `e1000_isr()` 调用

---

## Phase 8: 高级应用 + 优化

### 8.1 DNS 端到端

**当前状态**: `dns_gethostbyname("example.com")` 已调用，但回调未触发 (IRQ 问题)。
IRQ 修复后应自然工作。

**验证**:
```
预期日志: DNS: example.com → 93.184.216.34
```

### 8.2 mDNS (多播 DNS)

**配置** (已启用): `LWIP_MDNS=1`, `MDNS_RESP_USENETIF_EXTCONTEXT=1`

**任务**:
1. 在 `qx_net_apps.c` 中添加 `mdns_resp_init()`
2. 添加 mDNS 服务条目: `mdns_resp_add_netif(netif, "antx", 3600)`
3. 从宿主机验证: `avahi-browse -a -r` 或 `dns-sd -B _http._tcp`

### 8.3 SNTP (时间同步)

**配置** (已启用): `LWIP_SNTP=1`, `SNTP_SERVER_DNS=1`

**任务**:
1. `sntp_init()` 在 IRQ 修复后应自动工作
2. 验证: `sntp_get_current_timestamp()` 返回 Unix 时间戳
3. 可选: 将同步时间写入系统时钟

### 8.4 MQTT / SMTP / TFTP

**配置** (已启用): 这些应用模块已在编译但未初始化。
**任务**: 在 `qx_net_apps.c` 中添加初始化入口 (可选，按需)。

### 8.5 lwiperf 性能测试

**配置** (已启用): `LWIP_LWIPERF=1`
**任务**: IRQ 修复后运行 `lwiperf_start_tcp_server_default()` 进行吞吐量测试。

### 8.6 LWIP_DEBUG 调试输出

如果需要详细的 lwIP 内部日志:
```c
// lwipopts.h
#define LWIP_DEBUG      1
#define DHCP_DEBUG      LWIP_DBG_ON
#define TCP_DEBUG       LWIP_DBG_ON
#define ETHARP_DEBUG    LWIP_DBG_ON
#define ICMP_DEBUG      LWIP_DBG_ON
// 同时修复 cc.h:
#define LWIP_PLATFORM_DIAG(x)  do { serial_puts(SERIAL_COM1, x); } while (0)
```
**注意**: 需要修复 `cc.h` 和 `debug.h` 中的 `U16_F` 等格式宏 (klog 不支持标准 printf)。

---

## 已知问题清单

| # | 问题 | 文件 | 根因 | 修复方向 |
|---|------|------|------|----------|
| 1 | E1000 IRQ 不触发 | `idt.c` / `e1000.c` | PIC 从片掩码屏蔽 IRQ 8-15 | 调用 `idt_enable_irq(11)` 或 `outb(0xA1, ~(1<<3))` |
| 2 | DHCP 超时无响应 | `qx_netif.c` | IRQ 不触发 → ARP/DHCP 收不到回复 | 修复 IRQ 后恢复 DHCP |
| 3 | `raw_new(IP_PROTO_ICMP)` 返回 NULL | `qx_net_apps.c` | 可能 MEMP 耗尽或 IRQ 上下文冲突 | IRQ 修复后重试，增加内存池 |
| 4 | `Failed to load init process!` | `main.c` | 用户空间 ELF 加载失败 | 非网络问题，不影响网络测试 |
| 5 | klog 输出含 `0x000000000000` 前缀 | `klog.c` | `serial_put_hex` 不对齐 | 低优先级 cosmetic |
| 6 | `LWIP_PLATFORM_DIAG` 不能处理 `%U16_F` | `cc.h` | klog `vsnprintf` 不支持 PRI 宏 | 调试时禁用需要格式宏的 debug 标志 |

---

## 快速验证脚本

### Phase 6-7 综合测试

```bash
#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf build isodir logs
mkdir -p logs isodir/boot/grub

# 编译
make all 2>&1 | tail -1

# 综合测试
make test-comprehensive 2>&1 | tail -2
ls -t tests/reports/comprehensive_*.log | head -1 | while read f; do
  echo "PASS:$(grep -c 'PASS\]' $f) FAIL:$(grep -c 'FAIL\]' $f)"
done

# QEMU 启动 (带 HTTP 端口转发)
cp build/kernel.bin isodir/boot/kernel.bin
echo 'set timeout=0; set default=0; menuentry "AntX" { multiboot2 /boot/kernel.bin }' \
  > isodir/boot/grub/grub.cfg
grub2-mkrescue -o build/antx.iso isodir 2>/dev/null

timeout 30 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx.iso \
  -netdev user,id=n0,hostfwd=tcp::8080-:80 \
  -device e1000,netdev=n0 \
  -serial file:logs/boot.log -display none 2>/dev/null || true

echo "=== Network Log ==="
grep -E "NETWORK|DRIVER.*E1000|Ping|HTTP|DNS" logs/boot.log | head -20

echo "=== HTTP Test ==="
curl -s --max-time 3 http://localhost:8080/ 2>/dev/null && echo "OK" || echo "FAIL"

echo "=== DHCP/Ping ==="
grep -E "DHCP|10\.0\.2" logs/boot.log | head -10
```

---

## 参考资源

| 资源 | 路径 |
|------|------|
| lwIP 配置 | [src/net/lwip/lwipopts.h](file:///home/anfer/Code/C/AntX/src/net/lwip/lwipopts.h) |
| E1000 驱动 | [src/net/driver/e1000.c](file:///home/anfer/Code/C/AntX/src/net/driver/e1000.c) |
| 中断系统 | [src/kernel/idt.c](file:///home/anfer/Code/C/AntX/src/kernel/idt.c) |
| Timer ISR | [src/kernel/timer.c](file:///home/anfer/Code/C/AntX/src/kernel/timer.c) |
| 内核主循环 | [src/kernel/main.c](file:///home/anfer/Code/C/AntX/src/kernel/main.c) |
| klog 系统 | [src/kernel/klog.c](file:///home/anfer/Code/C/AntX/src/kernel/klog.c) |
| 移植层 | [src/net/arch/sys_arch.c](file:///home/anfer/Code/C/AntX/src/net/arch/sys_arch.c) |
| HTTP 文件 | [src/net/qx_fsdata.c](file:///home/anfer/Code/C/AntX/src/net/qx_fsdata.c) |
| 网络应用 | [src/net/qx_net_apps.c](file:///home/anfer/Code/C/AntX/src/net/qx_net_apps.c) |
| 已有规划 | [docs/plans/lwip-integration-plan.md](file:///home/anfer/Code/C/AntX/docs/plans/lwip-integration-plan.md) |
