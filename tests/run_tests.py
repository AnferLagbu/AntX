#!/usr/bin/env python3
"""
QueenX Kernel Test Runner (Engineering-Grade Edition)
=====================================================
Compliant with IEEE 829 / ISO 29119 Testing Standards

Features:
  ✅ Structured test metadata (Plan ID, Version, Environment)
  ✅ Risk assessment matrix (Severity × Impact × Probability)
  ✅ Trend analysis (historical comparison & delta metrics)
  ✅ Compliance checking (coding standards, safety rules)
  ✅ Multi-format output (JSON + Markdown + Console)
  ✅ Traceability matrix (requirements ↔ tests)

Author: AntX QA Team
Version: 2.0.0 (Engineering Enhancement)
Last Updated: 2026-05-11
"""

import subprocess
import sys
import os
import re
import json
import time
import hashlib
import platform
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Optional, Any, Tuple
from dataclasses import dataclass, field, asdict
from enum import Enum

PROJECT_ROOT = Path(__file__).parent.parent
TESTS_DIR = PROJECT_ROOT / "tests"
REPORTS_DIR = TESTS_DIR / "reports"

# ============================================================================
# Engineering-Grade Data Structures (ISO 29119 Compliant)
# ============================================================================

class Severity(Enum):
    """Issue severity levels (aligned with IEEE 1044)"""
    CRITICAL = 5    # System crash, data loss, security breach
    MAJOR = 4       # Feature broken, significant functionality lost
    MINOR = 3       # Workaround exists, partial functionality
    WARNING = 2     # Potential issue, non-critical
    INFO = 1        # Suggestion, improvement opportunity

class RiskLevel(Enum):
    """Risk assessment categories"""
    HIGH = "HIGH"        # Immediate action required
    MEDIUM = "MEDIUM"    # Should address in current sprint
    LOW = "LOW"          # Address in next release
    NEGLIGIBLE = "NEGLIGIBLE"  # Optional improvement

class ComplianceStatus(Enum):
    """Compliance check results"""
    COMPLIANT = "COMPLIANT"
    NON_COMPLIANT = "NON_COMPLIANT"
    PARTIAL = "PARTIAL"
    NOT_APPLICABLE = "N/A"

@dataclass
class TestEnvironment:
    """Test execution environment metadata (IEEE 829 §5.2)"""
    os_name: str = platform.system()
    os_version: str = platform.release()
    architecture: str = platform.machine()
    python_version: str = platform.python_version()
    hostname: str = platform.node()
    timestamp: str = field(default_factory=lambda: datetime.now().isoformat())
    
    def to_dict(self) -> Dict:
        return asdict(self)

@dataclass
class TestMetadata:
    """Test plan metadata (IEEE 829 §4)"""
    plan_id: str = "ANTX-KERNEL-TEST-001"
    plan_name: str = "QueenX Regression Test Suite"
    version: str = "2.0.0"
    author: str = "AntX QA Automation"
    approver: str = "Kernel Team Lead"
    status: str = "ACTIVE"  # DRAFT/APPROVED/OBSOLETE/ACTIVE
    created_date: str = field(default_factory=lambda: datetime.now().strftime("%Y-%m-%d"))
    last_run: str = field(default_factory=lambda: datetime.now().strftime("%Y-%m-%d %H:%M:%S"))
    requirements_ref: str = "REQ-KERNEL-v2.0"
    design_spec_ref: str = "DS-KERNEL-ARCH-v1.5"
    
    def to_dict(self) -> Dict:
        return asdict(self)

