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
grub2-mkrescue -o build/antx_p5b.iso isodir 2>/dev/null

echo "=== Long Boot (35s) ==="
timeout 35 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx_p5b.iso \
  -netdev user,id=n0 -device e1000,netdev=n0 \
  -serial file:logs/p5b.log -display none 2>/dev/null || true

echo "=== All Net/App Lines ==="
grep -E "NETWORK|Ping|HTTP|DNS|DHCP|netif status" logs/p5b.log | head -20
echo "--- last 3 ---"
tail -3 logs/p5b.log
echo "--- total lines ---"
wc -l logs/p5b.log
