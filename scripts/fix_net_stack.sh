#!/bin/bash
# ============================================================================
# 修复网络栈: DHCP + TX/RX 全双工
# 问题:
#   1. sys_tick_inc() 从未调用 → sys_now() 恒为0 → DHCP/TCP 超时失效
#   2. ethernet_input_from_e1000 传裸指针给 ethernet_input (应传 pbuf*)
#   3. extract_pbuf_data 不支持多段 pbuf 链
# ============================================================================
set -e
cd /home/anfer/Code/C/AntX

echo "=== Backing up ==="
mkdir -p backup/net_dhcp_fix
cp src/kernel/timer/irq.rs backup/net_dhcp_fix/irq.rs.bak
cp src/kernel/net/netif.rs backup/net_dhcp_fix/netif.rs.bak
cp src/kernel/net/driver/e1000.rs backup/net_dhcp_fix/e1000.rs.bak
cp src/kernel/net/arch/net_glue.c backup/net_dhcp_fix/net_glue.c.bak
echo "Backup done"

# ============================================================================
# Fix 1: timer/irq.rs — 调用 sys_tick_inc() 驱动 lwIP 时间基准
# ============================================================================
echo "=== Fix 1: timer/irq.rs — add sys_tick_inc() ==="
python3 -c "
with open('src/kernel/timer/irq.rs', 'r') as f:
    c = f.read()

old = '''    // 1. 更新全局 tick 计数器 (核心功能)
    crate::kernel::timer::on_timer_interrupt();

    // 2. lwIP 协议栈定时器处理 (DHCP/TCP/ARP 状态机)
    // 每 100 个 tick (100ms) 调用一次，避免过于频繁
    if crate::kernel::timer::get_ticks() % 100 == 0 {
        extern \"C\" { fn sys_check_timeouts(); }
        unsafe { sys_check_timeouts(); }
    }'''

new = '''    // 1. 更新全局 tick 计数器
    crate::kernel::timer::on_timer_interrupt();

    // 2. 驱动 lwIP 时间基准 (sys_now 依赖此计数)
    extern \"C\" { fn sys_tick_inc(); }
    unsafe { sys_tick_inc(); }

    // 3. lwIP 协议栈定时器处理 (DHCP/TCP/ARP 状态机)
    extern \"C\" { fn sys_check_timeouts(); }
    unsafe { sys_check_timeouts(); }'''

if old in c:
    c = c.replace(old, new)
    with open('src/kernel/timer/irq.rs', 'w') as f:
        f.write(c)
    print('  timer/irq.rs updated')
else:
    print('  ERROR: pattern not found in timer/irq.rs')
    exit(1)
"

# ============================================================================
# Fix 2: net_glue.c — 加 antx_rx_packet() 创建 pbuf,调用 ethernet_input
# ============================================================================
echo "=== Fix 2: net_glue.c — add antx_rx_packet() ==="
cat > src/kernel/net/arch/net_glue.c << 'GLUE_EOF'
#include "lwip/netif.h"
#include "lwip/etharp.h"
#include "lwip/ethip6.h"
#include "lwip/pbuf.h"
#include "lwip/dhcp.h"

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
    struct pbuf *p, *q;
    u16_t copied;

    if (netif == NULL || data == NULL || len == 0) {
        return ERR_VAL;
    }

    /* 从 PBUF_POOL 分配 pbuf */
    p = pbuf_alloc(PBUF_RAW, len, PBUF_POOL);
    if (p == NULL) {
        return ERR_MEM;
    }

    /* 拷贝数据到 pbuf (可能跨多段) */
    copied = pbuf_take(p, data, len);
    if (copied != len) {
        pbuf_free(p);
        return ERR_MEM;
    }

    /* 送入 lwIP 协议栈 */
    if (netif->input(p, netif) != ERR_OK) {
        pbuf_free(p);
        return ERR_IF;
    }

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
GLUE_EOF
echo "  net_glue.c updated"

# ============================================================================
# Fix 3: netif.rs — ethernet_input_from_e1000 改为调 antx_rx_packet
# ============================================================================
echo "=== Fix 3: netif.rs — use antx_rx_packet ==="
python3 -c "
with open('src/kernel/net/netif.rs', 'r') as f:
    content = f.read()

