#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf isodir logs
mkdir -p logs isodir/boot/grub

echo "=== Net Boot (20s) ==="
cp build/kernel.bin isodir/boot/kernel.bin
cat > isodir/boot/grub/grub.cfg << 'GEOF'
set timeout=0; set default=0
menuentry "AntX" { multiboot2 /boot/kernel.bin }
GEOF
grub2-mkrescue -o build/antx_p4.iso isodir 2>/dev/null

timeout 20 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx_p4.iso \
  -netdev user,id=n0 -device e1000,netdev=n0 \
  -serial file:logs/p4.log -display none 2>/dev/null || true

echo "=== Key Lines ==="
grep -E "E1000|NET|DHCP|MAC|IRQ|address|10\." logs/p4.log | head -20
echo "--- last 3 ---"
tail -3 logs/p4.log
echo "--- Test Results ---"
echo "  Size: $(stat -c %s build/kernel.bin) bytes"
echo "  Link: $(grep -c 'Link up' logs/p4.log)"
echo "  DHCP: $(grep -c 'DHCP\|dhcp' logs/p4.log)"
echo "  Spurious: $(grep -c 'Spurious' logs/p4.log)"
