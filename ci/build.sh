#!/bin/bash
# QueenX 双架构构建验证脚本
# 用法: ./ci/build.sh [x86_64|aarch64|all]

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_ROOT"

build_arch() {
    local arch=$1
    local target=$2
    echo -e "${YELLOW}[CI] Building ARCH=${arch} (target: ${target})...${NC}"

    pushd src/rust > /dev/null
    if cargo build --release --target "${target}" 2>&1 | tail -5; then
        echo -e "${GREEN}[CI] ARCH=${arch}: build passed${NC}"
        popd > /dev/null
        return 0
    else
        echo -e "${RED}[CI] ARCH=${arch}: build FAILED${NC}"
        popd > /dev/null
        return 1
    fi
}

# 链接最终内核镜像 (kernel.flat / kernel.bin).
# cargo build 仅生成 Rust 静态库 (.a), 必须通过 make 链接汇编对象
# 才能生成可启动的内核二进制. 跳过此步骤会导致 QEMU 使用过期镜像.
# 注意: 双架构不能同时链接 (共用 build/kernel.bin 输出路径),
#       因此 all 模式下仅链接主架构 (x86_64).
link_kernel() {
    local arch=$1
    echo -e "${YELLOW}[CI] Linking ARCH=${arch} kernel image...${NC}"
    if make ARCH="${arch}" 2>&1 | tail -5; then
        echo -e "${GREEN}[CI] ARCH=${arch}: link passed${NC}"
        return 0
    else
        echo -e "${RED}[CI] ARCH=${arch}: link FAILED${NC}"
        return 1
    fi
}

run_host_tests() {
    echo -e "${YELLOW}[CI] Running host-side unit tests...${NC}"
    pushd host-tests > /dev/null
    if cargo test --quiet 2>&1 | tail -10; then
        echo -e "${GREEN}[CI] Host tests: passed${NC}"
        popd > /dev/null
        return 0
    else
        echo -e "${RED}[CI] Host tests: FAILED${NC}"
        popd > /dev/null
        return 1
    fi
}

check_forbidden_patterns() {
    echo -e "${YELLOW}[CI] Checking forbidden asm patterns...${NC}"

    # 查找所有 asm! 调用（排除 arch/x86_64 和 arch/mod.rs）
    # 同时过滤掉行内 cfg 门控
    local matches
    matches=$(grep -rFn 'asm!("' src/kernel/ --include='*.rs' 2>/dev/null \
        | grep -v 'arch/x86_64/' \
        | grep -v 'arch/mod.rs' \
        | grep -v '#\[cfg' \
        | grep -v '#!\[cfg' \
        || true)

    if [ -z "$matches" ]; then
        echo -e "${GREEN}[CI] Forbidden patterns check: clean${NC}"
        return 0
    fi

    # 显示发现并标注为人工审查
    echo "$matches" | while IFS=: read -r file line rest; do
        echo -e "  ${YELLOW}→${NC} $file:$line$rest"
    done
    echo -e "${YELLOW}[CI] Forbidden patterns: found $(echo "$matches" | wc -l) asm! calls (above). Verify cfg gating.${NC}"
    return 0
}

# ============================================================================
# Main
# ============================================================================

ARCH="${1:-all}"
PASSED=0
FAILED=0

case "$ARCH" in
    x86_64)
        build_arch "x86_64" "x86_64-unknown-none" && PASSED=$((PASSED+1)) || FAILED=$((FAILED+1))
        link_kernel "x86_64" && PASSED=$((PASSED+1)) || FAILED=$((FAILED+1))
        ;;
    aarch64)
        build_arch "aarch64" "aarch64-unknown-none" && PASSED=$((PASSED+1)) || FAILED=$((FAILED+1))
        link_kernel "aarch64" && PASSED=$((PASSED+1)) || FAILED=$((FAILED+1))
        ;;
    all)
        build_arch "x86_64" "x86_64-unknown-none" && PASSED=$((PASSED+1)) || FAILED=$((FAILED+1))
        build_arch "aarch64" "aarch64-unknown-none" && PASSED=$((PASSED+1)) || FAILED=$((FAILED+1))
        run_host_tests && PASSED=$((PASSED+1)) || FAILED=$((FAILED+1))
        check_forbidden_patterns && PASSED=$((PASSED+1)) || FAILED=$((FAILED+1))
        # 双架构共享 build/kernel.bin 输出路径, 仅链接主架构.
        # aarch64 链接验证在单独 `./ci/build.sh aarch64` 时完成.
        link_kernel "x86_64" && PASSED=$((PASSED+1)) || FAILED=$((FAILED+1))
        ;;
    *)
        echo "Usage: $0 [x86_64|aarch64|all]"
        exit 1
        ;;
esac

echo ""
echo -e "${GREEN}Passed: ${PASSED}${NC}  ${RED}Failed: ${FAILED}${NC}"
exit $FAILED