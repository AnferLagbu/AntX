#!/bin/bash
# 用途: 编译网络栈并运行 DHCP 测试
set -e

cd /home/anfer/Code/C/AntX

echo "=========================================="
echo "  [STEP 1/4] Cleaning previous build..."
echo "=========================================="
rm -rf build/
cd src/rust && cargo clean 2>/dev/null && cd /home/anfer/Code/C/AntX

echo ""
echo "=========================================="
echo "  [STEP 2/4] Building kernel + network stack..."
echo "=========================================="
make all 2>&1 | tail -40

echo ""
echo "=========================================="
echo "  [STEP 3/4] Building kernel.flat..."
echo "=========================================="
make build/kernel.flat

echo ""
echo "=========================================="
echo "  [STEP 4/4] Running with network (30s timeout)..."
echo "=========================================="
mkdir -p logs
timeout 35 qemu-system-x86_64 \
    -m 512 \
    -no-reboot \
    -kernel build/kernel.flat \
    -device e1000,netdev=n0 \
    -netdev user,id=n0,hostfwd=tcp::8080-:80,hostname=antx \
    -serial file:logs/dhcp_test.log \
    -display none \
    -d cpu_reset,guest_errors 2>logs/qemu_stderr.log || true

echo ""
echo "=========================================="
echo "  Results:"
echo "=========================================="
echo ""
echo "--- DHCP/Network related logs ---"
grep -iE "DHCP|NETWORK|E1000|lwIP|netif|IP addr|bound|offer|discover|10\.0\.2" logs/dhcp_test.log | head -40
echo ""
echo "--- Last 30 lines ---"
tail -30 logs/dhcp_test.log