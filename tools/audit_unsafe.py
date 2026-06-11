#!/usr/bin/env python3
"""
AntX/QueenX Framework Unsafe 块 SAFETY 注释自动审计

扫描 framework/ 下所有 *.rs 文件, 列出每个 unsafe 引用位置 + 上方 5 行内
是否含 SAFETY 注释, 输出一份诚实基线报告。

用法:
    python3 tools/audit_unsafe.py                  # 人类可读表格
    python3 tools/audit_unsafe.py --machine        # TSV
    python3 tools/audit_unsafe.py --missing-only   # 只列缺 SAFETY 的
    python3 tools/audit_unsafe.py --summary        # 统计数字

退出码:
    0 = 审计完成
    2 = 内部错误
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from typing import List, NamedTuple


PROJECT_ROOT = Path(__file__).resolve().parent.parent
FW_DIR = PROJECT_ROOT / "src" / "kernel" / "framework"


class UnsafeHit(NamedTuple):
    file: str
    line: int
    kind: str
    has_safety: bool
    code: str
    safety_context: str  # the SAFETY line content if any


# 匹配 unsafe 出现的行 (排除注释行/字符串)
UNSAFE_RE = re.compile(r"\bunsafe\b")

# 匹配 unsafe 块的几种形式
KIND_PATTERNS = [
    ("block",  re.compile(r"unsafe\s*\{",                  re.IGNORECASE)),
    ("fn",     re.compile(r"unsafe\s+fn\s+",              re.IGNORECASE)),
    ("impl",   re.compile(r"unsafe\s+impl\b",             re.IGNORECASE)),
    ("trait",  re.compile(r"unsafe\s+trait\b",            re.IGNORECASE)),
    ("extern", re.compile(r"unsafe\s+extern\b",           re.IGNORECASE)),
]


def classify_kind(code: str) -> str:
    for kind, pat in KIND_PATTERNS:
        if pat.search(code):
            return kind
    return "ref"


def is_comment_line(stripped: str) -> bool:
    if stripped.startswith("//"):
        return True
    if stripped.startswith("/*") or stripped.startswith("/**"):
        return True
    if stripped.startswith("*") or stripped.startswith("*/"):
        return True
    return False


def check_safety_above(lines: List[str], line_idx: int) -> tuple[bool, str]:
    """
    检查第 line_idx 行 (0-indexed) 的上方 8 行内 (line_idx-8 .. line_idx-1)
    是否有 SAFETY 注解。两种形式:
      1. `// SAFETY:` 或 `//SAFETY:`  紧邻注释 (Rust 惯用法, 紧挨 unsafe 块)
         - 注释可能多行: `// SAFETY: \n//   1. ... \n//   2. ...`
      2. `/// # Safety` 文档注释章节 (Rust 官方推荐, 紧挨 unsafe fn)
    """
    for i in range(max(0, line_idx - 8), line_idx):
        line = lines[i]
        if "SAFETY" in line:
            return True, line.rstrip()
        # /// # Safety 章节 (大小写不敏感)
        stripped = line.lstrip()
        if stripped.startswith("///") and "safety" in stripped.lower():
            return True, line.rstrip()
    return False, ""


def scan_file(path: Path) -> List[UnsafeHit]:
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError as e:
        print(f"WARN: {path}: {e}", file=sys.stderr)
        return []

    lines = text.splitlines()
    hits: List[UnsafeHit] = []
    rel = str(path.relative_to(PROJECT_ROOT))

    for idx, raw_line in enumerate(lines):
        if not UNSAFE_RE.search(raw_line):
            continue
        # 排除纯注释行
        stripped = raw_line.lstrip()
        if is_comment_line(stripped):
            continue
        # 排除字符串内 (粗略, 统计引号数量)
        # 如果行内含未闭合的字符串, 暂时放行 (Rust 单行字符串罕见)
        kind = classify_kind(raw_line)
        has_safety, ctx = check_safety_above(lines, idx)
        hits.append(UnsafeHit(
            file=rel,
            line=idx + 1,  # 1-indexed for humans
            kind=kind,
            has_safety=has_safety,
            code=raw_line.rstrip(),
            safety_context=ctx,
        ))

    return hits


def print_summary(hits: List[UnsafeHit]) -> None:
    total = len(hits)
    missing = sum(1 for h in hits if not h.has_safety)
    ok = total - missing
    by_kind: dict[str, int] = {}
    for h in hits:
        by_kind[h.kind] = by_kind.get(h.kind, 0) + 1

    print("=== Framework Unsafe 块 SAFETY 注释基线 ===")
    print(f"扫描目录:     {FW_DIR.relative_to(PROJECT_ROOT)}")
    print(f"扫描时间:     {__import__('datetime').datetime.now().isoformat(timespec='seconds')}")
    print()
    print(f"  unsafe 引用总数:  {total}")
    for k in ("block", "fn", "impl", "trait", "extern", "ref"):
        if k in by_kind:
            label = {
                "block":  "├─ 块 (unsafe { ... }):",
                "fn":     "├─ 函数 (unsafe fn):",
                "impl":   "├─ unsafe impl:",
                "trait":  "├─ unsafe trait:",
                "extern": "├─ unsafe extern:",
                "ref":    "└─ 引用 (其他):",
            }.get(k, f"├─ {k}:")
            print(f"  {label:<26}  {by_kind[k]}")
    print()
    pct = (ok * 100 // total) if total else 100
    print(f"  SAFETY 注释覆盖:  {ok} / {total}  ({pct}%)")
    print(f"  缺 SAFETY:        {missing}")
    print()
    print("  验收标准: 缺 SAFETY = 0")
    if missing == 0:
        print("  ✅ 全部已覆盖")
    else:
        print(f"  ❌ 仍有 {missing} 处需补 SAFETY 注释")
        print()
        print("  按文件 Top 5 (缺 SAFETY 最多):")
        miss_by_file: dict[str, int] = {}
        for h in hits:
            if not h.has_safety:
                miss_by_file[h.file] = miss_by_file.get(h.file, 0) + 1
        for f, c in sorted(miss_by_file.items(), key=lambda x: -x[1])[:5]:
            print(f"    {c:3}  {f}")


def print_human(hits: List[UnsafeHit], missing_only: bool) -> None:
    print("=== Framework Unsafe 块 SAFETY 注释基线 ===")
    print(f"扫描目录: {FW_DIR.relative_to(PROJECT_ROOT)}")
    print(f"扫描时间: {__import__('datetime').datetime.now().isoformat(timespec='seconds')}")
    print()

    shown = [h for h in hits if (not missing_only or not h.has_safety)]

    print(f"{'FILE:LINE':<60} {'LINE':>5} {'KIND':<7} {'SAFETY':<8}")
    print("-" * 90)
    for h in shown[:80]:
        key = f"{h.file}:{h.line}"
        if len(key) > 58:
            key = "..." + key[-(58 - 3):]
        safety = "✓" if h.has_safety else "✗"
        print(f"{key:<60} {h.line:>5} {h.kind:<7} {safety:<8}")
    if len(shown) > 80:
        print(f"\n(仅显示前 80 行, 完整列表用 --machine 或 --missing-only)")

    print()
    print_summary(hits)


def print_machine(hits: List[UnsafeHit], missing_only: bool) -> None:
    print("file\tline\tkind\thas_safety\tcode")
    for h in hits:
        if missing_only and h.has_safety:
            continue
        # 用 \t 拆分安全
        code = h.code.replace("\t", "    ")
        print(f"{h.file}\t{h.line}\t{h.kind}\t{str(h.has_safety).lower()}\t{code}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0] if __doc__ else "")
    ap.add_argument("--machine", action="store_true", help="TSV output")
    ap.add_argument("--missing-only", action="store_true", help="只输出缺 SAFETY 的")
    ap.add_argument("--summary", action="store_true", help="仅统计")
    args = ap.parse_args()

    if not FW_DIR.is_dir():
        print(f"ERROR: {FW_DIR} 不存在", file=sys.stderr)
        return 2

    files = sorted(FW_DIR.rglob("*.rs"))
    all_hits: List[UnsafeHit] = []
    for f in files:
        all_hits.extend(scan_file(f))

    if args.machine:
        print_machine(all_hits, args.missing_only)
    elif args.summary:
        print_summary(all_hits)
    else:
        print_human(all_hits, args.missing_only)

    return 0


if __name__ == "__main__":
    sys.exit(main())
