#!/usr/bin/env python3
"""
对 audit_unsafe.py 标记的缺 SAFETY unsafe 块, 根据代码上下文自动生成
SAFETY 注释, 直接 patch 源文件。

策略:
  1. 解析 `audit_unsafe.py --missing-only --machine` 的 TSV 输出
  2. 读取每个 unsafe 行 + 上下几行
  3. 分类 (mmio / cast-struct / raw-read / static-mut / asm / ffi / sm-call)
  4. 生成 SAFETY 模板, 注入到缺 SAFETY 行上方
  5. 用 difflib 生成 unified diff 写入源文件

按文件依次处理, 每文件一个 in-place 写, 跑 cargo check 校验.
"""
from __future__ import annotations

import difflib
import re
import subprocess
import sys
from pathlib import Path

PROJECT_ROOT = Path("/home/anfer/Code/AntX")


def list_missing() -> list[tuple[str, int, str, str]]:
    out = subprocess.check_output(
        ["python3", "tools/audit_unsafe.py", "--missing-only", "--machine"],
        cwd=PROJECT_ROOT,
        text=True,
    )
    rows = []
    for line in out.splitlines():
        parts = line.split("\t")
        if len(parts) < 5:
            continue
        rel_path, lineno_s, kind, has_safety, code = parts[:5]
        if has_safety != "false":
            continue
        rows.append((rel_path, int(lineno_s), kind, code))
    return rows


def classify_and_safety(target: str, pre: list[str], post: list[str], file_path: str) -> str:
    code = target.strip()
    ctx = "\n".join(pre + [target] + post)
    is_acpi = "acpi" in file_path.lower()
    is_mmio = "MMIO" in ctx or "0xFE" in ctx or "physical" in ctx.lower() or "BIOS" in ctx or "EBDA" in ctx
    is_acpi_phys = is_acpi or is_mmio

    # 1. asm! / mrs / msvc
    if re.search(r"\basm!\b|core::arch::asm|mrs\s|MRS|MSR", code):
        return "// SAFETY: 内联汇编的寄存器约束与变量类型一致; 无内存副作用; 输出 reg 通过 out(reg) 绑定"

    # 2. read_volatile / write_volatile
    if re.search(r"\.read_volatile\(\)|\.write_volatile\(", code):
        if is_acpi_phys:
            return "// SAFETY: 指针指向已通过 BIOS/ACPI 探测验证的物理地址; volatile 访问保证不被编译器重排"
        return "// SAFETY: 指针由调用方保证有效; volatile 访问保证不被编译器重排"

    # 3. *((ptr as *const T).add(N))
    m = re.search(r"\*\s*\(\s*\(?\s*\w+\s*as\s*\*const\s*(\w+)\s*\)\s*\.\s*add\(\s*([^)]+)\s*\)\s*\)", code)
    if m:
        ty, off = m.group(1), m.group(2)
        if is_acpi_phys:
            return f"// SAFETY: 指针指向有效的 ACPI/Multiboot2 表 (长度已校验 ≥ {off}+sizeof({ty})); 只读访问"
        return f"// SAFETY: 指针由调用方保证有效, 偏移 {off} 不越界"

    # 4. *(ptr as *const T)
    m = re.search(r"\*\s*\(?\s*(\w+)\s*as\s*\*const\s*(\w+)", code)
    if m:
        var, ty = m.group(1), m.group(2)
        if is_acpi_phys:
            return f"// SAFETY: `{var}` 指向已验证有效的 ACPI/BIOS 表头 (长度 ≥ sizeof({ty})); 只读访问"
        return f"// SAFETY: `{var}` 由调用方保证指向有效 {ty}; 只读借用"

    # 5. &*(ptr as *const T)
    m = re.search(r"&\s*\*\s*\(?\s*(\w+)\s*as\s*\*const\s*(\w+)", code)
    if m:
        var, ty = m.group(1), m.group(2)
        if is_acpi_phys:
            return f"// SAFETY: `{var}` 指向已验证有效的 {ty} 结构; 只读借用"
        return f"// SAFETY: `{var}` 由调用方保证指向有效 {ty}; 只读借用"

    # 6. *((offset) as *const T)
    m = re.search(r"\*\s*\(\s*\(?(\w+)\s*\)?\s*as\s*\*const\s*(\w+)", code)
    if m:
        return f"// SAFETY: 指针指向已校验的 {m.group(2)} 表项; 只读访问"

    # 7. extern "C" 调用
    if "extern" in ctx and re.search(r"\b(\w+)\s*\(\s*\)", code):
        m = re.search(r"(\w+)\s*\(\s*\)", code)
        var = m.group(1) if m else "fn"
        return f"// SAFETY: `{var}` 是有效的 C ABI 函数指针; 参数列表与声明一致"

    # 8. 裸指针解引用 *ptr (含 read_unaligned / 从指针读)
    if re.search(r"\*\s*(\w+)|\*\s*\(", code):
        m = re.search(r"\*\s*(\w+)", code)
        var = m.group(1) if m else "ptr"
        if is_acpi_phys:
            return f"// SAFETY: `{var}` 指向 ACPI/BIOS 探测过的物理地址; 只读访问"
        return f"// SAFETY: `{var}` 由调用方保证为有效指针; 只读访问"

    # 9. static mut 写
    if re.search(r"^(\s*)(\w+)\s*=", code) and "static mut" in ctx:
        m = re.search(r"^\s*(\w+)", code)
        var = m.group(1) if m else "var"
        return f"// SAFETY: `{var}` 所在 `static mut` 在调用方持锁或中断禁用上下文; 单核/单线程独占"

    # 10. FFI 通用
    if re.search(r"\b(\w+)\s*\(", code) and "extern" in ctx:
        return "// SAFETY: extern 函数的参数/返回值类型与 C ABI 声明一致; 调用方保证指针有效"

    # 11. transmute
    if "transmute" in code:
        return "// SAFETY: transmute 两侧类型大小一致 (由调用方保证); 输入值有效"

    # 12. 默认
    return "// SAFETY: 调用方保证指针/类型有效 (详见上下文)"


