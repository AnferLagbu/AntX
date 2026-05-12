#!/bin/bash
# 全量修复: DHCP + TX/RX 完整路径
# 从 git HEAD 开始, 一个脚本全部搞定
set -e
cd /home/anfer/Code/C/AntX

# =================================================================
# Fix 0: klog_net 缺失声明 (pre-existing bug fix)
# =================================================================
echo "=== Fix 0: klog_net extern ==="
python3 << 'PYEOF'
with open("src/kernel/net/driver/e1000.rs", "r") as f:
    c = f.read()
old = '    extern "C" {\n        fn antx_netif_init(netif: *mut core::ffi::c_void, mac: *const u8);\n    }'
new = '    extern "C" {\n        fn antx_netif_init(netif: *mut core::ffi::c_void, mac: *const u8);\n        fn klog_net(fmt: *const i8);\n    }'
if old in c:
    c = c.replace(old, new)
    with open("src/kernel/net/driver/e1000.rs", "w") as f:
        f.write(c)
    print("  OK")
else:
    print("  ERROR: pattern not found")
    exit(1)
PYEOF

# =================================================================  
# Fix 1: timer/irq.rs — sys_tick_inc() every tick (lwIP clock)
# =================================================================
echo "=== Fix 1: sys_tick_inc ==="
python3 << 'PYEOF'
with open("src/kernel/timer/irq.rs", "r") as f:
    c = f.read()
old = '''    // 1. 更新全局 tick 计数器
    crate::kernel::timer::on_timer_interrupt();

    // 2. lwIP 协议栈定时器处理 (DHCP/TCP/ARP 状态机)
    // 每 100 个 tick (100ms) 调用一次，避免过于频繁
    if crate::kernel::timer::get_ticks() % 100 == 0 {
        extern "C" { fn sys_check_timeouts(); }
        unsafe { sys_check_timeouts(); }
    }'''
new = '''    // 1. 更新全局 tick 计数器
    crate::kernel::timer::on_timer_interrupt();

    // 2. 驱动 lwIP 时间基准
    extern "C" { fn sys_tick_inc(); }
    unsafe { sys_tick_inc(); }

    // 3. lwIP 定时器处理 (DHCP/TCP/ARP)
    extern "C" { fn sys_check_timeouts(); }
    unsafe { sys_check_timeouts(); }'''
if old in c:
    c = c.replace(old, new)
    with open("src/kernel/timer/irq.rs", "w") as f:
        f.write(c)
    print("  OK")
else:
    print("  WARNING: pattern not found, trying line-by-line")
    lines = c.split('\n')
    out = []
    i = 0
    while i < len(lines):
        l = lines[i]
        if 'crate::kernel::timer::on_timer_interrupt();' in l:
            out.append(l)
            out.append('')
            out.append('    extern "C" { fn sys_tick_inc(); }')
            out.append('    unsafe { sys_tick_inc(); }')
            out.append('')
            out.append('    extern "C" { fn sys_check_timeouts(); }')
            out.append('    unsafe { sys_check_timeouts(); }')
            i += 1
            while i < len(lines) and 'sys_check_timeouts()' not in lines[i]:
                if lines[i].strip().startswith('extern') or lines[i].strip().startswith('unsafe { sys_check'):
                    i += 1
                    continue
                if 'if crate::kernel' in lines[i] or 'get_ticks' in lines[i]:
                    i += 1
                    continue
                break
            print("  OK (alt)")
        else:
            out.append(l)
        i += 1
    with open("src/kernel/timer/irq.rs", "w") as f:
        f.write('\n'.join(out))
PYEOF

# =================================================================
# Fix 2: net_glue.c — antx_rx_packet + antx_pbuf_copyout
# =================================================================
echo "=== Fix 2: net_glue.c ==="
cat > src/kernel/net/arch/net_glue.c << 'GEOFIX'
#include "lwip/netif.h"
#include "lwip/etharp.h"
#include "lwip/ethip6.h"
#include "lwip/pbuf.h"

extern err_t e1000_send(struct netif *netif, struct pbuf *p);

