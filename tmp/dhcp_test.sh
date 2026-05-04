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
grub2-mkrescue -o build/antx_dhcp.iso isodir 2>/dev/null

echo "=== DHCP Test (30s) ==="
timeout 30 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx_dhcp.iso \
  -netdev user,id=n0 -device e1000,netdev=n0 \
  -serial file:logs/dhcp.log -display none 2>/dev/null \
  -object filter-dump,id=f1,netdev=n0,file=/tmp/qemu_pcap.pcap 2>/dev/null || true

echo "=== DHCP/IP ==="
grep -E "DHCP|dhcp|address|netif.*ip|IP|10\.|192\.|255\." logs/dhcp.log | head -15
echo "=== last 5 ==="
tail -5 logs/dhcp.log
echo "=== Spurious ==="
grep -c "Spurious" logs/dhcp.log
