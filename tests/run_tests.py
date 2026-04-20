#!/usr/bin/env python3
"""
QueenX Kernel Test Runner
Runs unit tests, integration tests, and stress tests for the kernel.
"""

import subprocess
import sys
import os
import re
import json
import time
from datetime import datetime
from pathlib import Path

PROJECT_ROOT = Path(__file__).parent.parent
TESTS_DIR = PROJECT_ROOT / "tests"
REPORTS_DIR = TESTS_DIR / "reports"

class TestResult:
    def __init__(self, module: str, name: str, result: str, duration: float = 0, message: str = ""):
        self.module = module
        self.name = name
        self.result = result
        self.duration = duration
        self.message = message

class TestReport:
    def __init__(self):
        self.results = []
        self.total_passed = 0
        self.total_failed = 0
        self.total_skipped = 0
        self.start_time = None
        self.end_time = None
    
    def add_result(self, result: TestResult):
        self.results.append(result)
        if result.result == "PASS":
            self.total_passed += 1
        elif result.result == "FAIL":
            self.total_failed += 1
        else:
            self.total_skipped += 1
    
    def to_dict(self):
        return {
            "total_passed": self.total_passed,
            "total_failed": self.total_failed,
            "total_skipped": self.total_skipped,
            "duration": (self.end_time - self.start_time) if self.end_time and self.start_time else 0,
            "results": [
                {
                    "module": r.module,
                    "name": r.name,
                    "result": r.result,
                    "duration": r.duration,
                    "message": r.message
                }
                for r in self.results
            ]
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
    
    parser = argparse.ArgumentParser(description="QueenX Kernel Test Runner")
    parser.add_argument("--unit", action="store_true", help="Run unit tests")
    parser.add_argument("--integration", action="store_true", help="Run integration tests")
    parser.add_argument("--stress", action="store_true", help="Run stress tests")
    parser.add_argument("--all", action="store_true", help="Run all tests")
    parser.add_argument("--timeout", type=int, default=120, help="Timeout for unit tests (seconds)")
    
    args = parser.parse_args()
    
    if not (args.unit or args.integration or args.stress or args.all):
        args.all = True
    
    reports = []
    
    if args.unit or args.all:
        reports.append(("Unit Tests", run_unit_tests(args.timeout)))
    
    if args.integration or args.all:
        reports.append(("Integration Tests", run_integration_tests()))
    
    if args.stress or args.all:
        reports.append(("Stress Tests", run_stress_tests()))
    
    print("\n" + "=" * 60)
    print("FINAL TEST SUMMARY")
    print("=" * 60)
    
    total_passed = sum(r.total_passed for _, r in reports)
    total_failed = sum(r.total_failed for _, r in reports)
    total_skipped = sum(r.total_skipped for _, r in reports)
    
    for name, report in reports:
        print(f"\n{name}:")
        print(f"  Passed:  {report.total_passed}")
        print(f"  Failed:  {report.total_failed}")
        print(f"  Skipped: {report.total_skipped}")
    
    print(f"\nOverall:")
    print(f"  Total Passed:  {total_passed}")
    print(f"  Total Failed:  {total_failed}")
    print(f"  Total Skipped: {total_skipped}")
    
    if total_failed > 0:
        print("\n❌ SOME TESTS FAILED")
        sys.exit(1)
    elif total_passed > 0:
        print("\n✅ ALL TESTS PASSED")
        sys.exit(0)
    else:
        print("\n⚠️ NO TESTS RUN")
        sys.exit(2)

if __name__ == "__main__":
    main()
