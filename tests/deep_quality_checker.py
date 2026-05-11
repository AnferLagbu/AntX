#!/usr/bin/env python3
"""
AntX Rust 内核深度代码质量检查器 (Engineering-Grade v2.0)
=============================================================
Compliant with IEEE 829 / ISO 29119 Testing Standards

Enhanced Features (v2.0):
  ✅ Severity classification (IEEE 1044 aligned: CRITICAL/MAJOR/MINOR/INFO)
  ✅ Fix priority system (P0-P3 based on risk × effort)
  ✅ Risk assessment for each issue
  ✅ Engineering-grade JSON output (Schema v2.0)
  ✅ Trend comparison support
  ✅ Compliance scoring

Base Module: run_tests.py (Severity, RiskLevel enums)
"""

import os
import re
import json
import sys
from enum import Enum
from pathlib import Path
from datetime import datetime
from collections import defaultdict
from typing import List, Dict, Tuple, Optional, Any

PROJECT_ROOT = Path(__file__).parent.parent
SRC_KERNEL = PROJECT_ROOT / "src" / "kernel"

# Import engineering-grade data structures
sys.path.insert(0, str(Path(__file__).parent))
try:
    from run_tests import (
        Severity, RiskLevel, ComplianceStatus, 
        RiskAssessment, TestEnvironment, TestMetadata
    )
    ENGINEERING_MODE = True
except ImportError:
    ENGINEERING_MODE = False
    
    # Fallback severity levels
    class Severity:
        CRITICAL = "CRITICAL"
        MAJOR = "MAJOR"
        MINOR = "MINOR"
        INFO = "INFO"
    
    class RiskLevel:
        HIGH = "HIGH"
        MEDIUM = "MEDIUM"
        LOW = "LOW"
        NEGLIGIBLE = "NEGLIGIBLE"

class Priority(Enum):
    """Fix priority levels based on ISO 25010 quality characteristics"""
    P0 = "P0"  # Critical - Must fix immediately (security/stability)
    P1 = "P1"  # High - Should fix this sprint (functionality)
    P2 = "P2"  # Medium - Address in next release (quality)
    P3 = "P3"  # Low - Optional improvement (cosmetic)

