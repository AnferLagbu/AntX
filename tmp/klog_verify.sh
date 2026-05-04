#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf build isodir logs
mkdir -p logs isodir/boot/grub

echo "=== Build ==="
make all 2>&1 | grep -E "error:|fatal" | head -10
ls build/kernel.bin 2>/dev/null && echo "OK: $(stat -c %s build/kernel.bin) bytes" || { echo "BUILD FAIL"; make all 2>&1 | tail -10; exit 1; }

echo "=== Comp Test ==="
make test-comprehensive 2>&1 | tail -2
ls -t tests/reports/comprehensive_*.log | head -1 | while read f; do
  echo "  PASS:$(grep -c 'PASS\]' $f) FAIL:$(grep -c 'FAIL\]' $f)"
  echo "  --- Network lines ---"
  grep -E "E1000|NET|lwIP|DHCP|netif|MAC" $f | head -10
done

echo "=== QEMU Net Boot ==="
cp build/kernel.bin isodir/boot/kernel.bin
cat > isodir/boot/grub/grub.cfg << 'GEOF'
set timeout=0; set default=0
menuentry "AntX" { multiboot2 /boot/kernel.bin }
GEOF
grub2-mkrescue -o build/antx_klog.iso isodir 2>/dev/null
timeout 15 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx_klog.iso \
  -netdev user,id=n0 -device e1000,netdev=n0 \
  -serial file:logs/klog_boot.log -display none 2>/dev/null || true

echo "=== Klog Net Output ==="
grep -E "NETWORK|DRIVER.*E1000|INIT.*Net" logs/klog_boot.log | head -15
