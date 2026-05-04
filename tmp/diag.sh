#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf build isodir logs
mkdir -p logs isodir/boot/grub
make all 2>&1 | tail -1
cp build/kernel.bin isodir/boot/kernel.bin
cat > isodir/boot/grub/grub.cfg << 'GEOF'
set timeout=0; set default=0
menuentry "AntX" { multiboot2 /boot/kernel.bin }
GEOF
grub2-mkrescue -o build/antx.iso isodir 2>/dev/null
timeout 12 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx.iso \
  -netdev user,id=n0 -device e1000,netdev=n0 \
  -serial file:logs/boot.log -display none 2>/dev/null || true
echo "=== Devices found ==="
grep "PCI dev:" logs/boot.log | head -20
echo "=== E1000/NET ==="
grep "E1000\|NET\|lwIP" logs/boot.log
