#!/bin/bash
# 最小化诊断: 添加一次性 TX 日志验证 send 是否被调用
set -e
cd /home/anfer/Code/C/AntX
cp src/kernel/net/driver/e1000.rs backup/net_dhcp_fix/e1000.rs.bak8

python3 << 'PYEOF'
with open("src/kernel/net/driver/e1000.rs", "r") as f:
    lines = f.readlines()

# Find e1000_send function and add klog_net extern + log call
new_lines = []
in_send = False
found = False
for i, line in enumerate(lines):
    if 'pub extern "C" fn e1000_send' in line:
        in_send = True
        found = True
        new_lines.append(line)
        continue
    if in_send and 'unsafe {' in line:
        new_lines.append(line)
        new_lines.append('        extern "C" { fn klog_net(fmt: *const i8); }\n')
        continue
    if in_send and 'if E1000_INSTANCE.is_none()' in line:
        new_lines.append('        klog_net("e1000_send called\\0".as_ptr() as *const i8);\n')
        new_lines.append(line)
        in_send = False
        continue
    new_lines.append(line)

if found:
    with open("src/kernel/net/driver/e1000.rs", "w") as f:
        f.writelines(new_lines)
    print("  OK: send diagnostic added")
else:
    print("  ERROR: e1000_send not found")

# Also add diagnostic in handle_interrupt
lines2 = new_lines
new_lines2 = []
in_isr = False
for i, line in enumerate(lines2):
    if 'pub fn handle_interrupt' in line:
        in_isr = True
        new_lines2.append(line)
        continue
    if in_isr and 'self.isr_count += 1;' in line:
        new_lines2.append(line)
        new_lines2.append('        if self.isr_count == 1 {\n')
        new_lines2.append('            unsafe { extern "C" { fn klog_net(fmt: *const i8); } klog_net("e1000 ISR fired\\0".as_ptr() as *const i8); }\n')
        new_lines2.append('        }\n')
        in_isr = False
        continue
    new_lines2.append(line)

with open("src/kernel/net/driver/e1000.rs", "w") as f:
    f.writelines(new_lines2)
print("  OK: ISR diagnostic added")
PYEOF

echo "Done"
