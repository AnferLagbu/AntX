#!/bin/bash
# 编译 + QEMU 测试
set -e
cd /home/anfer/Code/C/AntX

echo "=== Step 1: Rust check ==="
cd src/rust && cargo check 2>&1 | grep -E "^error" | head -5
echo "  Rust check done"

echo "=== Step 2: Build ==="
cd /home/anfer/Code/C/AntX
rm -rf build && make all 2>&1 | tail -5
ls build/kernel.bin && echo "  Build: OK" || { echo "  Build: FAILED"; exit 1; }

echo "=== Step 3: ISO ==="
make iso 2>/dev/null

echo "=== Step 4: QEMU run-net (25s) ==="
# 后台启动并尝试 HTTP
timeout 25 qemu-system-x86_64 \
    -cdrom build/antx.iso \
    -serial stdio -display none -no-reboot \
    -m 128M \
    -device e1000,netdev=n0 -netdev user,id=n0,hostfwd=tcp::8080-:80 \
    -object filter-dump,id=f1,netdev=n0,file=/tmp/antx_net.pcap \
    2>&1 | tee /tmp/antx_net_test.log &
QPID=$!

# 等待启动
sleep 6

# 尝试 HTTP
echo "=== Step 5: HTTP test ==="
for i in 1 2 3 4 5; do
    sleep 2
    CODE=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8080/ 2>/dev/null || echo "000")
    echo "  Attempt $i: HTTP $CODE"
    if [ "$CODE" != "000" ]; then
        echo "  HTTP RESPONSE: $CODE — SUCCESS!"
        break
    fi
done

sleep 3
kill $QPID 2>/dev/null; wait $QPID 2>/dev/null
echo "=== Done ==="
