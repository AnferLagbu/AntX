#!/bin/bash
# Fix 2 v3: E1000 IRQ enable via direct PIC port I/O (using asm!)
set -e
cd /home/anfer/Code/C/AntX

cp src/kernel/net/driver/e1000.rs backup/net_dhcp_fix/e1000.rs.bak3

python3 << 'PYEOF'
with open("src/kernel/net/driver/e1000.rs", "r") as f:
    content = f.read()

# Insert PIC port helper functions, then fix the IRQ enable call
# We'll add helpers right before e1000_init

old_insert = """/// 全局 E1000 实例
static mut E1000_INSTANCE: Option<E1000Device> = None;"""

new_insert = """unsafe fn pic_outb(port: u16, value: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
}
unsafe fn pic_inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack));
    value
}

/// 全局 E1000 实例
static mut E1000_INSTANCE: Option<E1000Device> = None;"""

if old_insert in content:
    content = content.replace(old_insert, new_insert)
else:
    print("  ERROR: insert point not found")
    exit(1)

# Fix idt_register_irq call + add IRQ enable
old_decl = """                            extern "C" {
                                fn idt_register_irq(irq: u8, handler: extern "C" fn(*mut core::ffi::c_void), name: *const i8, flags: u8) -> i32;
                            }
                            idt_register_irq(dev.irq, e1000_irq_entry, b"e1000\\0".as_ptr() as *const i8, 0);"""

new_decl = """                            extern "C" {
                                fn idt_register_irq(irq: u8, handler: extern "C" fn(*mut core::ffi::c_void), name: *const i8, flags: u32) -> i32;
                            }
                            idt_register_irq(dev.irq, e1000_irq_entry as extern "C" fn(*mut core::ffi::c_void), b"e1000\\0".as_ptr() as *const i8, 0);
                            // 在 PIC 上取消屏蔽 IRQ 线 (使能硬件中断)
                            if dev.irq < 8 {
                                let mask = pic_inb(0x21);
                                pic_outb(0x21, mask & !(1u8 << dev.irq));
                            } else {
                                let mask = pic_inb(0xA1);
                                pic_outb(0xA1, mask & !(1u8 << (dev.irq - 8)));
                            }"""

if old_decl in content:
    content = content.replace(old_decl, new_decl)
    with open("src/kernel/net/driver/e1000.rs", "w") as f:
        f.write(content)
    print("  OK: e1000.rs updated")
else:
    print("  ERROR: idt pattern not found")
    exit(1)
PYEOF

echo "Done"