void antx_netif_init(struct netif *netif, const uint8_t *mac) {
    netif->hwaddr_len = 6;
    int i;
    for (i = 0; i < 6; i++) netif->hwaddr[i] = mac[i];
    netif->mtu = 1500;
    netif->flags = NETIF_FLAG_BROADCAST | NETIF_FLAG_ETHARP | NETIF_FLAG_ETHERNET
                 | NETIF_FLAG_IGMP | NETIF_FLAG_MLD6;
    netif->output = etharp_output;
    netif->output_ip6 = ethip6_output;
    netif->linkoutput = e1000_send;
    netif->name[0] = 'e';
    netif->name[1] = 'n';
}

err_t antx_rx_packet(struct netif *netif, const void *data, u16_t len) {
    struct pbuf *p;
    u16_t copied;

    if (netif == NULL || data == NULL || len == 0) return ERR_VAL;

    p = pbuf_alloc(PBUF_RAW, len, PBUF_POOL);
    if (p == NULL) return ERR_MEM;

    copied = pbuf_take(p, data, len);
    if (copied != len) { pbuf_free(p); return ERR_MEM; }

    if (netif->input(p, netif) != ERR_OK) { pbuf_free(p); return ERR_IF; }
    return ERR_OK;
}

void antx_pbuf_copyout(struct pbuf *p, void *buf, u16_t *out_len) {
    u16_t total = 0;
    struct pbuf *q = p;
    u8_t *dst = (u8_t *)buf;
    while (q != NULL && total + q->len <= *out_len) {
        memcpy(dst + total, q->payload, q->len);
        total += q->len;
        q = q->next;
    }
    *out_len = total;
}
GEOFIX
echo "  OK"

# =================================================================
# Fix 3: netif.rs — G_NETIF_BUFFER 2048 + antx_rx_packet + link_up
# =================================================================
echo "=== Fix 3: netif.rs ==="
python3 << 'PYEOF'
with open("src/kernel/net/netif.rs", "r") as f:
    c = f.read()

# Buffer size
c = c.replace(
    "static mut G_NETIF_BUFFER: [u8; 512] = [0u8; 512];",
    "static mut G_NETIF_BUFFER: [u8; 2048] = [0u8; 2048];")
c = c.replace(
    "core::ptr::write_bytes(G_NETIF_BUFFER.as_mut_ptr(), 0, 512);",
    "core::ptr::write_bytes(G_NETIF_BUFFER.as_mut_ptr(), 0, 2048);")

# ethernet_input_from_e1000 -> antx_rx_packet
old_rx = '''#[no_mangle]
pub unsafe extern "C" fn ethernet_input_from_e1000(
    data: *mut core::ffi::c_void,
    len: u16,
) -> i32 {
    // 检查网络接口是否已初始化
    if G_NETIF_PTR.is_null() || data.is_null() || len == 0 {
        return LwipErr::Val as i32; // 无效参数
    }

    // 调用 lwIP ethernet_input 处理数据包
    // 注意: 这里需要将原始数据包装成 pbuf 结构，或者直接使用内存指针
    // 简化实现: 直接传递给 ethernet_input (假设 lwIP 能处理原始指针)
    let result = ethernet_input(data, G_NETIF_PTR);

    result
}'''
new_rx = '''#[no_mangle]
pub unsafe extern "C" fn ethernet_input_from_e1000(
    data: *mut core::ffi::c_void,
    len: u16,
) -> i32 {
    extern "C" {
        fn antx_rx_packet(netif: *mut core::ffi::c_void, data: *const core::ffi::c_void, len: u16) -> i32;
    }
    if G_NETIF_PTR.is_null() || data.is_null() || len == 0 {
        return LwipErr::Val as i32;
    }
    antx_rx_packet(G_NETIF_PTR, data as *const core::ffi::c_void, len)
}'''
if old_rx in c:
    c = c.replace(old_rx, new_rx)
else:
    print("  WARN: eth_input_from_e1000 pattern not found")

# netif_set_link_up after netif_set_up
old_up = "    netif_set_up(result);"
new_up = '''    netif_set_up(result);
    extern "C" { fn netif_set_link_up(netif: *mut core::ffi::c_void); }
    netif_set_link_up(result);'''
c = c.replace(old_up, new_up)

with open("src/kernel/net/netif.rs", "w") as f:
    f.write(c)
