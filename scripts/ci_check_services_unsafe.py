#!/usr/bin/env python3
"""
检查 services/ 层是否有任何 unsafe 代码 (排除注释行).

用法: python3 scripts/ci_check_services_unsafe.py
退出码: 0 = 通过 (无 unsafe), 1 = 有违规
"""

import re
import sys
from pathlib import Path


def main():
    services = Path('src/kernel/services')
    if not services.exists():
        print(f'ERROR: {services} not found', file=sys.stderr)
        return 2

    issues = []
    rs_files = sorted(services.rglob('*.rs'))

    for f in rs_files:
        try:
            content = f.read_text(encoding='utf-8', errors='replace')
        except Exception as e:
            print(f'WARN: {f}: {e}', file=sys.stderr)
            continue

        for ln, line in enumerate(content.splitlines(), 1):
            stripped = line.strip()
            # 跳过注释行
            if (stripped.startswith('//') or
                stripped.startswith('*') or
                stripped.startswith('/*')):
                continue
            # 跳过 #![deny(...)] 等 attribute
            if stripped.startswith('#![') or stripped.startswith('#['):
                continue

            if re.search(r'\bunsafe\s*\{', line):
                issues.append(f'{f}:{ln}: unsafe {{ block: {line.strip()[:80]}')
            elif re.search(r'\bunsafe\s+fn\b', line):
                issues.append(f'{f}:{ln}: unsafe fn: {line.strip()[:80]}')
            elif re.search(r'\bunsafe\s+impl\b', line):
                issues.append(f'{f}:{ln}: unsafe impl: {line.strip()[:80]}')
            elif re.search(r'\bunsafe\s+trait\b', line):
                issues.append(f'{f}:{ln}: unsafe trait: {line.strip()[:80]}')

    print(f'扫描文件数: {len(rs_files)}')
    if issues:
        print(f'FAIL: services/ 中发现 {len(issues)} 处 unsafe')
        for i in issues:
            print(f'  {i}')
        return 1

    print('PASS: services/ 0 unsafe')
    return 0


if __name__ == '__main__':
    sys.exit(main())