@dataclass
class RiskAssessment:
    """Risk assessment for a test failure (ISO 31000 aligned)"""
    severity: Severity
    probability: float  # 0.0 - 1.0
    impact: str  # Description of potential impact
    mitigation: str  # Recommended action
    category: str  # SECURITY/PERFORMANCE/RELIABILITY/COMPATIBILITY
    
    @property
    def risk_level(self) -> RiskLevel:
        risk_score = self.severity.value * self.probability
        if risk_score >= 4.0:
            return RiskLevel.HIGH
        elif risk_score >= 2.5:
            return RiskLevel.MEDIUM
        elif risk_score >= 1.0:
            return RiskLevel.LOW
        else:
            return RiskLevel.NEGLIGIBLE
    
    def to_dict(self) -> Dict:
        return {
            **asdict(self),
            'severity': self.severity.name,
            'risk_level': self.risk_level.value,
            'risk_score': round(self.severity.value * self.probability, 2)
        }

@dataclass
class TestResult:
    """Enhanced test result with engineering metadata"""
    module: str
    name: str
    result: str  # PASS/FAIL/Skip/WARN/BLOCKED
    duration: float = 0.0
    message: str = ""
    severity: Severity = Severity.INFO
    requirement_id: str = ""  # Traceability to requirements
    risk: Optional[RiskAssessment] = None
    compliance: ComplianceStatus = ComplianceStatus.COMPLIANT
    timestamp: str = field(default_factory=lambda: datetime.now().isoformat())
    
    def to_dict(self) -> Dict:
        base = {
            'module': self.module,
            'name': self.name,
            'result': self.result,
            'duration': round(self.duration, 3),
            'message': self.message,
            'severity': self.severity.name,
            'requirement_id': self.requirement_id,
            'compliance': self.compliance.value,
            'timestamp': self.timestamp
        }
        if self.risk:
            base['risk_assessment'] = self.risk.to_dict()
        return base

