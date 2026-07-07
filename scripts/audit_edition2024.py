#!/usr/bin/env python3
"""
Edition 2024 迁移扫描脚本 v2

更精确地扫描需要修改的地方：
1. unsafe fn 内的 unsafe 操作（需要添加 unsafe {} 块）
2. extern "C" 块（需要添加 unsafe 标注）
3. 统计工作量
"""

import os
import re
import sys
import json
from pathlib import Path
from collections import defaultdict

# 扫描目录
SCAN_DIRS = [
    'src/kernel/framework',
    'src/kernel/services',
    'src/rust',
]

# 跳过的目录
SKIP_DIRS = {
    'target', '.git', 'node_modules', 'venv', '__pycache__',
    'src/kernel/services/net/smoltcp',
}

# 文件扩展名
RUST_EXTENSIONS = {'.rs'}


def find_rust_files(base_dirs):
    """找到所有 Rust 源文件"""
    files = []
    for base in base_dirs:
        if not os.path.exists(base):
            continue
        for root, dirs, filenames in os.walk(base):
            dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
            for f in filenames:
                if any(f.endswith(ext) for ext in RUST_EXTENSIONS):
                    files.append(os.path.join(root, f))
    return files


def analyze_unsafe_fns(content, filepath):
    """分析 unsafe fn 内的 unsafe 操作"""
    results = []
    lines = content.split('\n')
    
    # 状态跟踪
    in_unsafe_fn = False
    unsafe_fn_name = ""
    unsafe_fn_line = 0
    brace_depth = 0
    unsafe_fn_start_depth = 0
    
    # 用于检测函数体内的 unsafe 操作
    # 在 unsafe fn 内，每个 unsafe 操作都需要显式 unsafe {} 块
    
    for i, line in enumerate(lines, 1):
        stripped = line.strip()
        
        # 检测 unsafe fn 定义
        unsafe_fn_match = re.search(r'(pub\s+)?unsafe\s+fn\s+(\w+)', line)
        if unsafe_fn_match:
            in_unsafe_fn = True
            unsafe_fn_name = unsafe_fn_match.group(2)
            unsafe_fn_line = i
            unsafe_fn_start_depth = brace_depth
            continue
        
        if in_unsafe_fn:
            # 计算大括号深度
            for ch in line:
                if ch == '{':
                    brace_depth += 1
                elif ch == '}':
                    brace_depth -= 1
            
            # 检测裸指针解引用 (在 unsafe fn 内需要 unsafe {} 块)
            if re.search(r'\*\w+', stripped) and not stripped.startswith('//'):
                # 排除注释和字符串
                if not stripped.startswith('*') or stripped.startswith('*const') or stripped.startswith('*mut'):
                    results.append({
                        'file': filepath,
                        'line': i,
                        'unsafe_fn': unsafe_fn_name,
                        'unsafe_fn_line': unsafe_fn_line,
                        'content': stripped,
                        'type': 'unsafe_fn_deref',
                        'severity': 'high'
                    })
            
            # 检测 unsafe 块调用
            if re.search(r'\bunsafe\s*\{', stripped):
                results.append({
                    'file': filepath,
                    'line': i,
                    'unsafe_fn': unsafe_fn_name,
                    'unsafe_fn_line': unsafe_fn_line,
                    'content': stripped,
                    'type': 'unsafe_block_in_fn',
                    'severity': 'low'  # 已经有 unsafe 块
                })
            
            # 函数结束
            if brace_depth <= unsafe_fn_start_depth and i > unsafe_fn_line + 1:
                in_unsafe_fn = False
    
    return results


def find_extern_c_blocks(content, filepath):
    """找到所有 extern "C" 块"""
    results = []
    lines = content.split('\n')
    
    for i, line in enumerate(lines, 1):
        stripped = line.strip()
        # 匹配 extern "C" 块（不带 unsafe）
        if re.search(r'extern\s+"C"\s*\{', stripped) and 'unsafe' not in stripped:
            results.append({
                'file': filepath,
                'line': i,
                'content': stripped,
                'type': 'extern_c_block',
                'severity': 'medium'
            })
    
    return results


def analyze_file(filepath):
    """分析单个文件"""
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            content = f.read()
    except Exception as e:
        return []
    
    results = []
    results.extend(analyze_unsafe_fns(content, filepath))
    results.extend(find_extern_c_blocks(content, filepath))
    return results


