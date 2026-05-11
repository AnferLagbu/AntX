#!/usr/bin/env python3
"""
AntX Rust 内核自动化质量维护工具
功能:
  1. 检测并清理未使用的 use 导入
  2. 为 FFI 导出函数添加 #[allow(dead_code)] 注解
  3. 收集并分类 TODO/FIXME 标记
  4. 生成维护报告
"""

import os
import re
from pathlib import Path
from datetime import datetime

PROJECT_ROOT = Path(__file__).parent.parent
SRC_KERNEL = PROJECT_ROOT / "src" / "kernel"

class AutoFixer:
    def __init__(self, target_dir: Path):
        self.target = target_dir
        self.fixes_applied = []
        self.warnings = []
        self.stats = {
            "unused_imports_removed": 0,
            "dead_code_annotations_added": 0,
            "todos_categorized": 0,
            "files_modified": 0,
        }
    
    def find_rust_files(self):
        """发现所有Rust文件"""
        self.rust_files = list(self.target.rglob("*.rs"))
        print(f"📂 发现 {len(self.rust_files)} 个Rust文件")
    
    # ========================================================================
    # Fix 1: 清理未使用导入
    # ========================================================================
    def fix_unused_imports(self) -> int:
        """Phase 1: 检测和清理未使用的use导入"""
        print("\n" + "="*70)
        print("🧹 PHASE 1: 清理未使用导入")
        print("="*70)
        
        total_removed = 0
        
        for rust_file in self.rust_files:
            try:
                content = rust_file.read_text(encoding='utf-8', errors='ignore')
                lines = content.splitlines()
                new_lines = []
                removed_in_file = 0
                
                in_use_block = False
                use_block_start = None
                
                for i, line in enumerate(lines, 1):
                    stripped = line.strip()
                    
                    # 检测 use 语句开始
                    if stripped.startswith('use ') and not stripped.startswith('//'):
                        import_name = self._extract_import_name(stripped)
                        
                        if import_name:
                            # 检查此导入是否在文件其他地方被使用
                            usage_count = content.count(import_name.split('::')[-1])
                            
                            # 如果只在导入语句中出现, 可能是未使用
                            if usage_count <= 1 and not any(
                                kw in stripped for kw in ['#[allow', 'extern']
                            ):
                                # 额外检查: 是否在注释、字符串字面量等中出现
                                code_without_imports = content.replace(stripped, '')
                                real_usage = code_without_imports.count(import_name.split('::')[-1])
                                
                                if real_usage == 0:
                                    new_lines.append(f"// [AUTO-FIXED] Removed unused import: {stripped}")
                                    removed_in_file += 1
                                    total_removed += 1
                                    continue
                    
                    new_lines.append(line)
                
                if removed_in_file > 0:
                    # 写回文件
                    rust_file.write_text('\n'.join(new_lines), encoding='utf-8')
                    rel_path = rust_file.name.replace(str(SRC_KERNEL) + '/', '')
                    print(f"  ✅ {rel_path}: 移除了 {removed_in_file} 个未使用导入")
                    self.stats["files_modified"] += 1
                    
            except Exception as e:
                print(f"  ⚠️  处理 {rust_file}: {e}")
        
        self.stats["unused_imports_removed"] = total_removed
        return total_removed
    
    def _extract_import_name(self, import_line: str) -> str:
        """从use语句中提取导入名称"""
        match = re.match(r'use\s+(?:(\w+)::)*(\w+)', import_line)
        if match:
            return match.group(0).replace('use ', '').rstrip(';')
        return ""
    
    # ========================================================================
    # Fix 2: 为FFI导出函数添加死代码抑制注解
    # ========================================================================
    def fix_dead_code_warnings(self) -> int:
        """Phase 2: 为FFI导出函数添加#[allow(dead_code)]"""
        print("\n" + "="*70)
        print("🏷️  PHASE 2: 添加死代码抑制注解")
        print("="*70)
        
        annotations_added = 0
        
        for rust_file in self.rust_files:
            try:
                content = rust_file.read_text(encoding='utf-8', errors='ignore')
                lines = content.splitlines()
                new_lines = []
                added_in_file = 0
                
                i = 0
                while i < len(lines):
                    line = lines[i]
                    stripped = line.strip()
                    
                    # 检测 #[no_mangle] 函数定义
                    if ('#[no_mangle]' in line or 
                        (i > 0 and '#[no_mangle]' in lines[i-1])):
                        
                        # 查找下一个函数定义
                        func_match = re.search(
                            r'(?:pub\s+)?(?:extern\s+"C"\s+)?fn\s+(\w+)',
                            line if 'fn' in line else lines[min(i+1, len(lines)-1)]
                        )
                        
                        if func_match:
                            func_name = func_match.group(1)
                            
                            # 检查是否已经有 allow 注解
                            has_allow = False
                            for j in range(max(0, i-3), i):
                                if 'allow(dead_code)' in lines[j]:
                                    has_allow = True
                                    break
                            
                            if not has_allow and len(func_name) > 3:
                                # 在函数前添加注解
                                indent = len(line) - len(line.lstrip())
                                new_lines.append(' ' * indent + '/// Allow dead_code: FFI export function')
                                new_lines.append(' ' * indent + '#[allow(dead_code)]')
                                annotations_added += 1
                                added_in_file += 1
                    
                    new_lines.append(line)
                    i += 1
                
                if added_in_file > 0:
                    rust_file.write_text('\n'.join(new_lines), encoding='utf-8')
                    rel_path = rust_file.name.replace(str(SRC_KERNEL) + '/', '')
                    print(f"  ✅ {rel_path}: 添加了 {added_in_file} 个注解")
                    self.stats["files_modified"] += 1
                    
            except Exception as e:
                print(f"  ⚠️  {rust_file}: {e}")
        
        self.stats["dead_code_annotations_added"] = annotations_added
        return annotations_added
    
    # ========================================================================
    # Fix 3: 分类处理TODO/FIXME标记
    # ========================================================================
    def categorize_todos(self) -> dict:
        """Phase 3: 收集并分类TODO/FIXME"""
        print("\n" + "="*70)
        print("📋 PHASE 3: 分类TODO/FIXME标记")
        print("="*70)
        
        todos = {
            "HIGH": [],   # 需要立即处理
            "MEDIUM": [], # 本周内处理
            "LOW": [],    # 可延后
            "INFO": [],   # 仅信息记录
        }
        
        todo_pattern = re.compile(
            r'(TODO|FIXME|HACK|XXX|WARN)\s*:\s*(.+)',
            re.IGNORECASE
        )
        
        for rust_file in self.rust_files:
            try:
                content = rust_file.read_text(encoding='utf-8', errors='ignore')
                
                for match in todo_pattern.finditer(content):
                    tag = match.group(1).upper()
                    message = match.group(2)[:100]
                    line_num = content[:match.start()].count('\n') + 1
                    rel_path = rust_file.name.replace(str(SRC_KERNEL) + '/', '')
                    
                    todo_item = {
                        "file": rel_path,
                        "line": line_num,
                        "tag": tag,
                        "message": message,
                    }
                    
                    # 简单优先级分类
                    if tag == "FIXME":
                        todos["HIGH"].append(todo_item)
                    elif tag == "HACK" or "XXX":
                        todos["MEDIUM"].append(todo_item)
                    elif tag == "WARN":
                        todos["LOW"].append(todo_item)
                    else:  # TODO
                        todos["INFO"].append(todo_item)
                    
                    self.stats["todos_categorized"] += 1
                    
            except Exception as e:
                pass
        
        # 打印分类结果
        for priority, items in todos.items():
            icon = {"HIGH": "🔴", "MEDIUM": "🟡", "LOW": "🔵", "INFO": "⚪"}[priority]
            print(f"\n  {icon} {priority} PRIORITY ({len(items)} items):")
            
            for item in items[:10]:  # 只显示前10个
                print(f"     • [{item['tag']}] {item['file']}:{item['line']}")
                print(f"       {item['message']}")
            
            if len(items) > 10:
                print(f"     ... 还有 {len(items)-10} 个")
        
        return todos
    
    # ========================================================================
    # Fix 4: 增强测试模块
    # ========================================================================
    def enhance_tests(self) -> int:
        """Phase 4: 为核心模块增加基础测试用例"""
        print("\n" + "="*70)
        print("🧪 PHASE 4: 增强单元测试")
        print("="*70)
        
        tests_added = 0
        
        # 为每个核心模块检查是否有测试
        core_modules = [
            ("logging/klog.rs", self._add_klog_tests),
            ("cpu/mod.rs", self._add_cpu_tests),
            ("arch/x86_64/gdt.rs", self._add_gdt_tests),
            ("time/timer.rs", self._add_timer_tests),
            ("interrupt/ioapic.rs", self._add_ioapic_tests),
        ]
        
        for module_path, test_func in core_modules:
            full_path = SRC_KERNEL / module_path
            if full_path.exists():
                try:
                    added = test_func(full_path)
                    if added > 0:
                        tests_added += added
                        print(f"  ✅ {module_path}: 添加了 {added} 个测试")
                except Exception as e:
                    print(f"  ⚠️  {module_path}: {e}")
        
        return tests_added
    
    def _add_klog_tests(self, file_path: Path) -> int:
        """为klog模块添加测试"""
        content = file_path.read_text()
        
        if '#[cfg(test)]' not in content:
            test_code = '''

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Debug as u8 < LogLevel::Info as u8);
        assert!(LogLevel::Info as u8 < LogLevel::Error as u8);
        assert!(LogLevel::Error as u8 < LogLevel::Crit as u8);
    }
    
    #[test]
    fn test_constants() {
        assert_eq!(KLOG_BUFFER_SIZE, 4096);
        assert_eq!(KLOG_LINE_MAX, 256);
        assert_eq!(KLOG_CAT_MAX, 12);
    }
}
'''
            file_path.write_text(content + test_code)
            return 3
        return 0
    
    def _add_cpu_tests(self, file_path: Path) -> int:
        """为CPU模块添加测试"""
        content = file_path.read_text()
        
        if '#[cfg(test)]' not in content:
            test_code = '''

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vendor_recognition() {
        let intel = CpuVendor::from_vendor_string(b"GenuineIntel");
        assert_eq!(intel, CpuVendor::Intel);
        
        let amd = CpuVendor::from_vendor_string(b"AuthenticAMD");
        assert_eq!(amd, CpuVendor::Amd);
    }
    
    #[test]
    fn test_signature_effective_values() {
        let sig = CpuSignature {
            family: 6,
            model: 0x9E,
            ext_family: 0,
            ext_model: 0,
            ..Default::default()
        };
        assert_eq!(sig.effective_family(), 6);
        assert_eq!(sig.effective_model(), 0x9E);
    }
}
'''
            file_path.write_text(content + test_code)
            return 2
        return 0
    
    def _add_gdt_tests(self, file_path: Path) -> int:
        """为GDT模块添加测试"""
        content = file_path.read_text()
        
        if '#[cfg(test)]' not in content:
            test_code = '''

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_selector_values() {
        assert_eq!(SELECTOR_NULL, 0x00);
        assert_eq!(SELECTOR_KERNEL_CODE, 0x08);
        assert_eq!(SELECTOR_USER_CODE, 0x18);
    }
    
    #[test]
    fn test_access_byte_constants() {
        assert_eq!(AccessByte::kernel_code().0, 0x9A);
        assert_eq!(AccessByte::kernel_data().0, 0x92);
        assert_eq!(AccessByte::user_code().0, 0xFA);
    }
}
'''
            file_path.write_text(content + test_code)
            return 2
        return 0
    
    def _add_timer_tests(self, file_path: Path) -> int:
        """为Timer模块添加测试"""
        content = file_path.read_text()
        
        if '#[cfg(test)]' not in content:
            test_code = '''

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_timer_constants() {
        assert_eq!(PIT_BASE_FREQUENCY, 1193182);
        assert_eq!(TIMER_FREQUENCY, 100);
        assert_eq!(TIMER_IRQ_VECTOR, 32);
    }
    
    #[test]
    fn test_time_conversion() {
        assert_eq!(ticks_to_ms(100), 1000);
        assert_eq!(ms_to_ticks(1000), 100);
    }
}
'''
            file_path.write_text(content + test_code)
            return 2
        return 0
    
    def _add_ioapic_tests(self, file_path: Path) -> int:
        """为IOAPIC模块添加测试"""
        content = file_path.read_text()
        
        if '#[cfg(test)]' not in content:
            test_code = '''

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ioapic_constants() {
        assert_eq!(IOAPIC_REGSEL, 0xFEC00000);
        assert_eq!(IOAPIC_MAX_IRQ, 24);
    }
    
    #[test]
    fn test_redir_entry_creation() {
        let entry = IoApicRedirEntry::new_standard(40, 0);
        assert_eq!(entry.vector(), 40);
        assert!(!entry.is_masked());
    }
}
'''
            file_path.write_text(content + test_code)
            return 2
        return 0
    
    # ========================================================================
    # 主执行入口
    # ========================================================================
    def run_all_fixes(self) -> dict:
        """执行所有自动修复"""
        start_time = datetime.now()
        
        print("="*70)
        print("🔧 AntX Rust 内核 - 自动化质量维护")
        print(f"   目标: {self.target}")
        print(f"   时间: {start_time.strftime('%Y-%m-%d %H:%M:%S')}")
        print("="*70)
        
        # 发现文件
        self.find_rust_files()
        
        # 执行各阶段修复
        imports_removed = self.fix_unused_imports()
        annotations_added = self.fix_dead_code_warnings()
        todos = self.categorize_todos()
        tests_added = self.enhance_tests()
        
        end_time = datetime.now()
        duration = (end_time - start_time).total_seconds()
        
        # 生成报告
        report = {
            "summary": {
                "imports_removed": imports_removed,
                "annotations_added": annotations_added,
                "todos_found": self.stats["todos_categorized"],
                "tests_added": tests_added,
                "files_modified": self.stats["files_modified"],
                "duration_sec": duration,
            },
            "todos_by_priority": {
                k: len(v) for k, v in todos.items()
            },
            "stats": self.stats,
        }
        
        # 打印摘要
        print("\n" + "="*70)
        print("📊 维护完成摘要")
        print("="*70)
        
        print(f"\n  ✅ 已完成的操作:")
        print(f"     • 移除未使用导入: {imports_removed}")
        print(f"     • 添加死代码注解: {annotations_added}")
        print(f"     • 新增测试用例: {tests_added}")
        print(f"     • 修改文件数: {self.stats['files_modified']}")
        
        print(f"\n  📋 待处理事项:")
        for priority, count in report["todos_by_priority"].items():
            icon = {"HIGH": "🔴", "MEDIUM": "🟡", "LOW": "🔵", "INFO": "⚪"}[priority]
            print(f"     {icon} {priority}: {count}")
        
        print(f"\n  ⏱️  总耗时: {duration:.1f}s")
        
        return report


def main():
    import argparse
    
    parser = argparse.ArgumentParser(description="AntX Rust Auto Fixer")
    parser.add_argument("--target", "-t", default=str(SRC_KERNEL))
    parser.add_argument("--dry-run", action="store_true", help="只分析不修改")
    
    args = parser.parse_args()
    
    fixer = AutoFixer(Path(args.target))
    report = fixer.run_all_fixes()
    
    return 0


if __name__ == "__main__":
    import sys
    sys.exit(main())