print("  OK")
PYEOF

# =================================================================
# Fix 4: e1000.rs — extract_pbuf_data (chained pbuf) + PIC IRQ unmask
# =================================================================
echo "=== Fix 4: e1000.rs ==="
python3 << 'PYEOF'
with open("src/kernel/net/driver/e1000.rs", "r") as f:
    c = f.read()

# extract_pbuf_data replacement
old_ext = '''unsafe fn extract_pbuf_data(p: *mut core::ffi::c_void) -> (usize, *mut u8) {
    // 简化版: 假设 p 指向连续内存区域
    // 完整版需要遍历 pbuf 链表

    // 尝试读取 pbuf 的 next、len、payload 字段
    // 注意: 这里需要根据实际的 lwIP pbuf 结构定义来调整偏移量

    let pbuf_base = p as *mut u8;

    // pbuf 结构大致布局 (x86_64):
    // +0x00: next      (*pbuf)
    // +0x08: payload   (*void)
    // +0x10: tot_len   (u16_t)
    // +0x12: len       (u16_t)
    // +0x14: type      (u8)
    // +0x15: flags     (u8)
    // +0x16: ref       (u16_t)

    // 读取 tot_len (假设小端序)
    let len = *(pbuf_base.add(0x10) as *const u16) as usize;

    // 读取 payload 指针
    let payload = *(pbuf_base.add(0x08) as *const *mut u8);

    (len, payload)
}'''
new_ext = '''unsafe fn extract_pbuf_data(p: *mut core::ffi::c_void) -> (usize, *mut u8) {
    extern "C" {
        fn antx_pbuf_copyout(p: *mut core::ffi::c_void, buf: *mut u8, out_len: *mut u16);
    }
    let pbuf_base = p as *mut u8;
    let total = *(pbuf_base.add(0x10) as *const u16) as usize;
    static mut TX_BUF: [u8; 1600] = [0u8; 1600];
    let mut out_len: u16 = total.min(1600) as u16;
    antx_pbuf_copyout(p, TX_BUF.as_mut_ptr(), &mut out_len);
    (out_len as usize, TX_BUF.as_mut_ptr())
}'''
if old_ext in c:
    c = c.replace(old_ext, new_ext)
else:
    print("  WARN: extract_pbuf_data pattern not found")

# PIC port I/O helpers + IRQ unmask
# Insert helpers before global instance
c = c.replace(
    '/// 全局 E1000 实例\nstatic mut E1000_INSTANCE: Option<E1000Device> = None;',
    '''unsafe fn pic_outb(port: u16, value: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
}
unsafe fn pic_inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack));
    value
}

/// 全局 E1000 实例
static mut E1000_INSTANCE: Option<E1000Device> = None;''')

# Fix idt_register_irq flags type + add PIC unmask
old_irq = 'extern "C" {\n                                fn idt_register_irq(irq: u8, handler: extern "C" fn(*mut core::ffi::c_void), name: *const i8, flags: u8) -> i32;\n                            }\n                            idt_register_irq(dev.irq, e1000_irq_entry, b"e1000\\0".as_ptr() as *const i8, 0);'
new_irq = 'extern "C" {\n                                fn idt_register_irq(irq: u8, handler: extern "C" fn(*mut core::ffi::c_void), name: *const i8, flags: u32) -> i32;\n                            }\n                            idt_register_irq(dev.irq, e1000_irq_entry as extern "C" fn(*mut core::ffi::c_void), b"e1000\\0".as_ptr() as *const i8, 0);\n                            if dev.irq < 8 {\n                                let mask = pic_inb(0x21);\n                                pic_outb(0x21, mask & !(1u8 << dev.irq));\n                            } else {\n                                let mask = pic_inb(0xA1);\n                                pic_outb(0xA1, mask & !(1u8 << (dev.irq - 8)));\n                            }'
if old_irq in c:
    c = c.replace(old_irq, new_irq)
    print("  PIC unmask OK")
else:
    print("  WARN: IRQ pattern not found")

with open("src/kernel/net/driver/e1000.rs", "w") as f:
    f.write(c)
print("  OK")
PYEOF

echo ""
echo "=== All fixes applied ==="
echo "Next: cargo check + build + test"
