#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf isodir logs
mkdir -p logs isodir/boot/grub

echo "=== Comprehensive Test ==="
make test-comprehensive 2>&1 | tail -2
ls -t tests/reports/comprehensive_*.log | head -1 | while read f; do
  echo "  PASS:$(grep -c 'PASS\]' $f) FAIL:$(grep -c 'FAIL\]' $f)"
done

echo "=== QEMU + HTTP test (hostfwd:8080→80, 25s timeout) ==="
cp build/kernel.bin isodir/boot/kernel.bin
echo 'set timeout=0; set default=0; menuentry "AntX" { multiboot2 /boot/kernel.bin }' > isodir/boot/grub/grub.cfg
grub2-mkrescue -o build/antx_p5f.iso isodir 2>/dev/null

timeout 25 qemu-system-x86_64 -m 512 -no-reboot \
  -cdrom build/antx_p5f.iso \
  -netdev user,id=n0,hostfwd=tcp::8080-:80 \
  -device e1000,netdev=n0 \
  -serial file:logs/p5f.log -display none 2>/dev/null || true

echo "=== Net/App Lines ==="
grep -E "NETWORK|DRIVER.*E1000|HTTP|DNS|static" logs/p5f.log | head -15
echo "---"
grep -E "DNS.*found|DNS.* →" logs/p5f.log
echo "Lines: $(wc -l < logs/p5f.log)"

echo "=== HTTP Test ==="
# Try to access HTTP server from host
curl -s --max-time 3 http://localhost:8080/ 2>/dev/null && echo "HTTP 200 OK" || echo "HTTP failed or timed out"
curl -s --max-time 3 http://localhost:8080/index.html 2>/dev/null && echo "index.html OK" || echo "index.html failed"
