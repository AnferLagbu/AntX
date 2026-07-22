#!/usr/bin/env python3
"""
M6.5 services 层隐式依赖审计脚本

检查 services 层模块对 framework 全局状态的引用,
检测通过全局静态变量产生的隐式依赖。

退出码: 0 = 通过, 1 = 有严重违规
"""

import os
import re
import sys
from pathlib import Path

SERVICES_BASE = Path('src/kernel/services')
FRAMEWORK_BASE = Path('src/kernel/framework')

# framework 全局状态变量 (services 不应直接访问)
# 仅包含实际定义在 framework 中的变量
FRAMEWORK_GLOBALS = [
    'SCHEDULER',
    'PROCESS_TABLE',
    'SOCKET_SET',
    'NET_DEVICE',
    'NET_STACK',
    'GLOBAL_FRAMEBUFFER',
    'VGA_DRIVER',
    'ISR_TABLE',
    'LOG_SINKS',
    'SLAB_CACHES',
    'GENERAL_CACHES',
    'GLOBAL_DMA',
    'GLOBAL_KMALLOC',
]

# 允许的直接引用 (framework 层安全 API)
ALLOWED_PATTERNS = [
    r'crate::kernel::framework::fs::VFS_MANAGER',
    r'crate::kernel::framework::ipc::IPC_NAMESPACE',
]

def find_framework_globals():
    """查找 framework 中实际定义的全局变量"""
    globals_found = set()
    for rust_file in FRAMEWORK_BASE.rglob('*.rs'):
        if 'smoltcp' in str(rust_file):
            continue
        with open(rust_file, 'r', encoding='utf-8', errors='ignore') as f:
            for line in f:
                for global_name in FRAMEWORK_GLOBALS:
                    if f'static mut {global_name}' in line or f'static {global_name}:' in line:
                        globals_found.add(global_name)
    return globals_found

def scan_services(framework_globals):
    """扫描 services 层对 framework 全局状态的引用"""
    violations = []
    
    for rust_file in SERVICES_BASE.rglob('*.rs'):
        if 'smoltcp' in str(rust_file):
            continue
            
        with open(rust_file, 'r', encoding='utf-8', errors='ignore') as f:
            lines = f.read().split('\n')
            
            for line_num, line in enumerate(lines, 1):
                stripped = line.strip()
                if stripped.startswith('//') or stripped.startswith('/*') or stripped.startswith('*'):
                    continue
                
                for global_name in framework_globals:
                    if global_name in line:
                        is_allowed = any(re.search(pattern, line) for pattern in ALLOWED_PATTERNS)
                        if not is_allowed:
                            violations.append({
                                'file': str(rust_file),
                                'line': line_num,
                                'global': global_name,
                                'code': line.strip(),
                            })
    
    return violations

def main():
    print("M6.5 services 层隐式依赖审计")
    print("=" * 60)
    
    # 先查找 framework 中实际定义的全局变量
    framework_globals = find_framework_globals()
    print(f"\n检测到 framework 全局变量: {len(framework_globals)} 个")
    for g in sorted(framework_globals):
        print(f"  - {g}")
    
    violations = scan_services(framework_globals)
    
    if violations:
        print(f"\n发现 {len(violations)} 处隐式依赖:")
        print("-" * 60)
        
        for v in violations:
            rel_path = os.path.relpath(v['file'], Path.cwd())
            print(f"  {rel_path}:{v['line']}: {v['global']}")
            print(f"    {v['code'][:80]}")
            print()
        
        print("-" * 60)
        print(f"FAIL: {len(violations)} 处 services 层直接访问 framework 全局状态")
        sys.exit(1)
    else:
        print("\nPASS: services 层无隐式依赖")
        sys.exit(0)

if __name__ == '__main__':
    main()