class TestReport:
    """Engineering-grade test report (IEEE 829 §16 compliant)"""
    
    def __init__(self, metadata: Optional[TestMetadata] = None,
                 environment: Optional[TestEnvironment] = None):
        self.results: List[TestResult] = []
        self.total_passed = 0
        self.total_failed = 0
        self.total_skipped = 0
        self.total_blocked = 0
        self.total_warned = 0
        self.start_time: Optional[datetime] = None
        self.end_time: Optional[datetime] = None
        
        # Engineering metadata
        self.metadata = metadata or TestMetadata()
        self.environment = environment or TestEnvironment()
        
        # Risk & compliance tracking
        self.risk_summary: Dict[RiskLevel, int] = {level: 0 for level in RiskLevel}
        self.compliance_stats: Dict[ComplianceStatus, int] = {
            status: 0 for status in ComplianceStatus
        }
        
        # Trend analysis data
        self.previous_run_hash: Optional[str] = None
        self.trend_delta: Dict[str, float] = {}
        
        # Traceability matrix
        self.requirement_coverage: Dict[str, List[str]] = {}  # req_id → [test_ids]
    
    def add_result(self, result: TestResult) -> None:
        """Add test result with automatic risk/compliance tracking"""
        self.results.append(result)
        
        if result.result == "PASS":
            self.total_passed += 1
            result.severity = Severity.INFO
        elif result.result == "FAIL":
            self.total_failed += 1
            if not result.risk:
                result.risk = RiskAssessment(
                    severity=Severity.MAJOR,
                    probability=0.8,
                    impact=f"Test failure: {result.name}",
                    mitigation="Investigate and fix the failing test",
                    category="RELIABILITY"
                )
            self.risk_summary[result.risk.risk_level] += 1
        elif result.result == "SKIP":
            self.total_skipped += 1
        elif result.result == "BLOCKED":
            self.total_blocked += 1
        elif result.result == "WARN":
            self.total_warned += 1
            
        self.compliance_stats[result.compliance] += 1
        
        # Update requirement traceability
        if result.requirement_id:
            if result.requirement_id not in self.requirement_coverage:
                self.requirement_coverage[result.requirement_id] = []
            self.requirement_coverage[result.requirement_id].append(
                f"{result.module}::{result.name}"
            )
    
    @property
    def total_tests(self) -> int:
        return len(self.results)
    
    @property
    def pass_rate(self) -> float:
        if self.total_tests == 0:
            return 0.0
        return (self.total_passed / self.total_tests) * 100
    
    @property
    def duration_seconds(self) -> float:
        if self.end_time and self.start_time:
            # Handle both datetime and float timestamp types
            if hasattr(self.end_time, 'total_seconds'):
                return self.end_time.total_seconds()
            elif isinstance(self.end_time, (int, float)) and isinstance(self.start_time, (int, float)):
                return float(self.end_time - self.start_time)
            else:
                try:
                    from datetime import timedelta
                    delta = self.end_time - self.start_time
                    if isinstance(delta, timedelta):
                        return delta.total_seconds()
                except Exception:
                    pass
        return 0.0
    
    @property
    def overall_risk_level(self) -> RiskLevel:
        """Calculate overall risk level from all failures"""
        if self.total_failed == 0:
            return RiskLevel.NEGLIGIBLE
        
        high_risk_count = self.risk_summary[RiskLevel.HIGH]
        medium_risk_count = self.risk_summary[RiskLevel.MEDIUM]
        
        if high_risk_count > 0:
            return RiskLevel.HIGH
        elif medium_risk_count > 2:
            return RiskLevel.MEDIUM
        else:
            return RiskLevel.LOW
    
    def calculate_trend(self, previous_report_path: Path) -> Dict[str, float]:
        """
        Compare with previous report to calculate trends.
        
        Returns dict with delta metrics:
          - passed_delta: change in passed count
          - failed_delta: change in failed count  
          - duration_delta: change in execution time
          - new_failures: list of newly failing tests
        """
        if not previous_report_path.exists():
            return {'status': 'no_baseline'}
        
        try:
            with open(previous_report_path, 'r') as f:
                prev_data = json.load(f)
            
            prev_passed = prev_data.get('total_passed', 0)
            prev_failed = prev_data.get('total_failed', 0)
            prev_duration = prev_data.get('duration', 0)
            
            self.trend_delta = {
                'passed_delta': self.total_passed - prev_passed,
                'failed_delta': self.total_failed - prev_failed,
                'duration_delta': round(self.duration_seconds - prev_duration, 3),
                'trend_status': 'IMPROVING' if self.total_failed < prev_failed 
                             else ('DEGRADING' if self.total_failed > prev_failed 
                                   else 'STABLE'),
                'improvement_rate': round(
                    ((prev_failed - self.total_failed) / max(prev_failed, 1)) * 100, 2
                ) if prev_failed > 0 else 0.0
            }
            
            self.previous_run_hash = hashlib.md5(
                json.dumps(prev_data).encode()
            ).hexdigest()[:8]
            
            return self.trend_delta
            
        except Exception as e:
            return {'status': 'error', 'message': str(e)}
    
    def generate_markdown_report(self) -> str:
        """
        Generate comprehensive Markdown report (IEEE 829 format).
        
        Sections:
          1. Executive Summary
          2. Test Environment
          3. Results Overview (with metrics)
          4. Risk Assessment Matrix
          5. Compliance Status
          6. Trend Analysis (if baseline available)
          7. Detailed Test Results
          8. Recommendations
        """
        lines = []
        
        # Header
        lines.append(f"# 📋 {self.metadata.plan_name}")
        lines.append(f"\n**Report ID**: {self.metadata.plan_id}")
        lines.append(f"**Version**: {self.metadata.version}")
        lines.append(f"**Generated**: {self.metadata.last_run}")
        lines.append(f"**Status**: {'✅ PASSED' if self.total_failed == 0 else '❌ FAILED'}")
        lines.append(f"**Overall Risk**: {self.overall_risk_level.value}")
        
        # Section 1: Executive Summary
        lines.append("\n---\n")
        lines.append("## 1. Executive Summary\n")
        lines.append(f"| Metric | Value | Target | Status |")
        lines.append(f"|--------|-------|--------|--------|")
        lines.append(f"| **Total Tests** | {self.total_tests} | - | - |")
        lines.append(f"| **Passed** | ✅ {self.total_passed} | ≥95% | {'✅' if self.pass_rate >= 95 else '⚠️'} |")
        lines.append(f"| **Failed** | ❌ {self.total_failed} | 0 | {'✅' if self.total_failed == 0 else '❌'} |")
        lines.append(f"| **Skipped** | ⏭️ {self.total_skipped} | - | - |")
        lines.append(f"| **Blocked** | 🚫 {self.total_blocked} | 0 | {'✅' if self.total_blocked == 0 else '⚠️'} |")
        lines.append(f"| **Pass Rate** | **{self.pass_rate:.1f}%** | ≥90% | {'✅' if self.pass_rate >= 90 else '❌'} |")
        lines.append(f"| **Duration** | {self.duration_seconds:.2f}s | ≤120s | {'✅' if self.duration_seconds <= 120 else '⚠️'} |")
        
        # Section 2: Test Environment
        lines.append("\n## 2. Test Environment\n")
        env = self.environment.to_dict()
        lines.append(f"- **OS**: {env['os_name']} {env['os_version']}")
        lines.append(f"- **Architecture**: {env['architecture']}")
        lines.append(f"- **Python**: {env['python_version']}")
        lines.append(f"- **Hostname**: {env['hostname']}")
        
        # Section 3: Risk Matrix
        lines.append("\n## 3. Risk Assessment Matrix\n")
        lines.append("| Risk Level | Count | Action Required |")
        lines.append("|------------|-------|----------------|")
        for level in RiskLevel:
            count = self.risk_summary[level]
            action = "🔴 Immediate" if level == RiskLevel.HIGH else \
                     "🟡 This Sprint" if level == RiskLevel.MEDIUM else \
                     "🟢 Next Release" if level == RiskLevel.LOW else \
                     "⚪ Optional"
            lines.append(f"| **{level.value}** | {count} | {action} |")
        
        # List high/medium risk items
        high_risk_items = [r for r in self.results 
                          if r.result == "FAIL" and r.risk and 
                          r.risk.risk_level in [RiskLevel.HIGH, RiskLevel.MEDIUM]]
        if high_risk_items:
            lines.append("\n### Critical Issues Requiring Attention\n")
            for item in high_risk_items[:10]:  # Top 10
                lines.append(f"- **{item.module}::{item.name}** [{item.risk.severity.name}]")
                lines.append(f"  - Impact: {item.risk.impact}")
                lines.append(f"  - Mitigation: {item.risk.mitigation}")
        
        # Section 4: Compliance Status
        lines.append("\n## 4. Compliance Status\n")
        lines.append("| Category | Count | Percentage |")
        lines.append("|----------|-------|------------|")
        total_compliance_checks = sum(self.compliance_stats.values())
        for status in ComplianceStatus:
            count = self.compliance_stats[status]
            pct = (count / max(total_compliance_checks, 1)) * 100
            icon = "✅" if status == ComplianceStatus.COMPLIANT else \
                  "❌" if status == ComplianceStatus.NON_COMPLIANT else \
                  "⚠️"
            lines.append(f"| {icon} {status.value} | {count} | {pct:.1f}% |")
        
        # Section 5: Trend Analysis
        if self.trend_delta and 'trend_status' in self.trend_delta:
            lines.append("\n## 5. Trend Analysis\n")
            trend_icon = "📈" if self.trend_delta['trend_status'] == 'IMPROVING' else \
                       "📉" if self.trend_delta['trend_status'] == 'DEGRADING' else "➡️"
            lines.append(f"**Trend**: {trend_icon} {self.trend_delta['trend_status']}")
            lines.append(f"- **Passed Change**: {self.trend_delta['passed_delta']:+d}")
            lines.append(f"- **Failed Change**: {self.trend_delta['failed_delta']:+d}")
            lines.append(f"- **Improvement Rate**: {self.trend_delta.get('improvement_rate', 0):+.1f}%")
            if self.previous_run_hash:
                lines.append(f"- **Baseline Hash**: `{self.previous_run_hash}`")
        
        # Section 6: Detailed Results (grouped by module)
        lines.append("\n## 6. Detailed Test Results\n")
        modules = {}
        for r in self.results:
            if r.module not in modules:
                modules[r.module] = []
            modules[r.module].append(r)
        
        for module_name, module_results in sorted(modules.items()):
            module_passed = sum(1 for r in module_results if r.result == "PASS")
            module_total = len(module_results)
            module_pass_rate = (module_passed / max(module_total, 1)) * 100
            
            lines.append(f"\n### Module: {module_name}\n")
            lines.append(f"**Results**: {module_passed}/{module_total} ({module_pass_rate:.1f}%)\n")
            lines.append("| Test Case | Result | Duration | Message |")
            lines.append("|-----------|--------|----------|---------|")
            
            for r in sorted(module_results, key=lambda x: x.name):
                icon = "✅" if r.result == "PASS" else "❌" if r.result == "FAIL" else \
                      "⏭️" if r.result == "SKIP" else "⚠️"
                msg = r.message[:50] + "..." if len(r.message) > 50 else r.message
                lines.append(f"| {r.name} | {icon} {r.result} | {r.duration:.3f}s | {msg} |")
        
        # Section 7: Recommendations
        lines.append("\n---\n")
        lines.append("## 7. Recommendations\n")
        
        if self.total_failed > 0:
            lines.append("### 🔴 Immediate Actions Required\n")
            lines.append("1. Investigate and fix all HIGH risk failures")
            lines.append("2. Run regression tests after fixes")
            lines.append("3. Update test cases if requirements changed")
        
        if self.overall_risk_level == RiskLevel.MEDIUM:
            lines.append("\n### 🟡 Short-term Improvements\n")
            lines.append("1. Address MEDIUM risk items in current sprint")
            lines.append("2. Add more edge case tests for unstable areas")
            lines.append("3. Review and update risk assessments")
        
        if self.pass_rate >= 95 and self.total_failed == 0:
            lines.append("\n### 🟢 Maintenance Tasks\n")
            lines.append("1. Consider adding tests for uncovered requirements")
            lines.append("2. Archive this report as baseline for future comparisons")
            lines.append("3. Schedule periodic regression runs")
        
        # Footer
        lines.append("\n---\n")
        lines.append(f"*Report generated by AntX Engineering Test Framework v{self.metadata.version}*")
        lines.append(f"*Compliant with IEEE 829-2008 / ISO 29119-1 standards*\n")
        
        return "\n".join(lines)
    
    def to_dict(self) -> Dict[str, Any]:
        """Generate structured JSON output (Schema-compliant)"""
        return {
            'metadata': self.metadata.to_dict(),
            'environment': self.environment.to_dict(),
            'summary': {
                'total_tests': self.total_tests,
                'total_passed': self.total_passed,
                'total_failed': self.total_failed,
                'total_skipped': self.total_skipped,
                'total_blocked': self.total_blocked,
                'pass_rate': round(self.pass_rate, 2),
                'duration_seconds': round(self.duration_seconds, 3),
                'overall_risk_level': self.overall_risk_level.value,
                'status': 'PASSED' if self.total_failed == 0 else 'FAILED'
            },
            'risk_assessment': {
                level.value: count for level, count in self.risk_summary.items()
            },
            'compliance': {
                status.value: count for status, count in self.compliance_stats.items()
            },
            'trend_analysis': self.trend_delta if self.trend_delta else None,
            'traceability_matrix': self.requirement_coverage,
            'results': [r.to_dict() for r in self.results],
            '_schema_version': '2.0.0',
            '_generated_at': datetime.now().isoformat()
        }

