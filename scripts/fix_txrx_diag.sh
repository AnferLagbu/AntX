#!/bin/bash
# 最终修复: TX/RX 路径验证 + DHCP 调优
set -e
cd /home/anfer/Code/C/AntX

cp src/kernel/net/driver/e1000.rs backup/net_dhcp_fix/e1000.rs.bak6

python3 << 'PYEOF'
with open("src/kernel/net/driver/e1000.rs", "r") as f:
    content = f.read()

# Add atomic TX counter for diagnostics
insert_pos = 'use crate::kernel::driver::framework::{Driver, DeviceType, DriverError, Result};'
new_import = '''use crate::kernel::driver::framework::{Driver, DeviceType, DriverError, Result};
use core::sync::atomic::AtomicU32;'''

if insert_pos in content:
    content = content.replace(insert_pos, new_import)
else:
    print("  WARNING: import insert point not found")

# Add global TX counter
counter_pos = '/// 全局 E1000 实例'
new_counter = '''static TX_CNT: AtomicU32 = AtomicU32::new(0);
static RX_CNT: AtomicU32 = AtomicU32::new(0);

/// 全局 E1000 实例'''
if counter_pos in content:
    content = content.replace(counter_pos, new_counter)
else:
    print("  WARNING: counter insert point not found")

# Log TX in e1000_send
old_send = '''            // 通过 E1000 发送
            match dev.send_packet(packet) {
                Ok(_) => 0,  // ERR_OK
                Err(_) => -1,
            }'''
new_send = '''            // 通过 E1000 发送
            match dev.send_packet(packet) {
                Ok(n) => {
                    let c = TX_CNT.fetch_add(1, Ordering::Relaxed);
                    if c == 0 { klog_net("e1000: first TX packet sent\\0".as_ptr() as *const i8); }
                    0
                },
                Err(_) => -1,
            }'''

if old_send in content:
    content = content.replace(old_send, new_send)
    print("  OK: TX logging added")
else:
    print("  WARNING: send pattern not found")

# Log RX in ethernet_input_from_e1000 caller (process_rx)
old_rx = '''                        // 调用 lwIP ethernet_input 处理数据包
                        unsafe {
                            ethernet_input_from_e1000(
                                self.rx_buffers[self.rx_tail] as *mut core::ffi::c_void,
                                len as u16
                            );
                        }'''
new_rx = '''                        // 调用 lwIP 处理数据包
                        unsafe {
                            ethernet_input_from_e1000(
                                self.rx_buffers[self.rx_tail] as *mut core::ffi::c_void,
                                len as u16
                            );
                            let c = RX_CNT.fetch_add(1, Ordering::Relaxed);
                            if c == 0 { klog_net("e1000: first RX packet received\\0".as_ptr() as *const i8); }
                        }'''

if old_rx in content:
    content = content.replace(old_rx, new_rx)
    print("  OK: RX logging added")
else:
    print("  WARNING: rx pattern not found")

with open("src/kernel/net/driver/e1000.rs", "w") as f:
    f.write(content)
PYEOF

echo "Done"