def generate_report(all_results):
    """生成报告"""
    # 按类型分组
    by_type = defaultdict(list)
    for r in all_results:
        by_type[r['type']].append(r)
    
    # 按文件分组
    by_file = defaultdict(list)
    for r in all_results:
        by_file[r['file']].append(r)
    
    # 按严重程度分组
    by_severity = defaultdict(list)
    for r in all_results:
        by_severity[r['severity']].append(r)
    
    print("=" * 80)
    print("Edition 2024 迁移扫描报告")
    print("=" * 80)
    
    # 统计
    print("\n## 统计概览")
    print(f"  高优先级 (unsafe 操作): {len(by_severity.get('high', []))} 处")
    print(f"  中优先级 (extern \"C\"): {len(by_severity.get('medium', []))} 处")
    print(f"  低优先级 (已有 unsafe 块): {len(by_severity.get('low', []))} 处")
    
    # 需要修改的地方
    print("\n## 需要修改的地方")
    
    # 1. unsafe fn 内的裸指针操作
    unsafe_deref = by_type.get('unsafe_fn_deref', [])
    if unsafe_deref:
        print(f"\n### 1. unsafe fn 内的裸指针操作 ({len(unsafe_deref)} 处)")
        print("  edition 2024 要求每个 unsafe 操作显式包裹 unsafe {} 块")
        for r in unsafe_deref[:15]:
            print(f"  {r['file']}:{r['line']} (unsafe fn '{r['unsafe_fn']}' at line {r['unsafe_fn_line']})")
        if len(unsafe_deref) > 15:
            print(f"  ... 还有 {len(unsafe_deref) - 15} 处")
    
    # 2. 需要 unsafe 标注的 extern "C" 块
    extern_c = by_type.get('extern_c_block', [])
    if extern_c:
        print(f"\n### 2. 需要 unsafe 标注的 extern \"C\" 块 ({len(extern_c)} 处)")
        print("  edition 2024 要求 unsafe extern \"C\" { ... }")
        for r in extern_c[:15]:
            print(f"  {r['file']}:{r['line']}")
        if len(extern_c) > 15:
            print(f"  ... 还有 {len(extern_c) - 15} 处")
    
    # 3. 按文件统计
    print("\n### 3. 按文件统计 (Top 10)")
    file_stats = [(f, len(items)) for f, items in by_file.items()]
    file_stats.sort(key=lambda x: x[1], reverse=True)
    
    for f, count in file_stats[:10]:
        print(f"  {f}: {count} 处")
    
    # 工作量估算
    print("\n## 工作量估算")
    high_count = len(by_severity.get('high', []))
    medium_count = len(by_severity.get('medium', []))
    
    # 高优先级需要手动修改
    high_effort = high_count * 3  # 每处约 3 行改动
    # 中优先级可以用 cargo fix 自动修复
    medium_effort = medium_count * 1  # 每处约 1 行改动
    
    total_effort = high_effort + medium_effort
    
    print(f"  高优先级 (手动修复): {high_count} 处 × 3 行 = {high_effort} 行")
    print(f"  中优先级 (自动修复): {medium_count} 处 × 1 行 = {medium_effort} 行")
    print(f"  总计: {total_effort} 行代码改动")
    print(f"  预计时间: {total_effort // 50} 天 (按每天 50 行计算)")
    
    # 优先级建议
    print("\n## 迁移优先级建议")
    print("  1. 先处理 extern \"C\" 块 (可自动修复)")
    print("  2. 再处理 unsafe fn 内的裸指针操作 (需手动)")
    print("  3. 最后验证和测试")
    
    return len(all_results) > 0


def main():
    """主函数"""
    files = find_rust_files(SCAN_DIRS)
    print(f"扫描 {len(files)} 个 Rust 文件...")
    
    all_results = []
    for f in files:
        results = analyze_file(f)
        all_results.extend(results)
    
    has_issues = generate_report(all_results)
    
    # 保存详细结果
    output_file = 'target/edition2024-scan.json'
    os.makedirs(os.path.dirname(output_file), exist_ok=True)
    
    with open(output_file, 'w') as f:
        json.dump(all_results, f, indent=2, ensure_ascii=False)
    
    print(f"\n详细结果已保存到: {output_file}")
    
    return 0 if not has_issues else 1


if __name__ == '__main__':
    sys.exit(main())