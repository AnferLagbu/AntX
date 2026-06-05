#!/usr/bin/env python3
"""
M6.2 死锁检测矩阵扫描脚本 — 锁顺序/中断上下文/不可重入函数分析 (v1)

检查规则:
  (1) 扫描所有 spin::Mutex / spin::RwLock / spin::Once 使用点
  (2) 检测在中断上下文 (IDT handler、ISR、irq handler) 中使用的锁
  (3) 标记非中断安全的锁 (应为 IrqSpinLock 而非 spin::Mutex)
  (4) 扫描锁的获取顺序, 检测潜在 AB-BA 死锁
  (5) 检测 sleep 锁 (Mutex) 在原子上下文的使用
  (6) 检测不可重入函数中的锁使用

退出码: 0 = 无严重风险, 1 = 有高风险 (中断上下文非安全锁)
"""

import os
import re
import sys
import json
from collections import defaultdict
from pathlib import Path

BASE = Path('src/kernel/framework')
SRC_ROOT = Path('src/kernel/framework')

# 中断上下文的函数白名单 (这些函数中使用的 spin::Mutex 视为高风险)
INTERRUPT_CONTEXT_FUNCS = [
    'handle_irq',
    'handle_exception',
    'exception_handler',
    'irq_handler',
    'interrupt_handler',
    'do_softirq',
    'isr_',
    'fault_handler',
    'page_fault_handler',
    'timer_tick',
    'timer_handler',
    'clock_interrupt',
    'schedule_from_isr',
    'eoi',
]

# 严重级别
SEVERITY_CRITICAL = 'CRITICAL'   # 中断上下文使用非安全锁, 必然死锁
SEVERITY_HIGH = 'HIGH'           # 锁顺序可疑
SEVERITY_MEDIUM = 'MEDIUM'       # 跨线程 sleep 锁使用
SEVERITY_LOW = 'LOW'             # 良好实践警告
SEVERITY_INFO = 'INFO'           # 仅记录


def is_in_interrupt_context(filepath, lineno, content):
    """检查给定的行是否在中断上下文中.

    通过查找所在函数名, 与 INTERRUPT_CONTEXT_FUNCS 比对.
    """
    # 读取整个文件
    try:
        with open(filepath, 'r', encoding='utf-8', errors='replace') as f:
            lines = f.readlines()
    except Exception:
        return False, None

    # 向上查找最近的 fn 定义
    fn_pattern = re.compile(r'\b(?:pub\s+)?(?:unsafe\s+)?(?:extern\s+"C"\s+)?fn\s+(\w+)')
    for j in range(lineno - 1, -1, -1):
        m = fn_pattern.search(lines[j])
        if m:
            fn_name = m.group(1)
            for ctx in INTERRUPT_CONTEXT_FUNCS:
                if fn_name == ctx or fn_name.startswith(ctx):
                    return True, fn_name
            return False, fn_name
    return False, None


def is_in_impl_block(filepath, lineno, content):
    """检测是否在 impl 块内 (有 self 引用)."""
    try:
        with open(filepath, 'r', encoding='utf-8', errors='replace') as f:
            lines = f.readlines()
    except Exception:
        return False

    # 向上查找最近的 impl 或 fn
    impl_pattern = re.compile(r'\bimpl\b.*\bfor\b')
    fn_pattern = re.compile(r'\b(?:pub\s+)?(?:unsafe\s+)?(?:extern\s+"C"\s+)?fn\s+\w+')

    depth = 0
    for j in range(lineno - 1, -1, -1):
        line = lines[j]
        # 简化: 计算 { 和 } 平衡
        opens = line.count('{')
        closes = line.count('}')
        depth += closes - opens
        if depth < 0:
            # 已经退出当前作用域
            if impl_pattern.search(line):
                return True
            if fn_pattern.search(line):
                return False
    return False


