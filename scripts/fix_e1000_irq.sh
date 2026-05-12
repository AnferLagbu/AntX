#!/bin/bash
# Fix 2: E1000 IRQ enable + type fix
set -e
cd /home/anfer/Code/C/AntX

# Backup
cp src/kernel/net/driver/e1000.rs backup/net_dhcp_fix/e1000.rs.bak2

python3 -c '
with open("src/kernel/net/driver/e1000.rs", "r") as f:
    content = f.read()

# Fix 1: idt_register_irq 声明中的 flags: u8 → u32
old_decl = """                            extern "C" {
                                fn idt_register_irq(irq: u8, handler: extern "C" fn(*mut core::ffi::c_void), name: *const i8, flags: u8) -> i32;
                            }
                            idt_register_irq(dev.irq, e1000_irq_entry, b"e1000\\0".as_ptr() as *const i8, 0);"""
new_decl = """                            extern "C" {
                                fn idt_register_irq(irq: u8, handler: extern "C" fn(*mut core::ffi::c_void), name: *const i8, flags: u32) -> i32;
                                fn idt_enable_irq(irq: u8);
                            }
                            idt_register_irq(dev.irq, e1000_irq_entry as extern "C" fn(*mut core::ffi::c_void), b"e1000\\0".as_ptr() as *const i8, 0);
                            // 启用 PCI IRQ 线 (PIC 掩码)
                            idt_enable_irq(dev.irq);"""
if old_decl in content:
    content = content.replace(old_decl, new_decl)
    with open("src/kernel/net/driver/e1000.rs", "w") as f:
        f.write(content)
    print("  e1000.rs: IRQ enable + type fix")
else:
    print("  ERROR: pattern not found")
    exit(1)
'

echo "Done"
