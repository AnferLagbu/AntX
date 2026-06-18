#!/usr/bin/env python3
"""
TRACK-XXX Backlog 同步工具 — 校对并修复 docs/plan/kernel-roadmap.md 末尾的 Backlog 段

检测 Backlog 中每条 [TRACK-XXX] `path:line` 与源码实际状态的偏差, 输出:
  - in_source:  roadmap 引用源码真实 TODO 行, 保持
  - mismatch:   源码存在 TODO 但行号漂移, 自动更新行号
  - no_todo:    源码已无对应 TODO (已闭环), 从 roadmap 删除该行
  - file_gone:  源文件已不存在 (整个模块迁移), 删除该行

执行: 默认干跑 (打印改动); --apply 落盘
退出: 0 = 成功, 1 = I/O 错误
"""

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ROADMAP = ROOT / "docs" / "plan" / "kernel-roadmap.md"
SRC_PREFIX = "src/"

BACKLOG_HDR = "## Backlog: 过期 TODO 跟踪"
ITEM_PATTERN = re.compile(r"^- \[(TRACK-[A-F0-9]{6})\] `([A-Za-z0-9_./]+):(\d+)`")


def parse_backlog(content: str) -> tuple[int, int, list[tuple[str, str, int, int]]]:
    """返回 (段起始行号 1-based, 段结束行号 1-based, [(trk, path, roadmap_line, abs_lineno), ...])."""
    lines = content.splitlines(keepends=True)
    start = end = None
    for i, ln in enumerate(lines, 1):
        if ln.rstrip() == BACKLOG_HDR:
            start = i
        elif start and ln.startswith("## ") and ln.rstrip() != BACKLOG_HDR:
            end = i - 1
            break
    if start is None:
        return -1, -1, []
    if end is None:
        end = len(lines)
    items: list[tuple[str, str, int, int]] = []
    for i in range(start, end + 1):
        line = lines[i - 1]
        # 去掉行尾换行符, 但保留前导空格 (roadmap 用 '- ' 缩进, 但前导允许)
        m = ITEM_PATTERN.match(line)
        if m:
            trk, path, ln = m.group(1), m.group(2), int(m.group(3))
            items.append((trk, path, ln, i))
    return start, end, items


def classify(trk: str, path: str, roadmap_line: int) -> str:
    """返回: in_source / mismatch / no_todo / file_gone"""
    root = Path(ROOT)
    full = root / path
    if not full.exists():
        return "file_gone"
    text = full.read_text(encoding="utf-8", errors="replace")
    src_lines = text.splitlines()
    todo_pat = re.compile(rf"TODO\({trk}\)")
    found = [i + 1 for i, ln in enumerate(src_lines) if todo_pat.search(ln)]
    if not found:
        return "no_todo"
    if roadmap_line in found:
        return "in_source"
    return "mismatch"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true", help="落盘修改")
    args = ap.parse_args()

    if not ROADMAP.exists():
        print(f"! roadmap 不存在: {ROADMAP}", file=sys.stderr)
        return 1
    content = ROADMAP.read_text(encoding="utf-8")
    start, end, items = parse_backlog(content)
    if not items:
        if start == -1:
            print("未找到 '## Backlog: 过期 TODO 跟踪' 段")
        else:
            print(f"Backlog 段 (L{start}-{end}) 无 TRACK 项")
        return 0

    stats = {"in_source": 0, "mismatch": 0, "no_todo": 0, "file_gone": 0}
    plan_lines: list[tuple[int, str]] = []  # (原行号, 新内容或 None=删除)
    for trk, path, rline, abs_line in items:
        c = classify(trk, path, rline)
        stats[c] += 1
        if c == "mismatch":
            full = Path(ROOT) / path
            text = full.read_text(encoding="utf-8", errors="replace")
            new_line = None
            for i, ln in enumerate(text.splitlines(), 1):
                if re.search(rf"TODO\({trk}\)", ln):
                    new_line = i
                    break
            plan_lines.append((abs_line, f"- [{trk}] `{path}:{new_line}` TODO\n"))
            print(f"  [fix]    {trk}  {path}:{rline} -> :{new_line}")
        elif c == "no_todo":
            plan_lines.append((abs_line, None))
            print(f"  [delete] {trk}  {path}:{rline}  (源码已无 TODO)")
        elif c == "file_gone":
            plan_lines.append((abs_line, None))
            print(f"  [delete] {trk}  {path}:{rline}  (文件已不存在)")
        else:
            print(f"  [keep]   {trk}  {path}:{rline}")

    print()
    print(f"统计: in_source={stats['in_source']} mismatch={stats['mismatch']} "
          f"no_todo={stats['no_todo']} file_gone={stats['file_gone']}")

    if not args.apply:
        print("干跑模式, 需加 --apply 落盘")
        return 0

    if not plan_lines:
        print("无需修改")
        return 0

    # 应用改动: 倒序处理 (按行号降序, 避免索引失效)
    lines = content.splitlines(keepends=True)
    for abs_line, new_content in sorted(plan_lines, key=lambda x: -x[0]):
        if new_content is None:
            # 删除
            if abs_line - 1 < len(lines):
                del lines[abs_line - 1]
        else:
            lines[abs_line - 1] = new_content
    ROADMAP.write_text("".join(lines), encoding="utf-8")
    print(f"已落盘 {len(plan_lines)} 条修改到 {ROADMAP.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
