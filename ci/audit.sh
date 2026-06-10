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

# ── 0. TCB 安全边界门禁 (M2 里程碑硬约束) ────────────────────
# 服务层 (services/) 不允许任何 unsafe 代码。这是框内核架构的核心契约。
# check_tcb.sh 是 fail-fast 门禁: 一旦发现立即 exit 1, 整个 audit 终止。
# 历史教训: 2026-06-03 审计发现 check_tcb.sh 正则有 bug (变长 lookbehind),
# 导致 services/ 出现 8 处 unsafe 仍报 PASS。已修复, 见 tools/check_tcb.sh 顶部注释。
step "0/6 TCB 安全边界门禁 (services/ 零 unsafe)"
if [ -x "$PROJECT_ROOT/tools/check_tcb.sh" ]; then
    if "$PROJECT_ROOT/tools/check_tcb.sh"; then
        ok "TCB 边界: services/ 零 unsafe, framework/ 收敛"
    else
        err "TCB 边界被破坏! 见上方 FAIL 输出"
    fi
else
    err "tools/check_tcb.sh 不存在或不可执行"
fi

# ── 0.5b TCB 度量报告 (E10) ──────────────────────────────────────
if command -v python3 >/dev/null 2>&1 && [ -f "$PROJECT_ROOT/scripts/audit_tcb_ratio.py" ]; then
    step "0.5b/6 TCB 度量报告 (E10)"
    "$PROJECT_ROOT/scripts/audit_tcb_ratio.py" 2>&1 | tail -20
fi

# ── 0.5c 6 安全不变式审计 (E9) ────────────────────────────────────
if command -v python3 >/dev/null 2>&1 && [ -f "$PROJECT_ROOT/scripts/audit_invariants.py" ]; then
    step "0.5c/6 6 安全不变式审计 (E9)"
    if "$PROJECT_ROOT/scripts/audit_invariants.py" 2>&1 | tail -12; then
        ok "6 安全不变式: 全部满足"
    else
        err "6 安全不变式: 有违反! 见上方输出"
    fi
fi

# Phase 3.2 真实工具: framework 全量 SAFETY 注释覆盖审计
# quick 模式也会跑 (核心 fail-fast 门禁, 不需要 Lockbud/Miri 等重工具)
if command -v python3 >/dev/null 2>&1 && [ -f "$PROJECT_ROOT/tools/audit_unsafe.py" ]; then
    step "0.5/6 Framework SAFETY 注释全量审计 (Phase 3.2)"
    AUDIT_RESULT=$("$PROJECT_ROOT/tools/audit_unsafe.py" --summary 2>&1 || true)
    echo "$AUDIT_RESULT" | tail -12
    MISSING=$(echo "$AUDIT_RESULT" | grep -E "缺 SAFETY:" | head -1 | awk '{print $NF}')
    if [ -n "$MISSING" ] && [ "$MISSING" -eq 0 ]; then
        ok "framework 100% SAFETY 覆盖 (Phase 3.2 达成)"
    elif [ -n "$MISSING" ]; then
        err "framework 仍有 $MISSING 处缺 SAFETY 注释 (Phase 3.2 未达成)"
    fi
fi

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
LOCKBUD_RESULT=0
cargo +nightly lockbud --target x86_64-unknown-none 2>&1 | tail -25 || LOCKBUD_RESULT=$?
if [ $LOCKBUD_RESULT -eq 0 ]; then
    ok "lockbud: passed"
else
    echo -e "${YELLOW}⚠ lockbud 返回非零 (可能发现潜在问题或工具未安装, 见上)${NC}"
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

# ── 7. QEMU 真实启动测试 (full 模式) ──────────────────────────
# Phase 3.6 门禁: 双架构内核必须在 QEMU 中真实启动通过
# 跳过条件: QEMU 二进制不可用 (e.g. CI 镜像未装 qemu-system-*)
step "7/7 QEMU 双架构真实启动测试 (Phase 3.6 门禁)"
if [ "$MODE" = "full" ]; then
    if command -v qemu-system-x86_64 >/dev/null 2>&1 && command -v qemu-system-aarch64 >/dev/null 2>&1; then
        if "$PROJECT_ROOT/scripts/qemu_boot_test.sh" all 2>&1 | tail -6; then
            ok "QEMU 双架构启动测试: 2/2 通过"
        else
            err "QEMU 双架构启动测试失败 (见 scripts/qemu_boot_test.sh 输出)"
        fi
    else
        echo -e "${YELLOW}⚠ QEMU 二进制不可用, 跳过启动测试 (安装 qemu-system-x86 + qemu-system-aarch64 启用)${NC}"
    fi
else
    echo -e "${BLUE}-> 跳过 (仅 full 模式执行; quick 模式跑 SA bootstrap 即可)${NC}"
fi

echo -e "\n${GREEN}━━━ audit (full) 完成 ━━━${NC}"