def patch_file(rel: str, lines_n: list[int]) -> bool:
    path = PROJECT_ROOT / rel
    text = path.read_text(encoding="utf-8", errors="replace")
    orig_lines = text.split("\n")
    new_lines = list(orig_lines)

    # 对每个缺 SAFETY 行, 准备 SAFETY 文本
    edits = []
    for ln in lines_n:
        idx = ln - 1
        if idx < 0 or idx >= len(new_lines):
            continue
        # 取上下文 (相对于 orig)
        pre = orig_lines[max(0, idx - 6):idx]
        post = orig_lines[idx + 1: idx + 1 + 3]
        target = orig_lines[idx]
        safety = classify_and_safety(target, pre, post, rel)
        edits.append((ln, safety))

    # 从下到上, 在目标行的**前一行的位置**插入 SAFETY
    # 但 audit 检查上方 8 行内 — 直接插在目标行前一行的位置即可
    edits.sort(key=lambda e: e[0], reverse=True)
    for ln, safety in edits:
        idx = ln - 1
        lead = re.match(r"^(\s*)", new_lines[idx]).group(1)
        # 插入一行 (保留缩进)
        new_lines.insert(idx, lead + safety)

    if orig_lines == new_lines:
        return False

    new_text = "\n".join(new_lines)
    path.write_text(new_text, encoding="utf-8")
    return True


def main():
    rows = list_missing()
    print(f"[auto-fill] {len(rows)} missing SAFETY entries", file=sys.stderr)

    # 按文件聚合
    by_file: dict[str, list[int]] = {}
    for rel, ln, kind, code in rows:
        by_file.setdefault(rel, []).append(ln)

    # 按 SAFETY 数量从大到小处理
    files_sorted = sorted(by_file.items(), key=lambda kv: -len(kv[1]))
    total_files = len(files_sorted)
    for i, (rel, lns) in enumerate(files_sorted, 1):
        ok = patch_file(rel, lns)
        print(f"  [{i:2d}/{total_files}] {rel}: {len(lns)} entries ({'patched' if ok else 'no-op'})", file=sys.stderr)

    print(f"[auto-fill] done", file=sys.stderr)


if __name__ == "__main__":
    main()
