#!/usr/bin/env python3
"""
AntX 综合测试运行器 (Comprehensive Test Runner)
运行所有类型的测试：单元测试、集成测试、硬件测试、性能测试、压力测试
"""

import subprocess
import sys
import os
import time
from pathlib import Path
from datetime import datetime
from dataclasses import dataclass, field
from typing import List

PROJECT_ROOT = Path(__file__).parent.parent
REPORTS_DIR = PROJECT_ROOT / "tests" / "reports"

@dataclass
class TestSuiteResult:
    name: str
    passed: bool
    duration: float
    output: str
    details: List[str] = field(default_factory=list)

def run_command(cmd: List[str], cwd: str = None, timeout: int = 300) -> tuple:
    """运行命令并返回结果"""
    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=cwd or str(PROJECT_ROOT)
        )
        return result.returncode, result.stdout + result.stderr
    except subprocess.TimeoutExpired:
        return -1, "Timeout"
    except Exception as e:
        return -1, str(e)

def run_unit_tests() -> TestSuiteResult:
    """运行单元测试"""
    print("  📦 运行单元测试...")
    start = time.time()
    
    code, output = run_command(
        ["cargo", "test", "--lib"],
        cwd=str(PROJECT_ROOT / "host-tests"),
        timeout=120
    )
    
    duration = time.time() - start
    passed = code == 0
    
    result = TestSuiteResult(
        name="单元测试",
        passed=passed,
        duration=duration,
        output=output
    )
    
    if passed:
        result.details.append("✅ 所有单元测试通过")
    else:
        result.details.append("❌ 部分单元测试失败")
    
    return result

def run_integration_tests() -> TestSuiteResult:
    """运行集成测试"""
    print("  🔗 运行集成测试...")
    start = time.time()
    
    code, output = run_command(
        ["python3", "tests/integration/run_driver_integration_tests.py"],
        timeout=120
    )
    
    duration = time.time() - start
    passed = code == 0
    
    result = TestSuiteResult(
        name="集成测试",
        passed=passed,
        duration=duration,
        output=output
    )
    
    if passed:
        result.details.append("✅ 所有集成测试通过")
    else:
        result.details.append("❌ 部分集成测试失败")
    
    return result

def run_hardware_tests() -> TestSuiteResult:
    """运行硬件测试"""
    print("  🖥️  运行硬件测试...")
    start = time.time()
    
    code, output = run_command(
        ["python3", "tests/hardware/run_qemu_hardware_tests.py"],
        timeout=60
    )
    
    duration = time.time() - start
    passed = code == 0
    
    result = TestSuiteResult(
        name="硬件测试",
        passed=passed,
        duration=duration,
        output=output
    )
    
    if passed:
        result.details.append("✅ 所有硬件测试通过")
    else:
        result.details.append("❌ 部分硬件测试失败")
    
    return result

def run_benchmark_tests() -> TestSuiteResult:
    """运行性能测试"""
    print("  ⏱️  运行性能测试...")
    start = time.time()
    
    code, output = run_command(
        ["python3", "tests/benchmark/run_benchmarks.py"],
        timeout=120
    )
    
    duration = time.time() - start
    passed = code == 0
    
    result = TestSuiteResult(
        name="性能测试",
        passed=passed,
        duration=duration,
        output=output
    )
    
    if passed:
        result.details.append("✅ 性能测试完成")
    else:
        result.details.append("❌ 性能测试失败")
    
    return result

def run_stress_tests() -> TestSuiteResult:
    """运行压力测试"""
    print("  🔥 运行压力测试...")
    start = time.time()
    
    code, output = run_command(
        ["python3", "tests/stress/run_stress_tests_enhanced.py"],
        timeout=120
    )
    
    duration = time.time() - start
    passed = code == 0
    
    result = TestSuiteResult(
        name="压力测试",
        passed=passed,
        duration=duration,
        output=output
    )
    
    if passed:
        result.details.append("✅ 压力测试通过")
    else:
        result.details.append("⚠️ 压力测试发现问题")
    
    return result

def print_header(title: str):
    print(f"\n{'='*70}")
    print(f"  {title}")
    print(f"{'='*70}\n")

def print_result(result: TestSuiteResult):
    status = "✅" if result.passed else "❌"
    print(f"\n  {status} {result.name}")
    print(f"     持续时间: {result.duration:.2f}s")
    for detail in result.details:
        print(f"     {detail}")

def generate_report(results: List[TestSuiteResult]) -> str:
    """生成测试报告"""
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    
    report = f"""# AntX 综合测试报告

**测试时间**: {datetime.now().strftime("%Y-%m-%d %H:%M:%S")}  
**测试类型**: 综合测试

---

## 测试概览

| 测试类型 | 状态 | 持续时间 |
|---------|------|---------|
"""
    
    for r in results:
        status = "✅ 通过" if r.passed else "❌ 失败"
        report += f"| {r.name} | {status} | {r.duration:.2f}s |\n"
    
    report += f"""
---

## 详细结果

"""
    
    for r in results:
        status = "✅" if r.passed else "❌"
        report += f"### {status} {r.name}\n\n"
        report += f"- **持续时间**: {r.duration:.2f}s\n"
        for detail in r.details:
            report += f"- {detail}\n"
        report += "\n"
    
    total_passed = sum(1 for r in results if r.passed)
    total = len(results)
    
    report += f"""---

## 总结

- **通过**: {total_passed}/{total}
- **失败**: {total - total_passed}/{total}
- **成功率**: {(total_passed / total * 100):.1f}%

"""
    
    if total_passed == total:
        report += "✅ **所有测试通过！**\n"
    else:
        report += f"⚠️ **{total - total_passed} 个测试失败**\n"
    
    return report

def main():
    print_header("AntX 综合测试运行器")
    
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    
    print("🚀 开始运行所有测试...\n")
    
    # 运行所有测试
    results = []
    
    results.append(run_unit_tests())
    results.append(run_integration_tests())
    results.append(run_hardware_tests())
    results.append(run_benchmark_tests())
    results.append(run_stress_tests())
    
    # 打印结果
    print("\n📊 测试结果:")
    for result in results:
        print_result(result)
    
    # 生成报告
    report = generate_report(results)
    report_file = REPORTS_DIR / f"comprehensive_test_{timestamp}.md"
    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    
    with open(report_file, 'w') as f:
        f.write(report)
    
    print(f"\n📁 测试报告已保存至: {report_file}")
    
    # 总结
    print(f"\n{'='*70}\n")
    total_passed = sum(1 for r in results if r.passed)
    total = len(results)
    
    print(f"  📈 总结: {total_passed}/{total} 测试套件通过")
    
    if total_passed == total:
        print(f"  ✅ 所有测试通过!")
        return 0
    else:
        print(f"  ❌ {total - total_passed} 个测试套件失败")
        return 1

if __name__ == "__main__":
    sys.exit(main())
