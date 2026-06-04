#!/usr/bin/env python3
"""
M6.1 SAFETY 完备性审计脚本 — 8 类 TCB 安全 API (v2 — 改进检测规则)

检查规则 (修正后):
  (1) unsafe 块 (unsafe {): 8 行内必须有 // SAFETY: 注释
  (2) unsafe fn: 上方 50 行内必须有 // SAFETY: 或 # SAFETY 段
      (允许穿透 impl 块、pub struct、mod 边界, 但不跨过 fn 体闭合)
  (3) unsafe impl Send/Sync: 上面 1-5 行内 // SAFETY: 注释

退出码: 0 = 100% 覆盖, 1 = 有缺失
"""

import os
import re
import sys

FILES = ['frame', 'vmspace', 'usermode', 'userctx', 'iomem', 'ioport', 'irqline', 'dma_buf']
BASE = 'src/kernel/framework'

# 最大向上搜索行数 (覆盖深度嵌套的 docstring)
MAX_LOOKBACK = 50


def _scan_backward(lines, ln, max_lookback, require_safety):
    """向后扫描行查找 SAFETY 注释, 直到遇到非注释/非空代码行停止.

    Args:
        lines: 文件所有行 (0-indexed)
        ln: 当前行 (1-indexed)
        max_lookback: 最多向上查找多少行
        require_safety: 是否要求 'SAFETY' 字符串; False 则只检测是否仅为注释

    Returns:
        (found_safety, hit_non_comment_code)
    """
    # j 是 0-indexed 行号, 起始为 ln-2 (即 ln 行的前一行)
    for j in range(ln - 2, max(ln - 1 - max_lookback, -1), -1):
        if j < 0:
            break
        pl = lines[j].strip()
        if not pl:
            continue
        if 'SAFETY' in pl and require_safety:
            return True, False
        # 注释行: // (行内), /// (docstring), /* */ (块), * (块中间)
        if pl.startswith('//') or pl.startswith('*') or pl.startswith('///') or pl.startswith('/*'):
            continue
        # 遇到非注释代码, 停止
        return False, True
    # 范围内都是注释/空行, 但没找到 SAFETY
    return False, False


def has_safety_nearby(lines, ln, lcontent):
    """向前查找 SAFETY 注释, 支持 docstring 穿透."""

    # ===== 规则 1: unsafe { 块 — 8 行内必须 // SAFETY: =====
    if re.search(r'\bunsafe\s*\{', lcontent):
        found, _ = _scan_backward(lines, ln, 8, require_safety=True)
        return found

    # ===== 规则 2: unsafe fn — 上方 MAX_LOOKBACK 行内 // SAFETY: 或 # SAFETY =====
    if re.search(r'\bunsafe\s+fn\b', lcontent):
        # 允许穿透 #[cfg(...)] / #[allow(...)] 等属性 (它们不属于 SAFETY 屏障)
        for j in range(ln - 2, max(ln - 1 - MAX_LOOKBACK, -1), -1):
            if j < 0:
                break
            pl = lines[j].strip()
            if not pl:
                continue
            if 'SAFETY' in pl:
                return True
            # 注释行 (//, ///, /*, *)
            if pl.startswith('//') or pl.startswith('*') or pl.startswith('///') or pl.startswith('/*'):
                continue
            # 属性行 #[cfg]/#[allow] 属于非 SAFETY 屏障, 允许穿透
            if pl.startswith('#['):
                continue
            # 跨过了 Rust 代码, 停止
            return False
        return False

    # ===== 规则 3: unsafe impl Send/Sync — 上面 1-5 行内 // SAFETY: =====
    if re.search(r'\bunsafe\s+impl\b', lcontent):
        # 先看上面 1-5 行, 但允许穿透 #[cfg] 等属性和连续的 unsafe impl
        consecutive_unsafe_impls = 0
        for j in range(ln - 2, max(ln - 1 - 5, -1), -1):
            if j < 0:
                break
            pl = lines[j].strip()
            if not pl:
                continue
            if 'SAFETY' in pl:
                return True
            # 允许穿透 #[cfg(...)] 等属性
            if pl.startswith('#['):
                continue
            # 允许穿透连续的 unsafe impl (Send/Sync 对)
            if re.search(r'\bunsafe\s+impl\b', pl):
                consecutive_unsafe_impls += 1
                if consecutive_unsafe_impls <= 2:
                    continue
                return False
            # 普通注释行: 跳过
            if pl.startswith('//') or pl.startswith('*') or pl.startswith('///') or pl.startswith('/*'):
                continue
            # 遇到非注释代码: 停止
            return False
        return False

    return False


def main():
    total_unsafe = 0
    total_covered = 0
    all_gaps = []

    print("=" * 78)
    print("M6.1 SAFETY 完备性审计 — 8 类 TCB 安全 API (v2)")
    print("=" * 78)
    print(f"  {'文件':12s} | {'unsafe':>7s} | {'SAFETY':>7s} | 状态")
    print("-" * 78)

    for f in FILES:
        path = f"{BASE}/{f}.rs"
        if not os.path.exists(path):
            continue
        with open(path) as fh:
            content = fh.read()
        lines = content.splitlines()

        unsafe_lines = []
        for i, line in enumerate(lines, 1):
            stripped = line.strip()
            # 跳过注释行 (包括 //, ///, /*, *)
            if stripped.startswith('//') or stripped.startswith('*') or stripped.startswith('/*'):
                continue
            if re.search(r'\bunsafe\b', line):
                unsafe_lines.append((i, line.rstrip()))

        covered = 0
        uncovered = []
        for ln, lcontent in unsafe_lines:
            if has_safety_nearby(lines, ln, lcontent):
                covered += 1
            else:
                uncovered.append((ln, lcontent[:80]))

        total_unsafe += len(unsafe_lines)
        total_covered += covered
        status = "✓ 完整" if not uncovered else f"✗ {len(uncovered)} 处缺失"
        print(f"  {f + '.rs':12s} | {len(unsafe_lines):>7d} | {covered:>7d} | {status}")
        for ln, lc in uncovered:
            all_gaps.append((f, ln, lc))
            print(f"    缺失: {f}.rs:{ln}: {lc}")

    print("-" * 78)
    pct = (total_covered / total_unsafe * 100) if total_unsafe > 0 else 100
    print(f"  {'总计':12s} | {total_unsafe:>7d} | {total_covered:>7d} | {pct:.1f}% 覆盖")
    print("=" * 78)

    if all_gaps:
        print(f"\n❌ M6.1 失败: {len(all_gaps)} 处 SAFETY 缺失")
        return 1
    else:
        print("\n✓ M6.1 通过: 100% SAFETY 覆盖")
        return 0


if __name__ == '__main__':
    sys.exit(main())
