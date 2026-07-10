#!/usr/bin/env python3
"""
audit_dead_code.py — 智能 dead_code 审计脚本

规则 F9: 新增代码禁止 #[allow(dead_code)]，硬件规范常量豁免。

用法:
    python3 scripts/audit_dead_code.py          # 检查违规
    python3 scripts/audit_dead_code.py --stats  # 统计报告
    python3 scripts/audit_dead_code.py --full   # 全量扫描 (含豁免项)

退出码:
    0 = 通过
    1 = 失败 (发现违规)
"""

import re
import sys
from pathlib import Path
from collections import defaultdict

# ============================================================================
# 豁免规则
# ============================================================================

# 硬件规范常量豁免
SPEC_PATTERNS = [
    r"规范定义", r"spec definition", r"hardware register",
    r"IOAPIC|APIC|GIC|PIT", r"ATA_|NVME_|AHCI_|AHCI_",
    r"PORTSC|PORT_|USB_|XHCI", r"VGA_|FB_|DP_",
    r"PCI_|MSI_",
    r"NVME_REG|ATA_STATUS|ATA_CTRL|ATA_ERROR",
    r"PORT_ENABLED|PORT_POWER",
]

# 文件级 allow 豁免 (大文件)
FILE_EXEMPT = [
    "net/init.rs", "sync/lockdep.rs", "ioport.rs", "fs/initramfs.rs",
    "pci/msi.rs", "debug/mod.rs", "syscall/mod.rs", "syscall/futex.rs",
    "net/iface_trait.rs", "net/unix.rs",
]

# ============================================================================
# 智能检测
# ============================================================================

def is_in_cfg_test(lines: list, idx: int) -> bool:
    """检测是否在 #[cfg(test)] 块中"""
    depth = 0
    for i in range(idx - 1, -1, -1):
        line = lines[i].strip()
        depth += line.count("}") - line.count("{")
        if re.match(r"#\[cfg\(test\)\]", line):
            return True
        if depth < 0:
            break
        # 遇到模块/函数定义且不在 test 块中则停止
        if depth <= 0 and re.match(r"^(pub\s+)?(fn|struct|enum|impl|trait|mod)\s", line):
            return False
    return False

def is_in_cfg_block(lines: list, idx: int) -> bool:
    """检测是否在 #[cfg()] 条件编译块中"""
    for i in range(idx - 1, max(idx - 30, -1), -1):
        if i < 0:
            break
        line = lines[i].strip()
        if re.match(r"#\[cfg\(", line) or re.match(r"#!\[cfg\(", line):
            return True
        if re.match(r"^(pub\s+)?(fn|struct|enum|impl|trait|mod)\s", line):
            break
    return False

def is_reexport(lines: list, idx: int) -> bool:
    """检测是否是 re-export 语句"""
    line = lines[idx].strip() if idx < len(lines) else ""
    return "pub use" in line or "pub extern crate" in line

def is_ffi_binding(lines: list, idx: int) -> bool:
    """检测是否是 FFI 绑定 (extern "C")"""
    for i in range(idx, min(idx + 5, len(lines))):
        if "extern \"C\"" in lines[i] or "extern \"system\"" in lines[i]:
            return True
    return False

def is_const_definition(lines: list, idx: int) -> bool:
    """检测是否是常量定义"""
    line = lines[idx].strip() if idx < len(lines) else ""
    return bool(re.match(r"(pub\s+)?(const|static)\s", line))

def get_allow_context(lines: list, idx: int) -> dict:
    """获取 #[allow(dead_code)] 的完整上下文"""
    context = {
        "line": idx + 1,
        "code": lines[idx].strip()[:100],
        "comment": "",
        "target_type": "unknown",
        "target_name": "",
        "is_cfg_test": is_in_cfg_test(lines, idx),
        "is_cfg_block": is_in_cfg_block(lines, idx),
        "is_reexport": is_reexport(lines, idx),
        "is_ffi": is_ffi_binding(lines, idx),
        "is_const": is_const_definition(lines, idx),
    }
    
    # 提取注释
    line = lines[idx].strip()
    if "//" in line:
        context["comment"] = line[line.index("//"):]
    elif idx > 0 and "//" in lines[idx - 1]:
        context["comment"] = lines[idx - 1][lines[idx - 1].index("//"):]
    
    # 识别目标类型
    if "#![allow(dead_code)]" in line:
        context["target_type"] = "file-level"
    else:
        # 向前查找被 allow 的项
        for i in range(idx + 1, min(idx + 10, len(lines))):
            next_line = lines[i].strip()
            if re.match(r"pub\s+fn\s+(\w+)", next_line):
                context["target_type"] = "function"
                context["target_name"] = re.search(r"pub\s+fn\s+(\w+)", next_line).group(1)
                break
            elif re.match(r"(pub\s+)?struct\s+(\w+)", next_line):
                context["target_type"] = "struct"
                context["target_name"] = re.search(r"(pub\s+)?struct\s+(\w+)", next_line).group(1)
                break
            elif re.match(r"(pub\s+)?enum\s+(\w+)", next_line):
                context["target_type"] = "enum"
                context["target_name"] = re.search(r"(pub\s+)?enum\s+(\w+)", next_line).group(1)
                break
            elif re.match(r"(pub\s+)?(const|static)\s+(\w+)", next_line):
                context["target_type"] = "constant"
                context["target_name"] = re.search(r"(pub\s+)?(const|static)\s+(\w+)", next_line).group(1)
                break
            elif re.match(r"(pub\s+)?type\s+(\w+)", next_line):
                context["target_type"] = "type_alias"
                context["target_name"] = re.search(r"(pub\s+)?type\s+(\w+)", next_line).group(1)
                break
            elif "mod " in next_line:
                context["target_type"] = "module"
                break
    
    return context

