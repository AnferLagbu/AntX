#!/bin/bash
# tools/check_tcb.sh — QueenX TCB 统计
#
# Phase 0 产物: 自动化 unsafe 分布检查
# 用法: ./tools/check_tcb.sh

set -euo pipefail
cd "$(dirname "$0")/.."

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "=== QueenX TCB Inventory ==="
echo ""

# 统计 framework 中 unsafe
FW_UNSAFE=$(grep -rn "unsafe " src/kernel/framework/ 2>/dev/null | wc -l || echo 0)
FW_LINES=$(find src/kernel/framework -name "*.rs" -exec cat {} \; 2>/dev/null | wc -l || echo 0)

# 统计 services 中 unsafe (期望为 0)
# 匹配实际的 unsafe 代码块 / 函数 / impl, 排除注释行。
# 修复历史: 之前用 `(?<!//.*)\bunsafe\s*[\{fn]` 含变长 lookbehind,
# PCRE2 直接拒匹配 (length of lookbehind assertion is not limited),
# 导致 services 不论有什么 unsafe 都报告 0。
#
# 过滤策略: 仅丢弃行首是 `//`(行注释) / `*`/`/*` (块注释延续) 的行。
# 错误地把 `unsafe impl<T: Send>` 误判为注释是上次 bug, 这里改用更稳的判定。
SV_UNSAFE=0
SV_LINES=0
if [ -d "src/kernel/services" ]; then
    SV_UNSAFE=$(grep -rPn '\bunsafe\b' src/kernel/services/ --include='*.rs' 2>/dev/null \
        | awk -F: '{
            # 跳过行注释和块注释
            code=$0; sub(/^[^:]+:[^:]+:/, "", code);
            if (code ~ /^[[:space:]]*\/\//) next;
            if (code ~ /^[[:space:]]*\*/) next;
            # 跳过字符串字面量内的 unsafe (极少, 简单起见不处理)
            print
        }' \
        | wc -l || echo 0)
    SV_LINES=$(find src/kernel/services -name "*.rs" -exec cat {} \; 2>/dev/null | wc -l || echo 0)
fi

# 内核总行数 (排除 smoltcp vendored)
TOTAL_LINES=$(find src/kernel -name "*.rs" -not -path "*/smoltcp/*" -exec cat {} \; 2>/dev/null | wc -l)

echo "framework unsafe 行数:  $FW_UNSAFE"
echo "framework 总行数:      $FW_LINES"
echo "services unsafe 行数:   $SV_UNSAFE  (MUST BE 0)"
echo "services 总行数:        $SV_LINES"
echo "---"
if [ "$TOTAL_LINES" -gt 0 ]; then
    PCT=$(awk "BEGIN {printf \"%.1f\", ($FW_LINES/$TOTAL_LINES)*100}")
    echo "内核总行数 (-smoltcp):  $TOTAL_LINES"
    echo "TCB 占比 (fw/total):    ${PCT}%"
else
    echo "内核总行数 (-smoltcp):  (empty)"
fi
echo ""

# 检查 services 中无 unsafe
if [ -d "src/kernel/services" ]; then
    if [ "$SV_UNSAFE" -gt 0 ]; then
        echo -e "${RED}FAIL${NC}: services/ 中发现 unsafe 块:"
        grep -rPn '\bunsafe\b' src/kernel/services/ --include='*.rs' 2>/dev/null \
            | awk -F: '{ code=$0; sub(/^[^:]+:[^:]+:/, "", code); if (code !~ /^[[:space:]]*(\/\/|\*)/) print }'
        exit 1
    else
        echo -e "${GREEN}PASS${NC}: services/ 无 unsafe"
    fi
fi

# 检查 TCB 占比
if [ "$TOTAL_LINES" -gt 0 ] && [ "$FW_LINES" -gt "$((TOTAL_LINES * 20 / 100))" ]; then
    echo -e "${YELLOW}WARN${NC}: TCB 超过 20%: ${PCT}%"
else
    echo -e "${GREEN}PASS${NC}: TCB < 20%: ${PCT}% (或 framework 尚未迁移)"
fi

echo ""
echo "=== Unsafe Top 10 (全内核) ==="
{
    grep -rln "unsafe " src/kernel/ 2>/dev/null
} | {
    xargs -I {} sh -c 'count=$(grep -c "unsafe " "{}" 2>/dev/null || echo 0); echo "$count {}"' 2>/dev/null || true
} | sort -rn | head -10 || true
