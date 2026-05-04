#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf build isodir logs
mkdir -p logs isodir/boot/grub

echo "=== Building ==="
make all 2>&1 | tail -1
ls -la build/kernel.bin

# Non-test boot
echo "=== Boot test ==="
cp build/kernel.bin isodir/boot/kernel.bin
cat > isodir/boot/grub/grub.cfg << 'GEOF'
set timeout=0
set default=0
menuentry "AntX" { multiboot2 /boot/kernel.bin }
GEOF
grub2-mkrescue -o build/antx_net.iso isodir 2>/dev/null

timeout 12 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx_net.iso \
  -netdev user,id=n0 -device e1000,netdev=n0 \
  -serial file:logs/boot3.log -display none 2>/dev/null || true

echo "=== Result ==="
grep -E "E1000|NET|PCI|lwIP|DHCP|MAC" logs/boot3.log | head -20
echo "--- tail ---"
tail -3 logs/boot3.log
