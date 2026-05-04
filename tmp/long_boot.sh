#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf isodir logs
mkdir -p logs isodir/boot/grub

cp build/kernel.bin isodir/boot/kernel.bin
cat > isodir/boot/grub/grub.cfg << 'GEOF'
set timeout=0; set default=0
menuentry "AntX" { multiboot2 /boot/kernel.bin }
GEOF
grub2-mkrescue -o build/antx_net.iso isodir 2>/dev/null

echo "=== Boot with 30s timeout ==="
timeout 30 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx_net.iso \
  -netdev user,id=n0 -device e1000,netdev=n0 \
  -serial file:logs/long_boot.log -display none 2>/dev/null || true

echo "=== DHCP/E1000/NET ==="
grep -E "DHCP|lwIP|E1000|NET|IP|address|10\.|Link|link" logs/long_boot.log | head -20
echo "=== last 5 ==="
tail -5 logs/long_boot.log