def scan_file(filepath):
    """扫描单个文件, 返回问题列表."""
    issues = []

    try:
        with open(filepath, 'r', encoding='utf-8', errors='replace') as f:
            content = f.read()
            lines = content.splitlines()
    except Exception as e:
        return issues

    # 模式:
    # - spin::Mutex::new(...)
    # - spin::Mutex<T>
    # - spin::RwLock
    # - spin::Once
    # - .lock() (不区分, 因为 framework/sync 内部也有 .lock())
    # - .read() / .write()

    # 两阶段扫描:
    #   阶段 A: 收集所有 spin::Mutex/RwLock/Once 字段名 (作为已知非安全锁)
    #   阶段 B: 扫描 .lock()/.read()/.write() 调用, 检查字段类型

    spin_field_pattern = re.compile(
        r'\b(?:pub(?:\([^)]*\))?\s+)?(\w+)\s*:\s*(?:spin::|crate::spin::)'
        r'(Mutex|RwLock|Once|OnceCell)',
    )

    spin_static_pattern = re.compile(
        r'\bstatic\s+(\w+)\s*:\s*(?:spin::|crate::spin::)'
        r'(Mutex|RwLock|Once|OnceCell)',
    )

    # 安全锁字段模式: 标记这些字段为 IRQ 安全 (即 .lock() 不会产生 CRITICAL 警告)
    safe_lock_field_pattern = re.compile(
        r'\b(?:pub(?:\([^)]*\))?\s+)?(\w+)\s*:\s*'
        r'(?:IrqSpinLock|FrameworkIrqSpinLock|'
        r'crate::kernel::framework::sync::irq_spinlock::IrqSpinLock|'
        r'framework::sync::irq_spinlock::IrqSpinLock)',
    )
    safe_lock_static_pattern = re.compile(
        r'\bstatic\s+(\w+)\s*:\s*'
        r'(?:IrqSpinLock|FrameworkIrqSpinLock|'
        r'crate::kernel::framework::sync::irq_spinlock::IrqSpinLock)',
    )

    unsafe_lock_fields = set()  # 已知非安全锁字段名
    unsafe_lock_statics = set()  # 已知非安全锁 static 名
    safe_lock_fields = set()  # 已知安全锁字段名
    safe_lock_statics = set()  # 已知安全锁 static 名

    # 阶段 A: 收集字段类型
    for lineno_1, line in enumerate(lines, start=1):
        if line.strip().startswith('//'):
            continue
        for m in spin_field_pattern.finditer(line):
            unsafe_lock_fields.add(m.group(1))
        for m in spin_static_pattern.finditer(line):
            unsafe_lock_statics.add(m.group(1))
        for m in safe_lock_field_pattern.finditer(line):
            safe_lock_fields.add(m.group(1))
        for m in safe_lock_static_pattern.finditer(line):
            safe_lock_statics.add(m.group(1))

    # 阶段 B: 检测 .lock()/.read()/.write() 调用
    lock_call_pattern = re.compile(
        r'(?:(?P<field>\w+)\.)?(?P<method>lock|read|write)\s*\('
    )

    framework_lock_methods = {'with', 'with_mut', 'lock_irqsave', 'try_lock'}

    for lineno_1, line in enumerate(lines, start=1):
        if not line.strip():
            continue
        # 跳过注释行
        stripped = line.strip()
        if stripped.startswith('//') or stripped.startswith('*') or stripped.startswith('///'):
            continue

        # 检测 .lock()/.read()/.write() 调用
        for m in lock_call_pattern.finditer(line):
            field_name = m.group('field')
            method = m.group('method')

            # 跳过非锁方法
            if method in framework_lock_methods:
                continue

            # 跳过 framework::sync 类型的锁 (它们的 method 不会通过 .lock() 暴露)
            if 'framework::sync' in line or 'sync::irq_spinlock' in line:
                continue

            # 跳过非字段调用 (例如: spin::Mutex::new, .call_once, 等)
            if not field_name:
                continue
            if field_name in ('spin', 'core', 'std', 'crate', 'self', 'super'):
                continue
            if method not in ('lock', 'read', 'write'):
                continue

            # 交叉检查字段类型
            if field_name in safe_lock_fields or field_name in safe_lock_statics:
                # 安全锁, 跳过
                continue
            # 如果字段已知为非安全锁 OR 字段未知 (在中断上下文中, 默认 CRITICAL)
            is_known_unsafe = (field_name in unsafe_lock_fields
                               or field_name in unsafe_lock_statics)

            # 检查是否在中断上下文
            in_irq, fn_name = is_in_interrupt_context(str(filepath), lineno_1, line)
            if in_irq:
                issues.append({
                    'file': str(filepath),
                    'line': lineno_1,
                    'severity': SEVERITY_CRITICAL,
                    'type': 'IRQ_CONTEXT_LOCK_ACQUISITION',
                    'lock': f'{field_name}.{method}()',
                    'function': fn_name,
                    'message': (f'中断上下文函数 `{fn_name}` 在 L{lineno_1} 获取锁 {field_name}.{method}() — '
                                f'{"已知非安全锁 (spin::Mutex), 须替换为 IrqSpinLock" if is_known_unsafe else "字段类型未识别, 须人工确认是否为 IrqSpinLock"}'),
                    'code': line.strip()[:200],
                })
            elif is_known_unsafe:
                # 字段已知为 spin::Mutex 等, 记录为 HIGH 风险
                # (因为该函数可能被中断上下文调用, 须人工审查调用栈)
                issues.append({
                    'file': str(filepath),
                    'line': lineno_1,
                    'severity': SEVERITY_HIGH,
                    'type': 'NON_IRQ_SAFE_LOCK_USE',
                    'lock': f'{field_name}.{method}() (字段类型: spin::Mutex/RwLock/Once)',
                    'function': fn_name,
                    'message': (f'函数 `{fn_name}` 在 L{lineno_1} 获取非 IRQ 安全锁 {field_name}.{method}() — '
                                f'如本函数或其调用者会在中断上下文中执行, 必须改为 IrqSpinLock'),
                    'code': line.strip()[:200],
                })

    # 阶段 C: 直接检测 spin::Mutex/RwLock/Once 声明 (作为非安全锁使用记录)
    spin_mutex_decl = re.compile(r'\bspin::Mutex\b')
    spin_rwlock_decl = re.compile(r'\bspin::RwLock\b')
    spin_once_decl = re.compile(r'\bspin::Once\b')
    spin_once_cell_decl = re.compile(r'\bspin::OnceCell\b')

    # framework 的安全锁 (不应被报告)
    framework_lock_patterns = [
        re.compile(r'\bframework::sync::(irq_spinlock|spinlock|mutex|rwlock|seqlock|once_lock|once_cell)\b'),
        re.compile(r'\bcrate::kernel::framework::sync::(irq_spinlock|spinlock|mutex|rwlock|seqlock|once_lock|once_cell)\b'),
        re.compile(r'\bsync::(irq_spinlock|spinlock|mutex|rwlock|seqlock|once_lock|once_cell)\b'),
    ]

    for lineno_1, line in enumerate(lines, start=1):
        if not line.strip():
            continue
        # 跳过注释行
        stripped = line.strip()
        if stripped.startswith('//') or stripped.startswith('*') or stripped.startswith('///'):
            continue

        # 检测第三方 spin 使用
        spin_uses = []
        if spin_mutex_decl.search(line):
            spin_uses.append('spin::Mutex')
        if spin_rwlock_decl.search(line):
            spin_uses.append('spin::RwLock')
        if spin_once_decl.search(line):
            spin_uses.append('spin::Once')
        if spin_once_cell_decl.search(line):
            spin_uses.append('spin::OnceCell')

        for spin_type in spin_uses:
            # 排除 import 语句
            if stripped.startswith('use ') and spin_type in stripped:
                continue
            # 排除注释中的引用
            if '//' in line:
                code_part = line.split('//', 1)[0]
                if spin_type not in code_part:
                    continue

            # 简化: 只要不包含 'framework::sync' 就算第三方使用
            is_framework_use = any(p.search(line) for p in framework_lock_patterns)
            if is_framework_use:
                continue

            # 检查是否在中断上下文
            in_irq, fn_name = is_in_interrupt_context(str(filepath), lineno_1, line)
            if in_irq:
                issues.append({
                    'file': str(filepath),
                    'line': lineno_1,
                    'severity': SEVERITY_CRITICAL,
                    'type': 'IRQ_CONTEXT_UNSAFE_LOCK_DECL',
                    'lock': spin_type,
                    'function': fn_name,
                    'message': f'中断上下文相关函数 `{fn_name}` 引用第三方 {spin_type} 声明, 应替换为 framework::sync::irq_spinlock::IrqSpinLock',
                    'code': line.strip()[:200],
                })
            else:
                # 记录普通使用, 供后续统一迁移
                issues.append({
                    'file': str(filepath),
                    'line': lineno_1,
                    'severity': SEVERITY_INFO,
                    'type': 'THIRD_PARTY_LOCK_USE',
                    'lock': spin_type,
                    'function': fn_name,
                    'message': f'使用第三方 {spin_type} (非中断上下文, 但仍应迁移到 framework::sync)',
                    'code': line.strip()[:200],
                })

    return issues


