#!/bin/bash
# 回滚诊断代码, 恢复 e1000.rs 清洁版本
set -e
cd /home/anfer/Code/C/AntX
cp src/kernel/net/driver/e1000.rs backup/net_dhcp_fix/e1000.rs.bak7

python3 << 'PYEOF'
with open("src/kernel/net/driver/e1000.rs", "r") as f:
    content = f.read()

# Remove TX_CNT/RX_CNT
old_counter = """static TX_CNT: AtomicU32 = AtomicU32::new(0);
static RX_CNT: AtomicU32 = AtomicU32::new(0);

/// 全局 E1000 实例"""
new_counter = """/// 全局 E1000 实例"""
if old_counter in content:
    content = content.replace(old_counter, new_counter)
    print("  Cleaned counters")

# Restore clean e1000_send
old_send = '''            // 通过 E1000 发送
            match dev.send_packet(packet) {
                Ok(n) => {
                    let c = TX_CNT.fetch_add(1, Ordering::Relaxed);
                    if c == 0 { klog_net("e1000: first TX packet sent\\0".as_ptr() as *const i8); }
                    0
                },
                Err(_) => -1,
            }'''
new_send = '''            // 通过 E1000 发送
            match dev.send_packet(packet) {
                Ok(_) => 0,
                Err(_) => -1,
            }'''
if old_send in content:
    content = content.replace(old_send, new_send)
    print("  Cleaned send")

# Restore clean RX
old_rx = '''                        // 调用 lwIP 处理数据包
                        unsafe {
                            ethernet_input_from_e1000(
                                self.rx_buffers[self.rx_tail] as *mut core::ffi::c_void,
                                len as u16
                            );
                            let c = RX_CNT.fetch_add(1, Ordering::Relaxed);
                            if c == 0 { klog_net("e1000: first RX packet received\\0".as_ptr() as *const i8); }
                        }'''
new_rx = '''                        // 调用 lwIP 处理数据包
                        unsafe {
                            ethernet_input_from_e1000(
                                self.rx_buffers[self.rx_tail] as *mut core::ffi::c_void,
                                len as u16
                            );
                        }'''
if old_rx in content:
    content = content.replace(old_rx, new_rx)
    print("  Cleaned RX")

with open("src/kernel/net/driver/e1000.rs", "w") as f:
    f.write(content)
PYEOF

echo "Done"
