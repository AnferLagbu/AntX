#!/bin/bash
# 综合修复 v4: IRQ enable + netif buffer size + link_up
set -e
cd /home/anfer/Code/C/AntX

cp src/kernel/net/driver/e1000.rs backup/net_dhcp_fix/e1000.rs.bak4 2>/dev/null
cp src/kernel/net/netif.rs backup/net_dhcp_fix/netif.rs.bak4 2>/dev/null
cp src/kernel/net/arch/net_glue.c backup/net_dhcp_fix/net_glue.c.bak4 2>/dev/null

echo "=== Fix A: e1000.rs — PIC IRQ enable + port I/O helpers ==="
python3 << 'PYEOF'
with open("src/kernel/net/driver/e1000.rs", "r") as f:
    content = f.read()

# Insert PIC port helper functions
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
    print("  ERROR A1: insert point not found")
    exit(1)

# Fix idt_register_irq + add PIC IRQ unmask
old_decl = """extern \"C\" {
                                fn idt_register_irq(irq: u8, handler: extern \"C\" fn(*mut core::ffi::c_void), name: *const i8, flags: u8) -> i32;
                            }
                            idt_register_irq(dev.irq, e1000_irq_entry, b\"e1000\\0\".as_ptr() as *const i8, 0);"""

new_decl = """extern \"C\" {
                                fn idt_register_irq(irq: u8, handler: extern \"C\" fn(*mut core::ffi::c_void), name: *const i8, flags: u32) -> i32;
                            }
                            idt_register_irq(dev.irq, e1000_irq_entry as extern \"C\" fn(*mut core::ffi::c_void), b\"e1000\\0\".as_ptr() as *const i8, 0);
                            if dev.irq < 8 {
                                let mask = pic_inb(0x21);
                                pic_outb(0x21, mask & !(1u8 << dev.irq));
                            } else {
                                let mask = pic_inb(0xA1);
                                pic_outb(0xA1, mask & !(1u8 << (dev.irq - 8)));
                            }"""

if old_decl in content:
    content = content.replace(old_decl, new_decl)
    print("  A: IRQ enable added")
else:
    print("  ERROR A2: idt pattern not found")

with open("src/kernel/net/driver/e1000.rs", "w") as f:
    f.write(content)
PYEOF

echo "=== Fix B: netif.rs — 扩大 G_NETIF_BUFFER 到 2048 字节 ==="
python3 << 'PYEOF'
with open("src/kernel/net/netif.rs", "r") as f:
    content = f.read()

old = "static mut G_NETIF_BUFFER: [u8; 512] = [0u8; 512];  // ✅ 静态分配"
new = "static mut G_NETIF_BUFFER: [u8; 2048] = [0u8; 2048];  // 匹配 lwIP netif 实际大小"

if old in content:
    content = content.replace(old, new)
    # Update the zero-init to match
    old_zero = "core::ptr::write_bytes(G_NETIF_BUFFER.as_mut_ptr(), 0, 512);"
    new_zero = "core::ptr::write_bytes(G_NETIF_BUFFER.as_mut_ptr(), 0, 2048);"
    content = content.replace(old_zero, new_zero)
    print("  B: buffer expanded to 2048")
else:
    print("  ERROR B: pattern not found")

with open("src/kernel/net/netif.rs", "w") as f:
    f.write(content)
PYEOF

echo "=== Fix C: net_glue.c — netif_set_link_up + hostname ==="
python3 << 'PYEOF'
with open("src/kernel/net/arch/net_glue.c", "r") as f:
    content = f.read()

# Add netif_set_link_up after setting flags
old = """    netif->output = etharp_output;
    netif->output_ip6 = ethip6_output;
    netif->linkoutput = e1000_send;
    netif->name[0] = 'e';
    netif->name[1] = 'n';
}"""

new = """    netif->output = etharp_output;
    netif->output_ip6 = ethip6_output;
    netif->linkoutput = e1000_send;
    netif->hostname = "antx";
    netif->name[0] = 'e';
    netif->name[1] = 'n';

    netif_set_link_up(netif);
}"""

if old in content:
    content = content.replace(old, new)
    print("  C: netif_set_link_up added")
else:
    print("  ERROR C: pattern not found")

with open("src/kernel/net/arch/net_glue.c", "w") as f:
    f.write(content)
PYEOF

echo ""
echo "All fixes applied"
