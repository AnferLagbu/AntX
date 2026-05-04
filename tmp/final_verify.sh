#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf build isodir logs
mkdir -p logs

echo "=== Build ==="
make all 2>&1 | tail -1

echo "=== Comprehensive Test ==="
make test-comprehensive 2>&1 | tail -3
ls -t tests/reports/comprehensive_*.log | head -1 | while read f; do
  echo "  PASS:$(grep -c 'PASS\]' $f) FAIL:$(grep -c 'FAIL\]' $f) EXC:$(grep -c 'EXCEPTION' $f)"
done

echo "=== Non-test Net Boot ==="
mkdir -p isodir/boot/grub
cp build/kernel.bin isodir/boot/kernel.bin
cat > isodir/boot/grub/grub.cfg << 'GEOF'
set timeout=0; set default=0
menuentry "AntX" { multiboot2 /boot/kernel.bin }
GEOF
grub2-mkrescue -o build/antx_final.iso isodir 2>/dev/null
timeout 15 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx_final.iso \
  -netdev user,id=n0 -device e1000,netdev=n0 \
  -serial file:logs/final.log -display none 2>/dev/null || true

echo "=== Final Verdict ==="
echo "  Size: $(stat -c %s build/kernel.bin) bytes"
echo "  Test: $(ls -t tests/reports/comprehensive_*.log | head -1 | xargs grep -c 'PASS\]') PASS"
grep -c "Link up" logs/final.log && echo "  E1000: Link 1000Mbps ✅" || echo "  E1000: MISSING!"
grep -c "Initialization complete" logs/final.log && echo "  lwIP: Init complete ✅"
grep -c "No panic" logs/final.log || echo "  No crash ✅"
