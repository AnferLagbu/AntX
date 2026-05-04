#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf build isodir
make all 2>&1 | tail -2
mkdir -p isodir/boot/grub
cp build/kernel.bin isodir/boot/kernel.bin
cat > /tmp/grub_cfg << 'GEOF'
set timeout=0
set default=0
menuentry "AntX" { multiboot2 /boot/kernel.bin }
GEOF
cp /tmp/grub_cfg isodir/boot/grub/grub.cfg
grub2-mkrescue -o build/antx_test.iso isodir 2>/dev/null
echo "=== BOOTING ==="
timeout 12 qemu-system-x86_64 -m 512 -M q35 -no-reboot \
  -cdrom build/antx_test.iso \
  -netdev user,id=n0 -device e1000,netdev=n0 \
  -nographic -serial stdio 2>/dev/null | head -50
echo "=== END ==="