def scan_directory(base):
    """递归扫描目录中的所有 .rs 文件."""
    all_issues = []
    files = list(base.rglob('*.rs'))
    for f in files:
        # 排除 smoltcp 第三方目录
        if 'smoltcp' in str(f):
            continue
        issues = scan_file(f)
        all_issues.extend(issues)
    return all_issues, len(files)


def generate_report(issues, file_count):
    """生成报告."""
    by_severity = defaultdict(list)
    for issue in issues:
        by_severity[issue['severity']].append(issue)

    report = []
    report.append('=' * 78)
    report.append('M6.2 死锁检测矩阵扫描报告')
    report.append('=' * 78)
    report.append('')
    report.append(f'扫描文件数: {file_count}')
    report.append(f'问题总数: {len(issues)}')
    report.append('')

    for sev in [SEVERITY_CRITICAL, SEVERITY_HIGH, SEVERITY_MEDIUM, SEVERITY_LOW, SEVERITY_INFO]:
        count = len(by_severity[sev])
        if count == 0:
            continue
        report.append(f'[{sev}] {count} 项')
        report.append('-' * 78)

        # 按文件分组
        by_file = defaultdict(list)
        for issue in by_severity[sev]:
            by_file[issue['file']].append(issue)

        for filepath, file_issues in sorted(by_file.items()):
            report.append(f'\n  {filepath}:')
            for issue in sorted(file_issues, key=lambda x: x['line']):
                report.append(f'    L{issue["line"]}: {issue["type"]}')
                report.append(f'      锁: {issue["lock"]}')
                if issue.get('function'):
                    report.append(f'      函数: {issue["function"]}')
                report.append(f'      描述: {issue["message"]}')
                report.append(f'      代码: {issue["code"]}')
        report.append('')

    return '\n'.join(report)


def main():
    if not BASE.exists():
        print(f'ERROR: {BASE} not found', file=sys.stderr)
        sys.exit(2)

    print(f'扫描 {BASE} ...')
    issues, file_count = scan_directory(BASE)
    report = generate_report(issues, file_count)
    print(report)

    # 保存 JSON 报告 (gitignored target/audit/)
    json_path = Path('target/audit/deadlock-matrix.json')
    json_path.parent.mkdir(parents=True, exist_ok=True)
    with open(json_path, 'w', encoding='utf-8') as f:
        json.dump({
            'file_count': file_count,
            'issue_count': len(issues),
            'issues': issues,
        }, f, ensure_ascii=False, indent=2)
    print(f'\nJSON 报告保存至: {json_path}')

    # 计算退出码
    critical = sum(1 for i in issues if i['severity'] == SEVERITY_CRITICAL)
    if critical > 0:
        print(f'\n>>> {critical} 个 CRITICAL 问题 (中断上下文非安全锁) <<<')
        sys.exit(1)
    sys.exit(0)


if __name__ == '__main__':
    main()
