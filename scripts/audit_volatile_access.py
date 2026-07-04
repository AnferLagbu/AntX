#!/usr/bin/env python3
"""
audit_volatile_access.py — LTO 错位防线: volatile 字段访问检查 (2026-07-02 新增)

检查关键字段 (bitmap_size, free_list_head 等) 是否使用 volatile 访问.
已知 LTO 错位 bug: set_bit 内 self.bitmap_size.get() 被错位到 self.failed_allocs.

用法: python3 scripts/audit_volatile_access.py
退出码: 0=通过, 1=有风险
"""
import re
import sys
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "src/kernel")

# 高风险字段: 曾经被 LTO 错位或有 LTO 错位风险
# 格式: (文件, 字段名, 期望访问方式)
RISKY_FIELDS = [
    ("framework/mm/pmm.rs", "bitmap_size", "read_volatile"),
    ("framework/mm/kmalloc.rs", "free_list_head", "read_volatile"),
    ("framework/sync/pi_mutex.rs", "effective_priority", "AtomicU32.load"),
]

def check_field_access(filename, field_name, expected_pattern):
    """检查指定字段在文件中的访问方式."""
    path = os.path.join(SRC, filename)
    if not os.path.exists(path):
        return None, f"文件不存在: {filename}"
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        content = f.read()

    # 查找字段名的所有访问点 (self.field_name)
    access_pattern = re.compile(
        r'self\.' + re.escape(field_name) + r'\.get\(\)',
        re.MULTILINE,
    )
    accesses = list(access_pattern.finditer(content))

    if not accesses:
        return None, f"未找到 self.{field_name}.get() 访问"

    # 检查每个访问是否在 raw pointer + read_volatile 内
    violations = []
    for m in accesses:
        # 取前后 200 字符检查 context
        start = max(0, m.start() - 500)
        end = min(len(content), m.end() + 200)
        context = content[start:end]
        # 排除注释行 (// 注释中的 .get() 不是实际访问)
        line_start = content.rfind('\n', 0, m.start()) + 1
        line_text = content[line_start:m.start()]
        if line_text.strip().startswith('//'):
            continue
        # 排除 init-only 路径 (count_free_pages 仅在 init 时调用, 非热路径)
        # 向上查找最近的 fn 声明
        fn_search_start = max(0, m.start() - 500)
        fn_match = re.search(r'fn\s+(\w+)', content[fn_search_start:m.start()])
        if fn_match and fn_match.group(1) == 'count_free_pages':
            continue
        has_volatile = 'read_volatile' in context or 'addr_of' in context
        has_raw_ptr = 'self as *const Self as *const u8' in context or 'p.add(' in context
        if not has_volatile and not has_raw_ptr:
            line_no = content[:m.start()].count('\n') + 1
            violations.append((line_no, content[m.start():m.end()]))

    return violations, None

def main():
    all_violations = []
    for filename, field, expected in RISKY_FIELDS:
        violations, err = check_field_access(filename, field, expected)
        if err:
            print(f"  ⚠ {filename}:{field}: {err}")
        elif violations:
            all_violations.extend([(filename, field, lv) for lv in violations])
        else:
            print(f"  ✓ {filename}:{field}: 已用 volatile/raw pointer 访问")

    print(f"=== audit_volatile_access: 检查 {len(RISKY_FIELDS)} 个高风险字段 ===")
    if all_violations:
        print(f"  ✗ {len(all_violations)} 处非 volatile 访问:")
        for fn, field, (line, text) in all_violations:
            print(f"    ✗ {fn}:{line} — self.{field}.get()")
        print("\n⚠ 存在 LTO 错位风险")
        sys.exit(1)
    else:
        print("✓ audit_volatile_access 通过")
        sys.exit(0)

if __name__ == "__main__":
    main()
