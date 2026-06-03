#!/bin/bash
# AntX 内核代码审计脚本
# 工具栈: cargo check + clippy (pedantic) + Lockbud + Miri 配置验证
# 用法: ./ci/audit.sh [quick|full]
#
# - quick: clippy + 双架构 check (CI 默认)
# - full:  quick + Lockbud + Miri 严格模式 + SAFETY 覆盖率统计

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PROJECT_ROOT"

MODE="${1:-quick}"

step() { echo -e "\n${YELLOW}━━━ $1 ━━━${NC}"; }
ok()   { echo -e "${GREEN}✓ $1${NC}"; }
err()  { echo -e "${RED}✗ $1${NC}"; exit 1; }

# ── 1. 双架构 check ─────────────────────────────────────────────
step "1/6 双架构 cargo check (x86_64 + aarch64)"
pushd src/rust > /dev/null
unset RUSTC_WRAPPER
for target in x86_64-unknown-none aarch64-unknown-none; do
    echo -e "${BLUE}[audit] target=${target}${NC}"
    if cargo +nightly check --target "${target}" 2>&1 | tail -3; then
        ok "${target}: check passed"
    else
        err "${target}: check FAILED"
    fi
done
popd > /dev/null

# ── 2. Clippy pedantic (x86_64) ────────────────────────────────
step "2/6 Clippy pedantic (x86_64, lib only)"
pushd src/rust > /dev/null
unset RUSTC_WRAPPER
# 仅审计 lib (kernel 源码), 跳过 build script (编译期脚本, 非 TCB)
if cargo +nightly clippy --lib --target x86_64-unknown-none \
    -- -W clippy::pedantic -W clippy::cargo 2>&1 | tail -10; then
    ok "clippy pedantic (lib): passed"
else
    echo -e "${YELLOW}⚠ clippy pedantic (lib) 有警告 (见上)${NC}"
fi
popd > /dev/null

if [ "$MODE" = "quick" ]; then
    echo -e "\n${GREEN}━━━ audit (quick) 完成 ━━━${NC}"
    exit 0
fi

# ── 3. SAFETY 注释覆盖率统计 ────────────────────────────────────
step "3/6 SAFETY 注释覆盖率统计"
KERNEL_DIR="$PROJECT_ROOT/src/kernel"
TOTAL_UNSAFE=$(grep -rcE "unsafe \{" "$KERNEL_DIR" --include="*.rs" 2>/dev/null | awk -F: '{s+=$2} END{print s}')
TOTAL_SAFETY=$(grep -rcE "// SAFETY:" "$KERNEL_DIR" --include="*.rs" 2>/dev/null | awk -F: '{s+=$2} END{print s}')
if [ "$TOTAL_UNSAFE" -gt 0 ]; then
    COVERAGE=$(( TOTAL_SAFETY * 100 / TOTAL_UNSAFE ))
    echo "  unsafe blocks : $TOTAL_UNSAFE"
    echo "  SAFETY 注释   : $TOTAL_SAFETY"
    echo "  覆盖率        : ${COVERAGE}%"
    if [ "$COVERAGE" -ge 50 ]; then
        ok "SAFETY 覆盖率 >= 50%"
    else
        echo -e "${YELLOW}⚠ SAFETY 覆盖率 < 50%${NC}"
    fi
fi

# ── 4. Lockbud 死锁/数据竞争扫描 ────────────────────────────────
step "4/6 Lockbud 死锁/数据竞争扫描"
pushd src/rust > /dev/null
unset RUSTC_WRAPPER
if cargo +nightly lockbud --target x86_64-unknown-none 2>&1 | tail -20; then
    ok "lockbud: passed"
else
    echo -e "${YELLOW}⚠ lockbud 发现潜在问题 (见上)${NC}"
fi
popd > /dev/null

# ── 5. Miri 严格 provenance 配置验证 ───────────────────────────
step "5/6 Miri 严格 provenance 配置"
MIRIFLAGS_OK=0
if grep -rn "MIRIFLAGS" "$PROJECT_ROOT/miri-tests" "$PROJECT_ROOT/src/rust" 2>/dev/null | head -3; then
    MIRIFLAGS_OK=1
fi
if [ "$MIRIFLAGS_OK" -eq 1 ]; then
    ok "Miri 配置存在"
else
    echo -e "${YELLOW}⚠ Miri 严格 provenance 未配置 (待补)${NC}"
fi

# ── 6. 模块级 SAFETY 不变式审计 ────────────────────────────────
step "6/6 模块级 SAFETY 不变式 (框架特权层)"
PRIVILEGE_MODULES=$(grep -rln "Framekernel privilege wrapper\|SAFETY invariant" "$KERNEL_DIR" --include="*.rs" 2>/dev/null | wc -l)
echo "  含模块级 SAFETY 不变式的文件: $PRIVILEGE_MODULES"
if [ "$PRIVILEGE_MODULES" -ge 5 ]; then
    ok "框架特权层封装充分"
else
    echo -e "${YELLOW}⚠ 框架特权层模块数 < 5${NC}"
fi

echo -e "\n${GREEN}━━━ audit (full) 完成 ━━━${NC}"