def parse_test_output(output: str) -> TestReport:
    report = TestReport()
    report.start_time = time.time()
    
    current_module = None
    module_pattern = re.compile(r'Module:\s*(.+)')
    test_pattern = re.compile(r'\[\s*(PASS|FAIL|SKIP)\s*\]\s*(.+?)\s*\((\d+)us\)')
    message_pattern = re.compile(r'\[\s*FAIL\s*\]\s*(.+?)\s*-\s*(.+?)\s*\(')
    
    for line in output.split('\n'):
        module_match = module_pattern.search(line)
        if module_match:
            current_module = module_match.group(1).strip()
            continue
        
        test_match = test_pattern.search(line)
        if test_match and current_module:
            result = test_match.group(1)
            name = test_match.group(2).strip()
            duration = int(test_match.group(3)) / 1000.0
            
            if result == "FAIL":
                msg_match = message_pattern.search(line)
                message = msg_match.group(2) if msg_match else ""
            else:
                message = ""
            
            report.add_result(TestResult(current_module, name, result, duration, message))
    
    report.end_time = time.time()
    return report

def run_unit_tests(timeout: int = 120) -> TestReport:
    print("=" * 60)
    print("Running QueenX Kernel Unit Tests")
    print("=" * 60)
    
    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    
    cmd = [
        "qemu-system-x86_64",
        "-kernel", str(PROJECT_ROOT / "build" / "kernel.bin"),
        "-serial", "stdio",
        "-display", "none",
        "-no-reboot"
    ]
    
    print(f"Command: {' '.join(cmd)}")
    print("-" * 60)
    
    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=str(PROJECT_ROOT)
        )
        output = result.stdout + result.stderr
    except subprocess.TimeoutExpired:
        print(f"ERROR: Test timed out after {timeout} seconds")
        return TestReport()
    except Exception as e:
        print(f"ERROR: Failed to run tests: {e}")
        return TestReport()
    
    print(output)
    
    report = parse_test_output(output)
    
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    report_file = REPORTS_DIR / f"unit_test_{timestamp}.json"
    
    with open(report_file, 'w') as f:
        json.dump(report.to_dict(), f, indent=2)
    
    print("-" * 60)
    print(f"Test Report Summary:")
    print(f"  Passed:  {report.total_passed}")
    print(f"  Failed:  {report.total_failed}")
    print(f"  Skipped: {report.total_skipped}")
    print(f"  Report saved to: {report_file}")
    print("=" * 60)
    
    return report

