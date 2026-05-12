#!/bin/bash
# 完整构建 + QEMU 验证
set -e
cd /home/anfer/Code/C/AntX

echo "=== Build ==="
rm -rf build
make all 2>&1 | tail -3
ls build/kernel.bin || { echo "BUILD FAILED"; exit 1; }
echo "Build OK, $(stat -c%s build/kernel.bin) bytes"

echo "=== ISO ==="
make iso 2>/dev/null

echo "=== QEMU (25s) ==="
timeout 25 qemu-system-x86_64 \
    -cdrom build/antx.iso \
    -serial stdio -display none -no-reboot \
    -m 128M \
    -device e1000,netdev=n0 -netdev user,id=n0,hostfwd=tcp::8080-:80 \
    2>&1 | tee /tmp/antx_final.log &
QPID=$!
sleep 8

echo ""
echo "=== HTTP test ==="
for i in 1 2 3 4; do
    sleep 2
    CODE=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8080/ 2>/dev/null || echo "000")
    echo "  Attempt $i: $CODE"
    if [ "$CODE" = "200" ]; then
        echo "  >>> HTTP 200 OK <<<"
        curl -s http://127.0.0.1:8080/ 2>/dev/null | head -5
        break
    fi
done

echo ""
echo "=== Key QEMU output ==="
grep -E "DHCP|bound|E1000.*OK|HTTP|Ready|error|fail" /tmp/antx_final.log | head -30

kill $QPID 2>/dev/null; wait $QPID 2>/dev/null
echo "=== Done ==="
