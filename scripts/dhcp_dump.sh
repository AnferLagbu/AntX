#!/bin/bash
# 用途: 编译并运行带 E1000 寄存器 dump 的版本
set -e

cd /home/anfer/Code/C/AntX

echo "=== Building ==="
rm -rf build/
cd src/rust && cargo clean 2>/dev/null && cd /home/anfer/Code/C/AntX
make all 2>&1 | tail -5
make build/kernel.flat 2>&1 | tail -5

echo ""
echo "=== Running with dump (45s) ==="
mkdir -p logs
timeout 45 qemu-system-x86_64 \
    -m 512 -no-reboot \
    -kernel build/kernel.flat \
    -device e1000,netdev=n0,debug=true \
    -netdev user,id=n0,hostfwd=tcp::8080-:80,dump=/home/anfer/Code/C/AntX/logs/net.pcap \
    -serial file:logs/dhcp_dump.log \
    -display none \
    -trace e1000\* 2>logs/qemu_trace.log || true

echo ""
echo "=== DHCP/Network ==="
grep -iE "DHCP|e1000|send|RX processed|link stat|NET\]|RCTL|STATUS|dump" logs/dhcp_dump.log | head -60
echo ""
echo "=== Trace (if any) ==="
head -30 logs/qemu_trace.log