#!/bin/bash
# Fix 2 v2: E1000 IRQ enable via direct PIC port I/O + type fix
set -e
cd /home/anfer/Code/C/AntX

cp src/kernel/net/driver/e1000.rs backup/net_dhcp_fix/e1000.rs.bak2

python3 << 'PYEOF'
with open("src/kernel/net/driver/e1000.rs", "r") as f:
    content = f.read()

# Fix: 声明修正 + 直接操作 PIC 取消屏蔽 IRQ
old_decl = """                            extern "C" {
                                fn idt_register_irq(irq: u8, handler: extern "C" fn(*mut core::ffi::c_void), name: *const i8, flags: u8) -> i32;
                            }
                            idt_register_irq(dev.irq, e1000_irq_entry, b"e1000\\0".as_ptr() as *const i8, 0);"""

new_decl = """                            extern "C" {
                                fn idt_register_irq(irq: u8, handler: extern "C" fn(*mut core::ffi::c_void), name: *const i8, flags: u32) -> i32;
                            }
                            idt_register_irq(dev.irq, e1000_irq_entry as extern "C" fn(*mut core::ffi::c_void), b"e1000\\0".as_ptr() as *const i8, 0);
                            // 直接在 PIC 上取消屏蔽 IRQ 线
                            if dev.irq < 8 {
                                let mask = core::arch::x86_64::__inbyte(0x21);
                                core::arch::x86_64::__outbyte(0x21, mask & !(1u8 << dev.irq));
                            } else {
                                let mask = core::arch::x86_64::__inbyte(0xA1);
                                core::arch::x86_64::__outbyte(0xA1, mask & !(1u8 << (dev.irq - 8)));
                            }"""

if old_decl in content:
    content = content.replace(old_decl, new_decl)
    with open("src/kernel/net/driver/e1000.rs", "w") as f:
        f.write(content)
    print("  OK: e1000.rs updated")
else:
    print("  ERROR: pattern not found")
    exit(1)
PYEOF

echo "Done"