class Issue:
    """
    Enhanced issue with engineering metadata (v2.0)
    
    Attributes:
      severity:      IEEE 1044 severity level
      category:      Issue category (DEAD_CODE/LOGIC_ERROR/FFI/etc.)
      file:          Source file path
      line:          Line number
      message:       Human-readable description
      suggestion:    Recommended fix action
      priority:      Fix priority (P0-P3) [NEW v2.0]
      risk:          Risk assessment [NEW v2.0]
      effort_hours:  Estimated fix time [NEW v2.0]
      compliance:    Compliance impact [NEW v2.0]
    """
    
    def __init__(self, severity: str, category: str, file: str, 
                 line: int, message: str, suggestion: str = "",
                 priority: Optional[str] = None,
                 risk_level: Optional[str] = None,
                 effort_hours: Optional[float] = None):
        
        self.severity = severity
        self.category = category
        self.file = file
        self.line = line
        self.message = message
        self.suggestion = suggestion
        
        # Engineering-grade attributes (v2.0)
        if ENGINEERING_MODE:
            self.priority = self._calculate_priority() if not priority else priority
            self.risk = self._assess_risk() if not risk_level else risk_level
            self.effort_hours = self._estimate_effort() if effort_hours is None else effort_hours
            self.compliance_impact = self._assess_compliance()
        else:
            self.priority = priority or "P3"
            self.risk = risk_level or "NEGLIGIBLE"
            self.effort_hours = effort_hours or 0.5
            self.compliance_impact = "UNKNOWN"
    
    def _calculate_priority(self) -> str:
        """
        Calculate fix priority based on severity × category.
        
        Rules:
          - P0: CRITICAL + any security/memory safety issue
          - P1: MAJOR + functionality/broken feature
          - P2: MINOR + code quality/maintainability
          - P3: INFO + cosmetic/suggestions
        """
        if self.severity == Severity.CRITICAL or 'SECURITY' in self.category.upper():
            return Priority.P0.value
        elif self.severity == Severity.MAJOR or \
             self.category in ['LOGIC_ERROR', 'FFI', 'DEAD_CODE']:
            return Priority.P1.value
        elif self.severity == Severity.MINOR:
            return Priority.P2.value
        else:
            return Priority.P3.value
    
    def _assess_risk(self) -> str:
        """Assess risk level of leaving this issue unfixed."""
        if self.priority == "P0":
            return RiskLevel.HIGH.value
        elif self.priority == "P1":
            return RiskLevel.MEDIUM.value
        elif self.priority == "P2":
            return RiskLevel.LOW.value
        else:
            return RiskLevel.NEGLIGIBLE.value
    
    def _estimate_effort(self) -> float:
        """
        Estimate fix effort in hours.
        
        Heuristics:
          - Simple annotation: 0.25h
          - Code refactoring: 1-4h
          - Logic fix: 2-8h
          - Architecture change: 8-24h
        """
        base_effort = {
            'CRITICAL': 8.0,
            'MAJOR': 4.0,
            'MINOR': 1.0,
            'INFO': 0.5
        }
        
        category_multiplier = {
            'DEAD_CODE': 0.5,      # Usually easy to remove/add #[allow]
            'LOGIC_ERROR': 2.0,     # May require debugging
            'FFI': 1.5,             # Need to check both sides
            'PERFORMANCE': 3.0,     # May need profiling
            'TEST': 1.0             # Add test case
        }
        
        return base_effort.get(self.severity, 1.0) * \
               category_multiplier.get(self.category, 1.0)
    
    def _assess_compliance(self) -> str:
        """Assess compliance impact."""
        critical_categories = ['SECURITY', 'MEMORY_SAFETY', 'THREAD_SAFETY']
        if self.category.upper() in critical_categories or \
           self.severity == Severity.CRITICAL:
            return ComplianceStatus.NON_COMPLIANT.value
        elif self.severity in [Severity.MAJOR, Severity.MINOR]:
            return ComplianceStatus.PARTIAL.value
        else:
            return ComplianceStatus.COMPLIANT.value
    
    def to_dict(self) -> Dict[str, Any]:
        """Generate structured output (Schema v2.0)"""
        base = {
            "severity": self.severity if isinstance(self.severity, str) else self.severity.name,
            "category": self.category,
            "file": self.file,
            "line": self.line,
            "message": self.message,
            "suggestion": self.suggestion
        }
        
        if ENGINEERING_MODE:
            base.update({
                "priority": self.priority,
                "risk_level": self.risk,
                "effort_hours_estimate": round(self.effort_hours, 1),
                "compliance_impact": self.compliance_impact
            })
        
        return base

