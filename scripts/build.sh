#!/bin/bash

set -e

echo "=========================================="
echo "        AntX Kernel Build Script         "
echo "=========================================="

echo ""
echo "[1/4] Cleaning build directory..."
rm -rf build/
mkdir -p build/

echo ""
echo "[2/4] Ensuring logs directory exists..."
mkdir -p logs/

echo ""
echo "[3/4] Compiling kernel..."
make all

echo ""
echo "[4/4] Build complete!"
echo ""
echo "Kernel binary: build/kernel.bin"
echo ""
echo "Available targets:"
echo "  make run   - Run kernel in QEMU with serial output to console"
echo "  make debug - Run kernel with GDB debugging support"
echo "  make log   - Run kernel and save serial log to logs/serial.log"
