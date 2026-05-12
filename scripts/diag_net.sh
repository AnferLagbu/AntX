#!/bin/bash
# 诊断网络栈 — 捕获 QEMU 完整输出
set -e
cd /home/anfer/Code/C/AntX

echo "=== Step 0: Build ==="
rm -rf build && make all 2>&1 | tail -5 && make iso 2>/dev/null
echo "=== Step 1: QEMU run-net (25s) ==="
timeout 25 qemu-system-x86_64 \
    -cdrom build/antx.iso \
    -serial stdio -display none -no-reboot \
    -m 128M \
    -device e1000,netdev=n0 -netdev user,id=n0,hostfwd=tcp::8080-:80 \
    2>&1 | tee /tmp/antx_net_diag.log

echo "EXIT:$?"
echo "=== Step 2: Key lines ==="
grep -E "DHCP|E1000|netif|e1000_send|ethernet_input|HTTP|bound|IRQ|Status|TCP|Link" /tmp/antx_net_diag.log | head -40
