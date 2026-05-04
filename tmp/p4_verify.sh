#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf build isodir logs
mkdir -p logs isodir/boot/grub

echo "=== Building ==="
make all 2>&1 | grep -E "error:|warning:|built" | head -10
ls -la build/kernel.bin 2>/dev/null && echo "OK: $(stat -c %s build/kernel.bin) bytes" || { echo "BUILD FAILED"; make all 2>&1 | tail -10; exit 1; }

echo "=== Comprehensive Test ==="
make test-comprehensive 2>&1 | tail -3
ls -t tests/reports/comprehensive_*.log | head -1 | while read f; do
  echo "  PASS:$(grep -c 'PASS\]' $f) FAIL:$(grep -c 'FAIL\]' $f) EXC:$(grep -c 'EXCEPTION' $f)"
done

echo "=== Phase 4 Net Boot ==="
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

echo "=== E1000 / lwIP / DHCP ==="
grep -E "E1000|NET|lwIP|DHCP|MAC|IRQ|IP|address|10\.|Link|link|timeout" logs/p4.log | head -25
echo "--- last 5 ---"
tail -5 logs/p4.log