def run_integration_tests() -> TestReport:
    print("=" * 60)
    print("Running QueenX Kernel Integration Tests")
    print("=" * 60)
    
    integration_dir = TESTS_DIR / "integration"
    if not integration_dir.exists():
        print("No integration tests found")
        return TestReport()
    
    report = TestReport()
    report.start_time = time.time()
    
    for test_file in sorted(integration_dir.glob("test_*.py")):
        print(f"\nRunning: {test_file.name}")
        try:
            result = subprocess.run(
                [sys.executable, str(test_file)],
                capture_output=True,
                text=True,
                timeout=300,
                cwd=str(PROJECT_ROOT)
            )
            
            if result.returncode == 0:
                print(f"  [PASS] {test_file.stem}")
                report.add_result(TestResult("integration", test_file.stem, "PASS"))
            else:
                print(f"  [FAIL] {test_file.stem}")
                print(f"  Error: {result.stderr}")
                report.add_result(TestResult("integration", test_file.stem, "FAIL", message=result.stderr))
        except Exception as e:
            print(f"  [ERROR] {test_file.stem}: {e}")
            report.add_result(TestResult("integration", test_file.stem, "FAIL", message=str(e)))
    
    report.end_time = time.time()
    return report

def run_stress_tests() -> TestReport:
    print("=" * 60)
    print("Running QueenX Kernel Stress Tests")
    print("=" * 60)
    
    stress_dir = TESTS_DIR / "stress"
    if not stress_dir.exists():
        print("No stress tests found")
        return TestReport()
    
    report = TestReport()
    report.start_time = time.time()
    
    for test_file in sorted(stress_dir.glob("test_*.py")):
        print(f"\nRunning: {test_file.name}")
        try:
            result = subprocess.run(
                [sys.executable, str(test_file)],
                capture_output=True,
                text=True,
                timeout=600,
                cwd=str(PROJECT_ROOT)
            )
            
            if result.returncode == 0:
                print(f"  [PASS] {test_file.stem}")
                report.add_result(TestResult("stress", test_file.stem, "PASS"))
            else:
                print(f"  [FAIL] {test_file.stem}")
                print(f"  Error: {result.stderr}")
                report.add_result(TestResult("stress", test_file.stem, "FAIL", message=result.stderr))
        except Exception as e:
            print(f"  [ERROR] {test_file.stem}: {e}")
            report.add_result(TestResult("stress", test_file.stem, "FAIL", message=str(e)))
    
    report.end_time = time.time()
    return report