old_fn = '''#[no_mangle]
pub unsafe extern \"C\" fn ethernet_input_from_e1000(
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

new_fn = '''#[no_mangle]
pub unsafe extern \"C\" fn ethernet_input_from_e1000(
    data: *mut core::ffi::c_void,
    len: u16,
) -> i32 {
    extern \"C\" {
        fn antx_rx_packet(netif: *mut core::ffi::c_void, data: *const core::ffi::c_void, len: u16) -> i32;
    }
    if G_NETIF_PTR.is_null() || data.is_null() || len == 0 {
        return LwipErr::Val as i32;
    }
    antx_rx_packet(G_NETIF_PTR, data as *const core::ffi::c_void, len)
}'''

if old_fn in content:
    content = content.replace(old_fn, new_fn)
    with open('src/kernel/net/netif.rs', 'w') as f:
        f.write(content)
    print('  netif.rs updated')
else:
    print('  WARNING: old pattern not found, trying alt')
    # Try looser match
    lines = content.split('\n')
    in_fn = False
    out = []
    for i, l in enumerate(lines):
        if 'fn ethernet_input_from_e1000' in l:
            in_fn = True
            out.append('''#[no_mangle]
pub unsafe extern \"C\" fn ethernet_input_from_e1000(
    data: *mut core::ffi::c_void,
    len: u16,
) -> i32 {
    extern \"C\" {
        fn antx_rx_packet(netif: *mut core::ffi::c_void, data: *const core::ffi::c_void, len: u16) -> i32;
    }
    if G_NETIF_PTR.is_null() || data.is_null() || len == 0 {
        return LwipErr::Val as i32;
    }
    antx_rx_packet(G_NETIF_PTR, data as *const core::ffi::c_void, len)
}''')
            continue
        if in_fn and l.strip() == '}':
            in_fn = False
            continue
        if not in_fn:
            out.append(l)
    content = '\n'.join(out)
    with open('src/kernel/net/netif.rs', 'w') as f:
        f.write(content)
    print('  netif.rs updated (alt method)')
"

# ============================================================================
# Fix 4: e1000.rs — extract_pbuf_data 支持链式 pbuf
# ============================================================================
echo "=== Fix 4: e1000.rs — chained pbuf support ==="
python3 -c "
with open('src/kernel/net/driver/e1000.rs', 'r') as f:
    content = f.read()

old_extract = '''unsafe fn extract_pbuf_data(p: *mut core::ffi::c_void) -> (usize, *mut u8) {
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

new_extract = '''unsafe fn extract_pbuf_data(p: *mut core::ffi::c_void) -> (usize, *mut u8) {
    extern \"C\" {
        fn antx_pbuf_copyout(p: *mut core::ffi::c_void, buf: *mut u8, out_len: *mut u16);
    }

    let pbuf_base = p as *mut u8;

    // pbuf 结构 (x86_64): next@0x00, payload@0x08, tot_len@0x10(u16), len@0x12(u16)
    let total = *(pbuf_base.add(0x10) as *const u16) as usize;

    // 静态缓冲区 — 单核, 锁外, 足够安全
    static mut TX_BUF: [u8; 1600] = [0u8; 1600];
    let mut out_len: u16 = total.min(1600) as u16;
    antx_pbuf_copyout(p, TX_BUF.as_mut_ptr(), &mut out_len);

    (out_len as usize, TX_BUF.as_mut_ptr())
}'''

if old_extract in content:
    content = content.replace(old_extract, new_extract)
    with open('src/kernel/net/driver/e1000.rs', 'w') as f:
        f.write(content)
    print('  e1000.rs updated')
else:
    print('  ERROR: pattern not found in e1000.rs')
    exit(1)
"

echo ""
echo "=== All source fixes applied ==="
echo "Files modified:"
echo "  src/kernel/timer/irq.rs"
echo "  src/kernel/net/arch/net_glue.c"
echo "  src/kernel/net/netif.rs"
echo "  src/kernel/net/driver/e1000.rs"
