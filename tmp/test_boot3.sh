#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf build isodir logs
mkdir -p logs isodir/boot/grub

make all 2>&1 | tail -1

cp build/kernel.bin isodir/boot/kernel.bin
cat > isodir/boot/grub/grub.cfg << 'GEOF'
set timeout=0
set default=0
menuentry "AntX" { multiboot2 /boot/kernel.bin }
GEOF
grub2-mkrescue -o build/antx_net.iso isodir 2>/dev/null

echo "=== BOOTING ==="
timeout 15 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx_net.iso \
  -netdev user,id=n0 \
  -device e1000,netdev=n0 \
  -serial file:logs/net_boot.log \
  -display none 2>/dev/null || true
echo "=== LOG ==="
grep -E "E1000|NET|PCI|lwIP|DHCP|MAC|panic|exception|EXCEPTION|Error|Network|DMA" logs/net_boot.log | head -20
echo "=== TAIL ==="
tail -5 logs/net_boot.log
