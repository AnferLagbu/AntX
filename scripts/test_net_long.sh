#!/bin/bash
# Long QEMU test: 60s with HTTP polling
set -e
cd /home/anfer/Code/C/AntX

echo "=== Building ==="
rm -rf build/net build/kernel.bin 2>/dev/null
make all 2>&1 | tail -2 && make iso 2>/dev/null

echo "=== Starting QEMU (60s) ==="
timeout 60 qemu-system-x86_64 \
    -cdrom build/antx.iso \
    -serial file:/tmp/antx_serial.log \
    -display none -no-reboot \
    -m 128M \
    -device e1000,netdev=n0 -netdev user,id=n0,hostfwd=tcp::8080-:80 \
    2>/dev/null &
QPID=$!

echo "QEMU PID: $QPID"
sleep 8

echo "=== Polling HTTP ==="
for i in $(seq 1 20); do
    sleep 2
    RESP=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:8080/ 2>/dev/null || echo "000")
    echo "  t=$(($i*2+8))s  HTTP: $RESP"
    if [ "$RESP" = "200" ]; then
        echo ">>> HTTP 200 — SUCCESS <<<"
        curl -s http://127.0.0.1:8080/ 2>/dev/null | head -5
        break
    fi
done

echo ""
echo "=== Serial log (key lines) ==="
grep -E "DHCP|E1000|bound|HTTP|Ready|error|fail|NET\]|Status" /tmp/antx_serial.log 2>/dev/null | tail -30

kill $QPID 2>/dev/null; wait $QPID 2>/dev/null
echo "=== Done ==="