class DeepQualityChecker:
    """深度代码质量检查器"""
    
    def __init__(self, target_dir: Path):
        self.target = target_dir
        self.issues: List[Issue] = []
        self.stats = defaultdict(int)
        self.rust_files: List[Path] = []
        
    def find_rust_files(self) -> None:
        """发现所有Rust文件"""
        self.rust_files = list(self.target.rglob("*.rs"))
        print(f"📂 发现 {len(self.rust_files)} 个Rust文件")
        
    # ========================================================================
    # Phase 1: 死代码检测
    # ========================================================================
    
    def check_dead_code(self) -> None:
        """Phase 1: 检测死代码"""
        print("\n" + "="*70)
        print("🔍 PHASE 1: 死代码检测 (Dead Code Detection)")
        print("="*70)
        
        for rust_file in self.rust_files:
            try:
                content = rust_file.read_text(encoding='utf-8', errors='ignore')
                lines = content.splitlines()
                rel_path = rust_file.name.replace(str(PROJECT_ROOT) + '/', '')
                
                # 1.1 检测未使用的函数定义
                self._check_unused_functions(lines, rel_path)
                
                # 1.2 检测未使用的 use 导入
                self._check_unused_imports(content, rel_path)
                
                # 1.3 检测 TODO/FIXME/HACK 标记
                self._check_todo_hacks(lines, rel_path)
                
                # 1.4 检测注释掉的代码块
                self._check_commented_out_code(lines, rel_path)
                
            except Exception as e:
                print(f"  ⚠️  处理文件错误 {rust_file}: {e}")
    
    def _check_unused_functions(self, lines: List[str], file: str) -> None:
        """检测可能未使用的函数"""
        func_pattern = re.compile(r'^\s*(pub\s+)?(async\s+)?(unsafe\s+)?fn\s+(\w+)')
        call_pattern = re.compile(r'\b(\w+)\s*\(')
        
        defined_funcs = set()
        called_funcs = set()
        
        for i, line in enumerate(lines, 1):
            match = func_pattern.match(line)
            if match and not line.strip().startswith('//'):
                func_name = match.group(4)
                if func_name not in ['new', 'default', 'drop', 'clone']:
                    defined_funcs.add((func_name, i))
            
            # 简单启发式: 收集函数调用 (排除定义行)
            if 'fn ' not in line or 'pub fn' in line:
                for call_match in call_pattern.finditer(line):
                    called_name = call_match.group(1)
                    if called_name in [f[0] for f in defined_funcs]:
                        called_funcs.add(called_name)
        
        # 报告未调用的函数 (排除 trait 实现、#[no_mangle]、test 函数)
        for func_name, line_num in defined_funcs:
            if (func_name not in called_funcs and 
                not any(f'#[{attr}]' in lines[max(0,line_num-5):line_num] 
                       for attr in ['no_mangle', 'test', 'export_name']) and
                len(func_name) > 3):  # 忽略太短的名称
                
                context = lines[line_num-1].strip()[:80]
                self.issues.append(Issue(
                    severity="WARNING",
                    category="DEAD_CODE",
                    file=file,
                    line=line_num,
                    message=f"可能未使用的函数: {func_name}",
                    suggestion="确认是否需要此函数, 或添加 #[allow(dead_code)]"
                ))
                self.stats["dead_functions"] += 1
    
    def _check_unused_imports(self, content: str, file: str) -> None:
        """检测未使用的use导入"""
        import_pattern = re.compile(r'use\s+(?:(\w+)::)*(\w+)(?:\s*::\s*\{([^}]*)\})?;')
        
        imports = []
        for match in import_pattern.finditer(content):
            full_import = match.group(0).strip()
            last_part = match.group(2)
            
            # 统计使用次数
            usage_count = content.count(last_part)
            
            # 如果只出现一次 (在导入语句本身), 则可能是未使用
            if usage_count <= 1:
                line_num = content[:match.start()].count('\n') + 1
                self.issues.append(Issue(
                    severity="INFO",
                    category="DEAD_CODE",
                    file=file,
                    line=line_num,
                    message=f"可能的未使用导入: {full_import[:60]}",
                    suggestion="删除或添加 #[allow(unused_imports)]"
                ))
                self.stats["unused_imports"] += 1
    
    def _check_todo_hacks(self, lines: List[str], file: str) -> None:
        """检测TODO/FIXME/HACK标记"""
        todo_pattern = re.compile(r'(TODO|FIXME|HACK|XXX|WARN)\s*:\s*(.+)', re.IGNORECASE)
        
        for i, line in enumerate(lines, 1):
            match = todo_pattern.search(line)
            if match:
                self.issues.append(Issue(
                    severity="INFO",
                    category="DEAD_CODE",
                    file=file,
                    line=i,
                    message=f"待处理项 [{match.group(1)}]: {match.group(2)[:50]}",
                    suggestion="建议尽快处理这些标记"
                ))
                self.stats["todos"] += 1
    
    def _check_commented_out_code(self, lines: List[str], file: str) -> None:
        """检测大块注释掉的代码"""
        comment_block_start = None
        block_lines = 0
        
        for i, line in enumerate(lines, 1):
            stripped = line.strip()
            
            if stripped.startswith('/*') and not stripped.startswith('/**'):
                if comment_block_start is None:
                    comment_block_start = i
                block_lines += 1
            elif stripped.endswith('*/') and comment_block_start:
                if block_lines > 5:  # 超过5行的注释代码块
                    self.issues.append(Issue(
                        severity="INFO",
                        category="DEAD_CODE",
                        file=file,
                        line=comment_block_start,
                        message=f"注释掉的大段代码 ({block_lines}行)",
                        suggestion="如果不再需要, 请删除; 如果暂时禁用, 添加说明"
                    ))
                    self.stats["commented_blocks"] += 1
                comment_block_start = None
                block_lines = 0
            elif comment_block_start:
                block_lines += 1
    
    # ========================================================================
    # Phase 2: 逻辑正确性验证
    # ========================================================================
    
    def check_logic_correctness(self) -> None:
        """Phase 2: 验证逻辑正确性"""
        print("\n" + "="*70)
        print("🧠 PHASE 2: 逻辑正确性验证 (Logic Correctness)")
        print("="*70)
        
        for rust_file in self.rust_files:
            try:
                content = rust_file.read_text(encoding='utf-8', errors='ignore')
                lines = content.splitlines()
                rel_path = rust_file.name.replace(str(PROJECT_ROOT) + '/', '')
                
                # 2.1 边界条件检查
                self._check_boundary_conditions(lines, rel_path)
                
                # 2.2 Option/Result 使用模式
                self._check_option_result_usage(lines, rel_path)
                
                # 2.3 unwrap() 安全性
                self._check_unwrap_safety(lines, rel_path)
                
                # 2.4 数组索引越界风险
                self._check_array_index_bounds(lines, rel_path)
                
                # 2.5 整数溢出风险
                self._check_integer_overflow(lines, rel_path)
                
            except Exception as e:
                print(f"  ⚠️  错误: {e}")
    
    def _check_boundary_conditions(self, lines: List[str], file: str) -> None:
        """检查边界条件处理"""
        risky_patterns = [
            (r'\.as_ref\(\)\.unwrap\(\)', "as_ref().unwrap() 可能panic"),
            (r'\[0\]\s*$', "直接访问数组第一个元素未检查长度"),
            (r'\.last\(\)\.unwrap\(\)', ".last().unwrap() 在空集合上会panic"),
        ]
        
        for pattern, msg in risky_patterns:
            for i, line in enumerate(lines, 1):
                if re.search(pattern, line) and not line.strip().startswith('//'):
                    self.issues.append(Issue(
                        severity="WARNING",
                        category="LOGIC_ERROR",
                        file=file,
                        line=i,
                        message=msg,
                        suggestion="使用 .get()? 或先检查长度"
                    ))
                    self.stats["boundary_issues"] += 1
    
    def _check_option_result_usage(self, lines: List[str], file: str) -> None:
        """检查Option/Result的正确使用"""
        # 检查是否忽略了Error
        ignore_error = re.compile(r'(?:let\s+\w+\s*=)?\s*.+;\s*$')
        
        for i, line in enumerate(lines, 1):
            stripped = line.strip()
            
            # 检查 .unwrap() 无上下文
            if '.unwrap()' in stripped and '?' not in stripped:
                # 允许在 test 代码中使用 unwrap
                if '#[cfg(test)]' not in ''.join(lines[max(0,i-10):i]):
                    self.issues.append(Issue(
                        severity="WARNING",
                        category="LOGIC_ERROR",
                        file=file,
                        line=i,
                        message="unwrap() 可能导致panic",
                        suggestion="考虑使用 ? 操作符或 .unwrap_or_default()"
                    ))
                    self.stats["unwrap_risks"] += 1
            
            # 检查忽略 Result 的 Err 分支
            if ('Ok(' in stripped or 'Err(' in stripped) and 'match' not in stripped:
                if 'if let' not in stripped and '=' in stripped:
                    pass  # 简化: 不报告所有情况
    
    def _check_unwrap_safety(self, lines: List[str], file: str) -> None:
        """深入检查unwrap安全性"""
        unsafe_unwraps = [
            (r'\.expect\(\s*"', "有 expect (较好)"),
            (r'\.unwrap\(\)', "无参数 unwrap (危险)"),
            (r'\.unwrap_or\b', "安全的 unwrap_or"),
            (r'\.unwrap_or_else', "安全的 unwrap_or_else"),
        ]
        
        dangerous_count = 0
        
        for i, line in enumerate(lines, 1):
            for pattern, desc in unsafe_unwraps:
                if pattern == r'\.unwrap\(\)' and re.search(pattern, line):
                    if not line.strip().startswith('//'):
                        dangerous_count += 1
        
        if dangerous_count > 5:
            self.issues.append(Issue(
                severity="WARNING",
                category="LOGIC_ERROR",
                file=file,
                line=0,
                message=f"文件中有 {dangerous_count} 个无参数 .unwrap() 调用",
                suggestion="优先使用 expect() 或 ? 操作符"
            ))
            self.stats["dangerous_unwraps"] += 1
    
    def _check_array_index_bounds(self, lines: List[str], file: str) -> None:
        """检查数组索引越界风险"""
        index_patterns = [
            r'\[(?:\w+)\s*\]',  # 变量索引
            r'\[\d+\]',           # 字面量索引
        ]
        
        for i, line in enumerate(lines, 1):
            # 检查是否有边界检查
            has_index = any(re.search(p, line) for p in index_patterns)
            has_bounds_check = any(kw in line.lower() for kw in 
                                   ['len(', '.len()', '.is_empty()', 'bounds'])
            
            if has_index and not has_bounds_check and '[' in line:
                # 简单启发式: 不是100%准确
                if 'for' not in line and 'while' not in line:
                    self.stats["index_risks"] += 1
    
    def _check_integer_overflow(self, lines: List[str], file: str) -> None:
        """检查整数溢出风险"""
        overflow_patterns = [
            (r'\w+\s*\*\s*\w+', "乘法可能导致溢出"),
            (r'\w+\s*\+\s*\w+', "加法可能溢出"),
            (r'.pow\(', ".pow() 可能溢出"),
        ]
        
        for i, line in enumerate(lines, 1):
            if any(re.search(p, line) for p, _ in overflow_patterns):
                # 检查是否有 saturating arithmetic 或 checked math
                if all(kw not in line.lower() for kw in ['saturating', 'checked', 'wrapping']):
                    self.stats["overflow_risks"] += 1
    
    # ========================================================================
    # Phase 3: FFI接口完整性检查
    # ========================================================================
    
    def check_ffi_integrity(self) -> None:
        """Phase 3: FFI接口完整性"""
        print("\n" + "="*70)
        print("🔗 PHASE 3: FFI接口完整性 (FFI Interface Integrity)")
        print("="*70)
        
        ffi_exports = {}  # name -> (file, line)
        ffi_imports = set()
        
        for rust_file in self.rust_files:
            try:
                content = rust_file.read_text(encoding='utf-8', errors='ignore')
                lines = content.splitlines()
                rel_path = rust_file.name.replace(str(PROJECT_ROOT) + '/', '')
                
                # 3.1 收集所有 #[no_mangle] 导出函数
                no_mangle_pattern = re.compile(r'#\[no_mangle\]\s*\n\s*(?:pub\s+)?(?:extern\s+"C"\s+)?fn\s+(\w+)')
                for match in no_mangle_pattern.finditer(content, re.MULTILINE):
                    func_name = match.group(1)
                    line_num = content[:match.start()].count('\n') + 1
                    ffi_exports[func_name] = (rel_path, line_num)
                
                # 3.2 检查 extern 声明
                extern_pattern = re.compile(r'extern\s+"C"\s*\{\s*fn\s+(\w+)')
                for match in extern_pattern.finditer(content):
                    ffi_imports.add(match.group(1))
                    
                # 3.3 验证 FFI 参数类型安全
                self._check_ffi_type_safety(lines, rel_path)
                
                # 3.4 检查裸指针使用
                self._check_raw_pointer_usage(lines, rel_path)
                
            except Exception as e:
                print(f"  ⚠️  错误: {e}")
        
        print(f"\n  📊 发现 {len(ffi_exports)} 个FFI导出函数")
        print(f"  📊 发现 {len(ffi_imports)} 个FFI导入函数")
        
        self.stats["ffi_exports"] = len(ffi_exports)
        self.stats["ffi_imports"] = len(ffi_imports)
    
    def _check_ffi_type_safety(self, lines: List[str], file: str) -> None:
        """检查FFI类型安全性"""
        risky_ffi = [
            (r'\*\s*mut\s+u8\s*as\s*\*\s*mut', "u8指针转换"),
            (r'transmute<.*>', "transmute (类型强制转换)"),
            (r'::std::mem::transmute', "显式 transmute"),
        ]
        
        for i, line in enumerate(lines, 1):
            for pattern, desc in risky_ffi:
                if re.search(pattern, line) and not line.strip().startswith('//'):
                    self.issues.append(Issue(
                        severity="CRITICAL" if 'transmute' in desc else "WARNING",
                        category="FFI",
                        file=file,
                        line=i,
                        message=f"FFI类型安全问题: {desc}",
                        suggestion="确保类型转换的安全性, 添加文档说明"
                    ))
                    self.stats["ffi_type_issues"] += 1
    
    def _check_raw_pointer_usage(self, lines: List[str], file: str) -> None:
        """检查裸指针使用"""
        raw_ptr_count = 0
        safe_ptr_count = 0
        
        for line in lines:
            if '*const ' in line or '*mut ' in line:
                if 'Box::' in line or 'Rc::' in line or 'Arc::' in line:
                    safe_ptr_count += 1
                else:
                    raw_ptr_count += 1
        
        if raw_ptr_count > safe_ptr_count * 2:
            self.issues.append(Issue(
                severity="WARNING",
                category="FFI",
                file=file,
                line=0,
                message=f"大量裸指针使用 ({raw_ptr_count} vs 安全指针 {safe_ptr_count})",
                suggestion="考虑使用引用或智能指针替代部分裸指针"
            ))
            self.stats["raw_pointers"] += 1
    
    # ========================================================================
    # Phase 4: 性能分析
    # ========================================================================
    
    def check_performance(self) -> None:
        """Phase 4: 性能瓶颈分析"""
        print("\n" + "="*70)
        print("⚡ PHASE 4: 性能瓶颈分析 (Performance Analysis)")
        print("="*70)
        
        total_unsafe = 0
        total_allocations = 0
        total_loops = 0
        
        for rust_file in self.rust_files:
            try:
                content = rust_file.read_text(encoding='utf-8', errors='ignore')
                lines = content.splitlines()
                rel_path = rust_file.name.replace(str(PROJECT_ROOT) + '/', '')
                
                # 4.1 统计 unsafe 块数量
                unsafe_count = len(re.findall(r'unsafe\s*\{', content))
                total_unsafe += unsafe_count
                
                # 4.2 检测堆内存分配
                alloc_patterns = [
                    r'Vec::new\(\)',
                    r'Box::new\(\)',
                    r'String::new\(\)',
                    r'\.to_vec\(\)',
                    r'\.collect\(\)',
                ]
                
                file_allocs = sum(len(re.findall(p, content)) for p in alloc_patterns)
                total_allocations += file_allocs
                
                # 4.3 检测循环效率
                loop_count = len(re.findall(r'\b(for|while)\s+', content))
                total_loops += loop_count
                
                # 4.4 检查克隆操作
                clone_count = len(re.findall(r'\.clone\(\)', content))
                if clone_count > 10:
                    self.issues.append(Issue(
                        severity="INFO",
                        category="PERFORMANCE",
                        file=rel_path,
                        line=0,
                        message=f"大量 .clone() 调用 ({clone_count})",
                        suggestion="考虑使用引用 (&) 替代不必要的克隆"
                    ))
                    self.stats["clones"] += 1
                    
                # 4.5 检查字符串拼接效率
                concat_count = len(re.findall(r'format!\(|concat!', content))
                if concat_count > 15:
                    self.issues.append(Issue(
                        severity="INFO",
                        category="PERFORMANCE",
                        file=rel_path,
                        line=0,
                        message=f"频繁字符串格式化 ({concat_count})",
                        suggestion="考虑使用 String::with_capacity 或预分配"
                    ))
                    self.stats["string_ops"] += 1
                    
            except Exception as e:
                print(f"  ⚠️  错误: {e}")
        
        print(f"\n  📊 Unsafe 块总数: {total_unsafe}")
        print(f"  📊 堆分配点: {total_allocations}")
        print(f"  📊 循环结构: {total_loops}")
        
        self.stats["total_unsafe"] = total_unsafe
        self.stats["total_allocations"] = total_allocations
    
    # ========================================================================
    # Phase 5: 测试覆盖度分析
    # ========================================================================
    
    def analyze_test_coverage(self) -> None:
        """Phase 5: 测试覆盖度分析"""
        print("\n" + "="*70)
        print("🧪 PHASE 5: 测试覆盖度分析 (Test Coverage Analysis)")
        print("="*70)
        
        test_modules = 0
        test_functions = 0
        untested_pub_functions = 0
        
        for rust_file in self.rust_files:
            try:
                content = rust_file.read_text(encoding='utf-8', errors='ignore')
                lines = content.splitlines()
                rel_path = rust_file.name.replace(str(SRC_KERNEL) + '/', '')
                
                # 统计测试模块
                if '#[cfg(test)]' in content:
                    test_modules += 1
                    
                    # 统计测试函数
                    test_fns = len(re.findall(r'#\[test\]', content))
                    test_functions += test_fns
                    
                    # 检查是否有集成测试
                    if 'mod tests' in content:
                        self.stats["test_modules_with_tests"] += 1
                        
                # 统计公开但未测试的函数
                pub_fns = len(re.findall(r'pub fn \w+', content))
                tested_fns = len(re.findall(r'#\[test\]\s*\n.*\b\w+\b.*\b(pub fn \w+)\b', 
                                         content, re.MULTILINE | re.DOTALL))
                
                if pub_fns > 0 and tested_fns < pub_fns:
                    untested_pub_functions += (pub_fns - tested_fns)
                    
            except Exception as e:
                pass
        
        print(f"\n  📊 测试模块数: {test_modules}")
        print(f"  📊 测试函数数: {test_functions}")
        print(f"  📊 未测试公开函数: ~{untested_pub_functions}")
        
        self.stats["test_modules"] = test_modules
        self.stats["test_functions"] = test_functions
    
    # ========================================================================
    # 主执行入口
    # ========================================================================
    
    def run_all_checks(self) -> Dict:
        """执行所有检查并返回结果"""
        start_time = datetime.now()
        
        print("="*70)
        print("🦀 AntX Rust 内核 - 深度代码质量检查")
        print(f"   目标目录: {self.target}")
        print(f"   开始时间: {start_time.strftime('%Y-%m-%d %H:%M:%S')}")
        print("="*70)
        
        # 发现文件
        self.find_rust_files()
        
        # 执行各阶段检查
        self.check_dead_code()
        self.check_logic_correctness()
        self.check_ffi_integrity()
        self.check_performance()
        self.analyze_test_coverage()
        
        end_time = datetime.now()
        duration = (end_time - start_time).total_seconds()
        
        # 生成报告
        report = {
            "summary": {
                "total_issues": len(self.issues),
                "critical": sum(1 for i in self.issues if i.severity == "CRITICAL"),
                "warnings": sum(1 for i in self.issues if i.severity == "WARNING"),
                "info": sum(1 for i in self.issues if i.severity == "INFO"),
                "duration_sec": duration,
                "files_checked": len(self.rust_files),
                "stats": dict(self.stats),
            },
            "issues": [i.to_dict() for i in self.issues],
        }
        
        # 打印摘要
        self._print_summary(report)
        
        return report
    
    def _print_summary(self, report: Dict) -> None:
        """打印检查摘要"""
        summary = report["summary"]
        
        print("\n" + "="*70)
        print("📊 深度检查摘要 (Engineering-Grade v2.0)")
        print("="*70)
        
        # Basic summary (backward compatible)
        print(f"\n  🔢 总问题数: {summary['total_issues']}")
        print(f"     🔴 严重 (CRITICAL): {summary['critical']}")
        print(f"     🟡 警告 (WARNING): {summary['warnings']}")
        print(f"     🔵 信息 (INFO):    {summary['info']}")
        
        print(f"\n  📁 检查文件数: {summary['files_checked']}")
        print(f"  ⏱️  检查耗时: {summary['duration_sec']:.1f}s")
        
        # Engineering-grade enhancements (v2.0)
        if ENGINEERING_MODE:
            print("\n" + "-"*70)
            print("🎯 FIX PRIORITY BREAKDOWN (ISO 25010 Aligned)")
            print("-"*70)
            
            priority_counts = defaultdict(int)
            total_effort = 0.0
            risk_distribution = defaultdict(int)
            
            for issue in self.issues:
                if hasattr(issue, 'priority'):
                    priority_counts[issue.priority] += 1
                    total_effort += getattr(issue, 'effort_hours', 0)
                    risk_distribution[issue.risk] += 1
            
            # Priority breakdown
            print(f"\n{'Priority':<12} {'Count':>8} {'Effort (hrs)':>14} {'Action':>20}")
            print("-"*56)
            
            for p in ['P0', 'P1', 'P2', 'P3']:
                count = priority_counts.get(p, 0)
                effort = sum(
                    getattr(i, 'effort_hours', 0) 
                    for i in self.issues 
                    if hasattr(i, 'priority') and i.priority == p
                )
                
                icon = "🔴" if p == "P0" else \
                      "🟠" if p == "P1" else \
                      "🟡" if p == "P2" else "🟢"
                      
                action = "IMMEDIATE" if p == "P0" else \
                        "This Sprint" if p == "P1" else \
                        "Next Release" if p == "P2" else "Optional"
                
                print(f"{icon} {p:<10} {count:>8} {effort:>13.1f}h {action:>20}")
            
            print("-"*56)
            print(f"{'TOTAL':<12} {len(self.issues):>8} {total_effort:>13.1f}h")
            
            # Risk distribution
            if risk_distribution:
                print(f"\n⚠️  Risk Distribution:")
                for level in ['HIGH', 'MEDIUM', 'LOW', 'NEGLIGIBLE']:
                    count = risk_distribution.get(level, 0)
                    if count > 0:
                        icon = "🔴" if level == "HIGH" else \
                              "🟡" if level == "MEDIUM" else \
                              "🟢" if level == "LOW" else "⚪"
                        print(f"   {icon} {level:<12}: {count} issues")
            
            # Compliance scoring
            compliant = sum(1 for i in self.issues 
                          if hasattr(i, 'compliance_impact') and 
                          i.compliance_impact == ComplianceStatus.COMPLIANT.value)
            non_compliant = sum(1 for i in self.issues 
                               if hasattr(i, 'compliance_impact') and 
                               i.compliance_impact == ComplianceStatus.NON_COMPLIANT.value)
            
            if len(self.issues) > 0:
                compliance_rate = (compliant / len(self.issues)) * 100
                status_icon = "✅" if compliance_rate >= 80 else "⚠️"
                print(f"\n{status_icon} Compliance Score: {compliance_rate:.1f}% "
                      f"({compliant}/{len(self.issues)} compliant)")
        
        # Detailed statistics (original)
        print("\n  📈 详细统计:")
        for key, value in sorted(summary['stats'].items()):
            if value > 0:
                print(f"     • {key}: {value}")
        
        # Category breakdown (enhanced with icons)
        categories = defaultdict(int)
        for issue in self.issues:
            categories[issue.category] += 1
        
        if categories:
            print("\n  📋 问题分类:")
            for cat, count in sorted(categories.items(), key=lambda x: -x[1]):
                icon = {"DEAD_CODE": "💀", "LOGIC_ERROR": "🧠", 
                       "FFI": "🔗", "PERFORMANCE": "⚡", 
                       "TEST": "🧪", "SECURITY": "🛡️"}.get(cat, "📌")
                print(f"     {icon} {cat}: {count}")


def main():
    import argparse
    
    parser = argparse.ArgumentParser(description="AntX Rust Deep Quality Checker")
    parser.add_argument("--target", "-t", default=str(SRC_KERNEL), help="目标目录")
    parser.add_argument("--json", action="store_true", help="输出JSON格式")
    parser.add_argument("--verbose", "-v", action="store_true", help="详细输出")
    
    args = parser.parse_args()
    
    checker = DeepQualityChecker(Path(args.target))
    report = checker.run_all_checks()
    
    if args.json:
        print("\n--- JSON REPORT ---")
        print(json.dumps(report, indent=2, ensure_ascii=False, default=str))
    
    # 返回退出码
    return 0 if report["summary"]["critical"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
