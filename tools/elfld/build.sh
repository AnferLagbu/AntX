#!/bin/bash
# elfld.so 构建脚本
#
# 用法: ./build.sh [arch]
# arch: x86_64 (默认) | aarch64 | riscv64
#
# 依赖: gcc/clang, musl-gcc (可选)
# 产出: elfld.so (ELF 动态链接器)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ARCH="${1:-x86_64}"
OUTPUT="${SCRIPT_DIR}/elfld.so"

echo "=== 构建 elfld.so (arch=$ARCH) ==="

# 检查编译器
CC=""
if command -v musl-gcc &>/dev/null; then
    CC="musl-gcc"
elif command -v gcc &>/dev/null; then
    CC="gcc"
elif command -v clang &>/dev/null; then
    CC="clang"
else
    echo "ERROR: 未找到 gcc/clang"
    exit 1
fi

echo "编译器: $CC"

# 架构特定标志
ARCH_FLAGS=""
TARGET=""
case "$ARCH" in
    x86_64)
        ARCH_FLAGS="-m64"
        TARGET="x86_64-unknown-none"
        ;;
    aarch64)
        ARCH_FLAGS=""
        TARGET="aarch64-unknown-none"
        ;;
    riscv64)
        ARCH_FLAGS=""
        TARGET="riscv64-unknown-none"
        ;;
    *)
        echo "ERROR: 不支持的架构: $ARCH"
        exit 1
        ;;
esac

# 编译 elfld.so
# -shared: 共享库
# -fPIC: 位置无关代码
# -nostdlib: 无标准库 (自包含)
# -nostartfiles: 无启动文件
# -e _start: 入口点
$CC -shared -fPIC -nostdlib -nostartfiles \
    $ARCH_FLAGS \
    -e _start \
    -o "$OUTPUT" \
    "${SCRIPT_DIR}/elfld.c"

echo "产出: $OUTPUT"
ls -la "$OUTPUT"
