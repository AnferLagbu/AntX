#!/bin/bash
set -e
cd /home/anfer/Code/C/AntX
rm -rf build isodir logs
mkdir -p logs isodir/boot/grub

echo "=== Building ==="
make all 2>&1 | tail -1

# ---- 综合测试 (KERNEL_TEST=1) ----
echo "=== Comprehensive Test ==="
make test-comprehensive 2>&1 | tail -2
ls -t tests/reports/comprehensive_*.log | head -1 | while read f; do
  echo "  PASS:$(grep -c 'PASS\]' $f)  FAIL:$(grep -c 'FAIL\]' $f)  EXC:$(grep -c 'EXCEPTION' $f)"
done

# ---- 非测试模式网络启动 ----
echo "=== Non-test Boot ==="
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
  -serial file:logs/net_boot2.log -display none 2>/dev/null || true

echo "=== Network boot log ==="
grep -E "E1000|NET|PCI|lwIP|DHCP|MAC|Network|DMA|panic|exception" logs/net_boot2.log | head -25
echo "--- last 3 ---"
tail -3 logs/net_boot2.log