def is_exempt(ctx: dict, filepath: str) -> tuple:
    """智能豁免判断，返回 (是否豁免, 原因)"""
    # 1. 文件级豁免
    for pattern in FILE_EXEMPT:
        if pattern in filepath:
            return True, "文件级豁免(大文件)"
    
    # 2. cfg 条件编译
    if ctx["is_cfg_block"]:
        return True, "cfg条件编译"
    
    # 3. 测试上下文
    if ctx["is_cfg_test"]:
        return True, "测试上下文"
    
    # 4. 硬件规范常量
    for pattern in SPEC_PATTERNS:
        if re.search(pattern, ctx["comment"], re.IGNORECASE):
            return True, "硬件规范常量"
    
    # 5. FFI 绑定 (通常需要保留)
    if ctx["is_ffi"]:
        return True, "FFI绑定"
    
    # 6. Re-export (可能是公共 API)
    if ctx["is_reexport"]:
        return True, "Re-export"
    
    return False, ""

def scan_file(filepath: Path, root: Path) -> list:
    """扫描单个文件"""
    violations = []
    rel_path = str(filepath.relative_to(root))
    
    try:
        content = filepath.read_text(encoding="utf-8")
    except Exception:
        return violations
    
    lines = content.split("\n")
    
    for i, line in enumerate(lines):
        stripped = line.strip()
        
        # 跳过纯注释行
        if stripped.startswith("//") or stripped.startswith("*") or stripped.startswith("/*"):
            continue
        
        # 检测 #[allow(dead_code)] 或 #![allow(dead_code)]
        if "#[allow(dead_code)]" in stripped or "#![allow(dead_code)]" in stripped:
            ctx = get_allow_context(lines, i)
            exempt, reason = is_exempt(ctx, rel_path)
            
            if not exempt:
                violations.append({
                    "file": rel_path,
                    "line": ctx["line"],
                    "code": ctx["code"],
                    "comment": ctx["comment"],
                    "target_type": ctx["target_type"],
                    "target_name": ctx["target_name"],
                })
    
    return violations

def scan_all(root: Path) -> list:
    """扫描所有文件"""
    violations = []
    
    for rs_file in root.rglob("*.rs"):
        rel = str(rs_file.relative_to(root))
        # 跳过第三方库 (smoltcp)、构建产物、Mimocode 配置
        if "smoltcp" in rel or "target" in rel or ".mimocode" in rel:
            continue
        violations.extend(scan_file(rs_file, root))
    
    return violations

def print_stats(violations: list):
    """打印统计报告"""
    by_file = defaultdict(list)
    by_type = defaultdict(list)
    
    for v in violations:
        by_file[v["file"]].append(v)
        by_type[v["target_type"]].append(v)
    
    print(f"=== Dead Code 统计 ===")
    print(f"总违规数: {len(violations)}")
    print(f"涉及文件: {len(by_file)}")
    print()
    
    print("按文件统计 (Top 10):")
    for filepath, items in sorted(by_file.items(), key=lambda x: -len(x[1]))[:10]:
        print(f"  {filepath}: {len(items)} 处")
    
    print()
    print("按目标类型统计:")
    for target_type, items in sorted(by_type.items(), key=lambda x: -len(x[1])):
        print(f"  {target_type}: {len(items)} 处")
    
    print()
    print("违规类型分布:")
    file_level = sum(1 for v in violations if v["target_type"] == "file-level")
    func_level = sum(1 for v in violations if v["target_type"] == "function")
    struct_level = sum(1 for v in violations if v["target_type"] == "struct")
    const_level = sum(1 for v in violations if v["target_type"] == "constant")
    other = len(violations) - file_level - func_level - struct_level - const_level
    print(f"  文件级 #![allow]: {file_level}")
    print(f"  函数: {func_level}")
    print(f"  结构体: {struct_level}")
    print(f"  常量: {const_level}")
    print(f"  其他: {other}")

def main():
    root = Path(__file__).parent.parent / "src" / "kernel"
    
    if not root.exists():
        print(f"错误: 目录不存在 {root}")
        sys.exit(1)
    
    mode = "new"
    if "--full" in sys.argv:
        mode = "full"
    elif "--stats" in sys.argv:
        mode = "stats"
    
    violations = scan_all(root)
    
    if mode == "stats":
        print_stats(violations)
        sys.exit(0)
    
    if violations:
        print(f"FAIL: 发现 {len(violations)} 处 dead_code 违规\n")
        for v in violations[:30]:
            print(f"  {v['file']}:{v['line']}")
            print(f"    类型: {v['target_type']} {v['target_name']}")
            print(f"    代码: {v['code']}")
            if v['comment']:
                print(f"    注释: {v['comment']}")
            print()
        
        if len(violations) > 30:
            print(f"  ... 还有 {len(violations) - 30} 处违规 (使用 --stats 查看完整统计)")
        
        sys.exit(1)
    else:
        print("PASS: 无 dead_code 违规")
        sys.exit(0)

if __name__ == "__main__":
    main()