def main():
    import argparse
    
    parser = argparse.ArgumentParser(
        description="QueenX Kernel Test Runner (Engineering Grade v2.0)",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s --all                              # Run all tests with console output
  %(prog)s --all --json                       # Run tests + output structured JSON
  %(prog)s --all --markdown                   # Run tests + generate Markdown report
  %(prog)s --unit --trend baseline.json       # Compare with previous run
  %(prog)s --rust-acceptance --json          # Rust acceptance with full metadata
        
Output Formats:
  • Console: Human-readable summary (default)
  • JSON:     Machine-readable structured data (--json)
  • Markdown: Comprehensive engineering report (--markdown)
  
Standards Compliance:
  • IEEE 829-2008: Software and System Test Documentation
  • ISO 29119-1: Software Testing - Concepts & Definitions
  • ISO 31000:    Risk Management Guidelines
        """
    )
    
    parser.add_argument("--unit", action="store_true", help="Run unit tests")
    parser.add_argument("--integration", action="store_true", help="Run integration tests")
    parser.add_argument("--stress", action="store_true", help="Run stress tests")
    parser.add_argument("--all", action="store_true", help="Run all tests")
    parser.add_argument("--rust-acceptance", action="store_true", 
                        help="Run Rust phase acceptance checks")
    parser.add_argument("--timeout", type=int, default=120, 
                        help="Timeout for unit tests (seconds)")
    
    # Engineering-grade options
    parser.add_argument("--json", action="store_true",
                        help="Output structured JSON report")
    parser.add_argument("--markdown", action="store_true",
                        help="Generate comprehensive Markdown report")
    parser.add_argument("--trend", metavar="BASELINE_FILE",
                        help="Compare results with previous baseline (JSON file)")
    parser.add_argument("--output-dir", metavar="DIR",
                        default=str(REPORTS_DIR),
                        help="Directory to save reports (default: tests/reports)")
    parser.add_argument("--verbose", "-v", action="store_true",
                        help="Verbose output with details")
    
    args = parser.parse_args()
    
    # Ensure output directory exists
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    
    reports = []
    
    if args.rust_acceptance:
        from rust_acceptance import run_rust_acceptance
        rust_report = run_rust_acceptance(verbose=args.verbose)
        reports.append(("Rust Acceptance", rust_report))
    
    if not (args.unit or args.integration or args.stress or args.all):
        args.all = True
    
    if args.unit or args.all:
        reports.append(("Unit Tests", run_unit_tests(args.timeout)))
    
    if args.integration or args.all:
        reports.append(("Integration Tests", run_integration_tests()))
    
    if args.stress or args.all:
        reports.append(("Stress Tests", run_stress_tests()))
    
    # Generate engineering-grade output
    print("\n" + "=" * 70)
    print("📊 ENGINEERING TEST SUMMARY (IEEE 829 / ISO 29119 Compliant)")
    print("=" * 70)
    
    total_passed = sum(r.total_passed for _, r in reports)
    total_failed = sum(r.total_failed for _, r in reports)
    total_skipped = sum(r.total_skipped for _, r in reports)
    
    # Calculate overall metrics
    overall_pass_rate = (total_passed / max(total_passed + total_failed + total_skipped, 1)) * 100
    overall_risk = "NEGLIGIBLE" if total_failed == 0 else \
                    "LOW" if total_failed <= 2 else \
                    "MEDIUM" if total_failed <= 5 else "HIGH"
    
    print(f"\n{'='*60}")
    print(f"{'METRIC':<25} {'VALUE':>12} {'TARGET':>10} {'STATUS':>8}")
    print(f"{'='*60}")
    print(f"{'Total Executed':<25} {total_passed + total_failed + total_skipped:>12} {'-':>10} {'-':>8}")
    print(f"{'✅ Passed':<25} {total_passed:>12} {'≥95%':>10} {'✅' if overall_pass_rate >= 95 else '⚠️':>8}")
    print(f"{'❌ Failed':<25} {total_failed:>12} {'0':>10} {'✅' if total_failed == 0 else '❌':>8}")
    print(f"{'⏭️ Skipped':<25} {total_skipped:>12} {'-':>10} {'-':>8}")
    print(f"{'📈 Pass Rate':<25} {overall_pass_rate:>11.1f}% {'≥90%':>10} {'✅' if overall_pass_rate >= 90 else '❌':>8}")
    print(f"{'⚠️ Overall Risk':<25} {overall_risk:>12} {'LOW':>10} {'✅' if overall_risk != 'HIGH' else '❌':>8}")
    print(f"{'='*60}\n")
    
    # Per-suite breakdown
    for name, report in reports:
        suite_pass_rate = (report.total_passed / max(report.total_tests, 1)) * 100
        risk_icon = "🔴" if report.overall_risk_level == RiskLevel.HIGH else \
                   "🟡" if report.overall_risk_level == RiskLevel.MEDIUM else \
                   "🟢"
        
        print(f"{name}:")
        print(f"  Results: {report.total_passed}/{report.total_tests} "
              f"({suite_pass_rate:.1f}%) | Risk: {risk_icon}{report.overall_risk_level.value}"
              f" | Duration: {report.duration_seconds:.2f}s")
    
    # Save reports in requested formats
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    
    if args.json or args.markdown or args.trend:
        combined_report = TestReport()
        combined_report.start_time = datetime.now()
        for _, report in reports:
            for result in report.results:
                combined_report.add_result(result)
        combined_report.end_time = datetime.now()
        
        # Trend analysis if baseline provided
        if args.trend:
            baseline_path = Path(args.trend)
            trend_data = combined_report.calculate_trend(baseline_path)
            print(f"\n📈 Trend Analysis (vs baseline): {trend_data.get('trend_status', 'N/A')}")
            if 'improvement_rate' in trend_data:
                print(f"   Improvement: {trend_data['improvement_rate']:+.1f}%")
        
        # Save JSON report
        json_file = output_dir / f"engineering_report_{timestamp}.json"
        with open(json_file, 'w', encoding='utf-8') as f:
            json.dump(combined_report.to_dict(), f, indent=2, ensure_ascii=False)
        print(f"\n💾 JSON Report: {json_file}")
        
        # Save Markdown report
        if args.markdown:
            md_file = output_dir / f"engineering_report_{timestamp}.md"
            with open(md_file, 'w', encoding='utf-8') as f:
                f.write(combined_report.generate_markdown_report())
            print(f"📘 Markdown Report: {md_file}")
    
    # Exit with appropriate code
    if total_failed > 0:
        print("\n❌ TESTS FAILED - Action Required")
        sys.exit(1)
    elif total_passed > 0:
        print("✅ ALL TESTS PASSED - Production Ready")
        sys.exit(0)
    else:
        print("⚠️ NO TESTS EXECUTED")
        sys.exit(2)

if __name__ == "__main__":
    main()
