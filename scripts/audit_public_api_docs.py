#!/usr/bin/env python3
"""
audit_public_api_docs.py — F8 公共 API 中文文档检查 (2026-07-03 新增)

AGENTS.md §6 F8: "公共 API 中文文档注释".
clippy missing-docs-in-crate-items 检查文档存在性, 但不检查内容语言.
本脚本检查所有 pub fn/struct/enum/trait 的文档注释是否包含中文字符.

用法: python3 scripts/audit_public_api_docs.py
退出码: 0=通过, 1=有违规
"""
import re
import sys
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "src/kernel/framework")

# 中文字符正则
CJK_RE = re.compile(r'[\u4e00-\u9fff]')

def check_chinese_doc(content, start_pos):
    """检查某位置前方的 doc 注释是否含中文."""
    # 向上找最近的 doc comment (/// 或 //!)
    lines_before = content[:start_pos].split('\n')
    for line in reversed(lines_before):
        stripped = line.strip()
        if stripped.startswith('///') or stripped.startswith('//!'):
            if CJK_RE.search(stripped):
                return True
        elif stripped == '' or stripped.startswith('#[') or stripped.startswith('pub'):
            continue
        else:
            break  # 遇到非 doc 行, 停止
    return False

def main():
    violations = []
    for root, dirs, files in os.walk(SRC):
        for fname in files:
            if not fname.endswith('.rs'):
                continue
            fpath = os.path.join(root, fname)
            rel_path = os.path.relpath(fpath, ROOT)
            # 跳过 smoltcp (vendored)
            if 'smoltcp' in fpath:
                continue
            with open(fpath, 'r', encoding='utf-8', errors='replace') as f:
                content = f.read()
            # 搜索 pub fn / pub struct / pub enum / pub trait
            for m in re.finditer(r'pub\s+(?:fn|struct|enum|trait)\s+(\w+)', content):
                name = m.group(1)
                line_no = content[:m.start()].count('\n') + 1
                has_doc = check_chinese_doc(content, m.start())
                if not has_doc:
                    violations.append((rel_path, line_no, name))

    print(f"=== audit_public_api_docs: 检查 pub fn/struct/enum/trait 中文文档 ===")
    if violations:
        print(f"  ✗ {len(violations)} 处缺少中文文档:")
        for path, line, name in violations[:20]:
            print(f"    ✗ {path}:{line} — {name}")
        if len(violations) > 20:
            print(f"    ... 共 {len(violations)} 处")
        print("\n⚠ 存在缺少中文文档的公共 API")
        sys.exit(1)
    else:
        print("✓ audit_public_api_docs 通过 (所有公共 API 有中文文档)")
        sys.exit(0)

if __name__ == "__main__":
    main()
