#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf isodir logs
mkdir -p logs isodir/boot/grub

echo "=== Comprehensive Test ==="
make test-comprehensive 2>&1 | tail -3
ls -t tests/reports/comprehensive_*.log | head -1 | while read f; do
  echo "  PASS:$(grep -c 'PASS\]' $f) FAIL:$(grep -c 'FAIL\]' $f) EXC:$(grep -c 'EXCEPTION' $f)"
done
echo "  Size: $(stat -c %s build/kernel.bin) bytes"

echo "=== Phase 5 QEMU Net Boot (25s) ==="
cp build/kernel.bin isodir/boot/kernel.bin
cat > isodir/boot/grub/grub.cfg << 'GEOF'
set timeout=0; set default=0
menuentry "AntX" { multiboot2 /boot/kernel.bin }
GEOF
grub2-mkrescue -o build/antx_p5.iso isodir 2>/dev/null

timeout 25 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx_p5.iso \
  -netdev user,id=n0 -device e1000,netdev=n0 \
  -serial file:logs/p5.log -display none 2>/dev/null || true

echo "=== Network Output ==="
grep -E "NETWORK|DRIVER.*E1000|INIT.*Net|Ping|HTTP|DNS" logs/p5.log | head -20
echo "---"
grep -E "DHCP|netif status" logs/p5.log | head -5
