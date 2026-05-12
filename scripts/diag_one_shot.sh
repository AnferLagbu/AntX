#!/bin/bash
# 一次性的最小诊断 e1000_send 调用
set -e
cd /home/anfer/Code/C/AntX
cp src/kernel/net/driver/e1000.rs backup/net_dhcp_fix/e1000.rs.bak9

python3 << 'PYEOF'
with open("src/kernel/net/driver/e1000.rs", "r") as f:
    c = f.read()

# Add a static bool for one-time diagnostic
old1 = '/// 全局 E1000 实例'
new1 = 'static mut TX_FIRST: bool = true;\n/// 全局 E1000 实例'
c = c.replace(old1, new1)

# Add klog in e1000_send's send path
old2 = '''        let packet = core::slice::from_raw_parts(data_ptr as *const u8, total_len);

            // 通过 E1000 发送
            match dev.send_packet(packet) {
                Ok(_) => 0,
                Err(_) => -1,
            }'''
new2 = '''        let packet = core::slice::from_raw_parts(data_ptr as *const u8, total_len);

            if TX_FIRST {
                TX_FIRST = false;
                extern "C" { fn klog_net(fmt: *const i8); }
                klog_net("e1000: TX working\\0".as_ptr() as *const i8);
            }

            // 通过 E1000 发送
            match dev.send_packet(packet) {
                Ok(_) => 0,
                Err(_) => -1,
            }'''
c = c.replace(old2, new2)

with open("src/kernel/net/driver/e1000.rs", "w") as f:
    f.write(c)
print("  OK")
PYEOF
echo "Done"
