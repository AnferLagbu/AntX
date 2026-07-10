#!/usr/bin/env python3
"""
audit_dead_code.py — 检查 dead_code 使用情况

规则 F9: 新增代码禁止 #[allow(dead_code)]，硬件规范常量豁免。

用法:
    python3 scripts/audit_dead_code.py          # 仅检查新增代码 (默认)
    python3 scripts/audit_dead_code.py --full   # 全量扫描 (含预留代码)
    python3 scripts/audit_dead_code.py --stats  # 统计报告

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

# 硬件规范常量豁免 (正则匹配注释)
SPEC_PATTERNS = [
    r"规范定义",           # 中文: 规范定义
    r"spec definition",    # 英文: spec definition
    r"hardware register",  # 硬件寄存器
    r"IOAPIC|APIC|GIC",   # 中断控制器常量
    r"ATA_|NVME_|AHCI_",  # 存储控制器常量
    r"PORTSC|PORT_|USB_", # USB 常量
    r"VGA_|FB_",          # 显示常量
    r"PCI_",              # PCI 常量
    r"NVME_REG|ATA_STATUS|ATA_CTRL",  # 具体寄存器
]

# 文件级 allow 豁免 (这些文件太大，逐项标注会淹没代码)
FILE_EXEMPT = [
    "net/init.rs",        # 网络栈初始化 (2700+ 行)
    "sync/lockdep.rs",    # 锁依赖检测器
    "ioport.rs",          # I/O 端口抽象层
    "fs/initramfs.rs",    # initramfs 解包
    "pci/msi.rs",         # MSI 中断
    "debug/mod.rs",       # 调试模块
    "syscall/mod.rs",     # 系统调用模块
    "syscall/futex.rs",   # futex 实现
    "net/iface_trait.rs", # 网络接口 trait
    "net/unix.rs",        # Unix socket
]

# cfg 条件编译豁免 (这些代码在特定配置下使用)
CFG_EXEMPT = [
    r"#\[cfg\(test\)\]",           # 测试代码
    r"#\[cfg\(feature",            # feature gate
    r"#\[cfg\(target_arch",        # 架构特定
    r"cfg!\(test\)",               # 运行时测试检查
]

# ============================================================================
# 扫描逻辑
# ============================================================================

def check_cfg_exempt(lines: list, line_idx: int) -> bool:
    """检查是否在 cfg 条件编译块中"""
    # 向前查找最近的 #[cfg] 或 #![cfg]
    for i in range(line_idx - 1, max(line_idx - 20, -1), -1):
        if i < 0:
            break
        line = lines[i].strip()
        if re.search(r"#\[cfg\(", line) or re.search(r"#!\[cfg\(", line):
            return True
        # 遇到函数/结构体定义停止
        if re.match(r"^(pub\s+)?(fn|struct|enum|impl|trait|mod)\s", line):
            break
    return False

def check_test_context(lines: list, line_idx: int) -> bool:
    """检查是否在测试模块中"""
    for i in range(line_idx - 1, max(line_idx - 50, -1), -1):
        if i < 0:
            break
        line = lines[i].strip()
        if re.match(r"#\[cfg\(test\)\]", line) or "mod tests {" in line:
            return True
        # 遇到函数定义停止 (非嵌套测试)
        if re.match(r"^(pub\s+)?fn\s", line) and "test" not in line:
            break
    return False

def is_exempt(comment: str, filepath: str, lines: list, line_idx: int) -> tuple:
    """检查是否豁免，返回 (是否豁免, 豁免原因)"""
    # 1. 文件级豁免
    for pattern in FILE_EXEMPT:
        if pattern in filepath:
            return True, "文件级豁免"
    
    # 2. cfg 条件编译豁免
    if check_cfg_exempt(lines, line_idx):
        return True, "cfg 条件编译"
    
    # 3. 测试上下文豁免
    if check_test_context(lines, line_idx):
        return True, "测试上下文"
    
    # 4. 硬件规范常量豁免
    for pattern in SPEC_PATTERNS:
        if re.search(pattern, comment, re.IGNORECASE):
            return True, "硬件规范常量"
    
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
            # 提取注释
            comment = ""
            if "//" in stripped:
                comment = stripped[stripped.index("//"):]
            elif i > 0 and "//" in lines[i-1]:
                comment = lines[i-1][lines[i-1].index("//"):]
            
            # 检查豁免
            exempt, reason = is_exempt(comment, rel_path, lines, i)
            
            if not exempt:
                # 获取上下文 (前后各 2 行)
                context_start = max(0, i - 2)
                context_end = min(len(lines), i + 3)
                context = "\n".join(f"  {j+1}: {lines[j]}" for j in range(context_start, context_end))
                
                violations.append({
                    "file": rel_path,
                    "line": i + 1,
                    "code": stripped[:80],
                    "comment": comment,
                    "context": context,
                })
    
    return violations

def scan_all(root: Path) -> list:
    """扫描所有文件"""
    violations = []
    
    for rs_file in root.rglob("*.rs"):
        # 跳过 vendored 代码和 target 目录
        rel = str(rs_file.relative_to(root))
        if "smoltcp" in rel or "target" in rel or ".mimocode" in rel:
            continue
        
        violations.extend(scan_file(rs_file, root))
    
    return violations

def print_stats(violations: list):
    """打印统计报告"""
    by_file = defaultdict(list)
    for v in violations:
        by_file[v["file"]].append(v)
    
    print(f"=== Dead Code 统计 ===")
    print(f"总违规数: {len(violations)}")
    print(f"涉及文件: {len(by_file)}")
    print()
    
    print("按文件统计:")
    for filepath, items in sorted(by_file.items(), key=lambda x: -len(x[1])):
        print(f"  {filepath}: {len(items)} 处")
    
    print()
    print("按类型统计:")
    file_level = sum(1 for v in violations if "#![allow" in v["code"])
    item_level = len(violations) - file_level
    print(f"  文件级 #![allow]: {file_level}")
    print(f"  单项 #[allow]: {item_level}")

def main():
    root = Path(__file__).parent.parent / "src" / "kernel"
    
    if not root.exists():
        print(f"错误: 目录不存在 {root}")
        sys.exit(1)
    
    # 解析参数
    mode = "new"  # 默认只检查新增代码
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
        for v in violations[:20]:  # 只显示前 20 个
            print(f"  {v['file']}:{v['line']}")
            print(f"    代码: {v['code']}")
            if v['comment']:
                print(f"    注释: {v['comment']}")
            print()
        
        if len(violations) > 20:
            print(f"  ... 还有 {len(violations) - 20} 处违规 (使用 --stats 查看完整统计)")
        
        sys.exit(1)
    else:
        print("PASS: 无 dead_code 违规")
        sys.exit(0)

if __name__ == "__main__":
    main()
