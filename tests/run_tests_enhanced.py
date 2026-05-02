#!/usr/bin/env python3
"""
QueenX Enhanced Test Runner
============================

Features:
- Robust QEMU output parsing (handles Unicode, ANSI, etc.)
- Multiple test modes: quick, full, verbose
- Automatic log management and rotation
- Detailed error diagnosis
- JSON and human-readable reports

Usage:
    python3 run_tests_enhanced.py              # Run all tests
    python3 run_tests_enhanced.py --unit        # Unit tests only
    python3 run_tests_enhanced.py --quick       # Quick mode (30s timeout)
    python3 run_tests_enhanced.py --verbose     # Show QEMU debug info
"""

import subprocess
import sys
import os
import re
import json
import time
import argparse
from datetime import datetime
from pathlib import Path
from typing import Optional, Dict, List, Tuple

# Configuration
PROJECT_ROOT = Path(__file__).parent.parent
TESTS_DIR = PROJECT_ROOT / "tests"
REPORTS_DIR = TESTS_DIR / "reports"
LOGS_DIR = PROJECT_ROOT / "logs"

QEMU_BIN = "qemu-system-x86_64"
KERNEL_BIN = PROJECT_ROOT / "build" / "kernel.bin"
KERNEL_TEST_BIN = PROJECT_ROOT / "build" / "kernel_test.bin"
ISO_FILE = PROJECT_ROOT / "build" / "antx_test.iso"

# QEMU base arguments
QEMU_BASE_ARGS = [
    QEMU_BIN,
    "-m", "512",
    "-no-reboot",
    "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"
]


class TestResult:
    """Individual test case result"""
    def __init__(self, module: str, name: str, result: str,
                 duration: float = 0, message: str = ""):
        self.module = module
        self.name = name
        self.result = result  # PASS, FAIL, SKIP, ERROR
        self.duration = duration
        self.message = message


class TestReport:
    """Complete test report"""
    def __init__(self):
        self.results: List[TestResult] = []
        self.total_passed = 0
        self.total_failed = 0
        self.total_skipped = 0
        self.total_errors = 0
        self.start_time: Optional[datetime] = None
        self.end_time: Optional[datetime] = None
        self.raw_output: str = ""
        self.qemu_stderr: str = ""
        self.exit_code: Optional[int] = None
        self.timeout_occurred: bool = False

    def add_result(self, result: TestResult):
        self.results.append(result)
        if result.result == "PASS":
            self.total_passed += 1
        elif result.result == "FAIL":
            self.total_failed += 1
        elif result.result == "SKIP":
            self.total_skipped += 1
        else:
            self.total_errors += 1

    def to_dict(self) -> dict:
        return {
            "timestamp": datetime.now().isoformat(),
            "summary": {
                "total": len(self.results),
                "passed": self.total_passed,
                "failed": self.total_failed,
                "skipped": self.total_skipped,
                "errors": self.total_errors,
                "duration_sec": (self.end_time - self.start_time).total_seconds()
                if self.end_time and self.start_time else 0
            },
            "results": [
                {
                    "module": r.module,
                    "name": r.name,
                    "result": r.result,
                    "duration_ms": r.duration * 1000,
                    "message": r.message
                }
                for r in self.results
            ],
            "metadata": {
                "timeout": self.timeout_occurred,
                "exit_code": self.exit_code,
                "output_length": len(self.raw_output)
            }
        }


class OutputParser:
    """Parse kernel test output with robust handling"""

    # Patterns for test output (supporting Unicode box-drawing chars)
    MODULE_PATTERN = re.compile(
        r'(?:Module|│\s*Module):\s*(.+?)[\s│]*$',
        re.MULTILINE
    )

    TEST_RESULT_PATTERN = re.compile(
        r'\[\s*(PASS|FAIL|SKIP)\s*\]\s*([^(\n]+?)\s*\((\d+)us\)',
        re.MULTILINE
    )

    FAIL_MESSAGE_PATTERN = re.compile(
        r'\[\s*FAIL\s*\]\s*([^\n]+?)\s*-\s*(.+?)(?:\s*\(|$)',
        re.MULTILINE
    )

    SUMMARY_PATTERN = re.compile(
        r'(?:✓|✗|○|Passed|Failed|Skipped)\s*:?\s*(\d+)',
        re.MULTILINE | re.IGNORECASE
    )

    @classmethod
    def clean_output(cls, raw: str) -> str:
        """Clean raw output by removing control characters and normalizing"""
        # Remove ANSI escape sequences
        ansi_escape = re.compile(r'\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])')
        cleaned = ansi_escape.sub('', raw)

        # Normalize line endings
        cleaned = cleaned.replace('\r\n', '\n').replace('\r', '\n')

        return cleaned

    @classmethod
    def parse(cls, raw_output: str) -> TestReport:
        """Parse test output into structured report"""
        report = TestReport()
        report.raw_output = raw_output
        report.start_time = datetime.now()

        cleaned = cls.clean_output(raw_output)

        current_module = None

        # Parse modules
        for line in cleaned.split('\n'):
            # Skip empty lines and decorative lines
            if not line.strip() or all(c in '═║╔╗╚╝┌┐└┘─│├┤┬┴┼' for c in line.strip()):
                continue

            # Check for module header
            module_match = cls.MODULE_PATTERN.search(line)
            if module_match:
                current_module = module_match.group(1).strip()
                continue

            # Check for test result
            test_match = cls.TEST_RESULT_PATTERN.search(line)
            if test_match and current_module:
                result = test_match.group(1).upper()
                name = test_match.group(2).strip()
                duration = int(test_match.group(3)) / 1000.0  # us -> ms

                message = ""
                if result == "FAIL":
                    msg_match = cls.FAIL_MESSAGE_PATTERN.search(line)
                    if msg_match:
                        message = msg_match.group(2).strip()

                report.add_result(TestResult(
                    module=current_module,
                    name=name,
                    result=result,
                    duration=duration,
                    message=message
                ))

        report.end_time = datetime.now()
        return report


def run_qemu_test(
    kernel_path: Path,
    iso_path: Optional[Path] = None,
    timeout: int = 120,
    verbose: bool = False,
    capture_serial: bool = True
) -> Tuple[TestReport, str]:
    """
    Run QEMU with the given kernel/ISO and capture output.

    Returns:
        Tuple of (TestReport, stderr_output)
    """
    report = TestReport()

    # Build QEMU command
    cmd = list(QEMU_BASE_ARGS)

    if iso_path and iso_path.exists():
        cmd.extend(["-cdrom", str(iso_path)])
    else:
        cmd.extend(["-kernel", str(kernel_path)])

    # Serial output configuration
    if capture_serial:
        # Use file backend to avoid stdio conflicts
        serial_log = LOGS_DIR / f"serial_{datetime.now().strftime('%Y%m%d_%H%M%S')}.log"
        cmd.extend(["-serial", f"file:{serial_log}"])
    else:
        cmd.extend(["-serial", "stdio"])

    # Display configuration
    cmd.extend(["-display", "none"])

    # Debug flags if verbose
    if verbose:
        cmd.extend(["-d", "int,cpu_reset,unimp,guest_errors"])

    print(f"\n{'='*70}")
    print(f"Running QEMU Test")
    print(f"{'='*70}")
    print(f"Command: {' '.join(cmd[:10])}...")  # Truncate long commands
    print(f"Timeout: {timeout}s")
    print(f"Kernel:  {kernel_path.name}")
    if iso_path:
        print(f"ISO:     {iso_path.name}")
    print(f"{'='*70}\n")

    try:
        LOGS_DIR.mkdir(parents=True, exist_ok=True)

        process = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=str(PROJECT_ROOT)
        )

        try:
            stdout, stderr = process.communicate(timeout=timeout)
            report.exit_code = process.returncode
            report.qemu_stderr = stderr.decode('utf-8', errors='replace')

        except subprocess.TimeoutExpired:
            process.kill()
            stdout, stderr = process.communicate()
            report.timeout_occurred = True
            report.exit_code = -1
            report.qemu_stderr = stderr.decode('utf-8', errors='replace')
            print(f"⏰  TIMEOUT: Test did not complete within {timeout}s")

        # Read serial log if captured
        if capture_serial and serial_log.exists():
            with open(serial_log, 'r', encoding='utf-8', errors='replace') as f:
                serial_output = f.read()
            report.raw_output = serial_output
        else:
            report.raw_output = stdout.decode('utf-8', errors='replace')

        # Parse the output
        parsed_report = OutputParser.parse(report.raw_output)

        # Merge metadata from our report
        parsed_report.qemu_stderr = report.qemu_stderr
        parsed_report.exit_code = report.exit_code
        parsed_report.timeout_occurred = report.timeout_occurred

        return parsed_report, report.qemu_stderr

    except FileNotFoundError:
        print(f"❌ ERROR: QEMU not found at '{QEMU_BIN}'")
        print("   Install with: sudo apt install qemu-system-x86")
        report.total_errors += 1
        return report, ""

    except Exception as e:
        print(f"❌ ERROR: Failed to run QEMU: {e}")
        report.total_errors += 1
        return report, ""


def diagnose_qemu_failure(stderr: str, output: str) -> List[str]:
    """
    Diagnose common QEMU failures and return actionable suggestions.
    """
    issues = []

    # Check for common error patterns
    if "Gdk" in stderr and "assertion" in stderr:
        issues.append("🖥️  Graphics environment issue detected")
        issues.append("   Solution: Use '-display none' or set DISPLAY variable")

    if "cannot use stdio by multiple character devices" in stderr:
        issues.append("🔌 Serial port conflict")
        issues.append("   Solution: Use '-serial file:<path>' instead of '-serial stdio'")

    if "Booting from ROM.." in output:
        count = output.count("Booting from ROM..")
        if count > 2:
            issues.append(f"🔄 Boot loop detected ({count} restarts)")
            issues.append("   Possible causes:")
            issues.append("   - Kernel panic on startup")
            issues.append("   - Missing multiboot header")
            issues.append("   - Memory configuration issue")

    if "triple fault" in output.lower():
        issues.append("💥 Triple fault (CPU shutdown)")
        issues.append("   The kernel caused a fatal exception")

    if not output.strip() or output.strip().startswith("SeaBIOS"):
        issues.append("📭 No kernel output received")
        issues.append("   Possible causes:")
        issues.append("   - Kernel didn't start")
        issues.append("   - Serial not initialized")
        issues.append("   - Wrong entry point")

    return issues


def print_report(report: TestReport, show_output: bool = False):
    """Print a formatted test report"""

    print(f"\n{'='*70}")
    print(f"TEST REPORT SUMMARY")
    print(f"{'='*70}")

    # Summary statistics
    total = report.total_passed + report.total_failed + report.total_skipped + report.total_errors
    print(f"\nTotal Tests:  {total}")
    print(f"✓ Passed:     {report.total_passed}")
    print(f"✗ Failed:     {report.total_failed}")
    print(f"○ Skipped:    {report.total_skipped}")

    if report.total_errors > 0:
        print(f"⚠ Errors:     {report.total_errors}")

    if report.timeout_occurred:
        print(f"⏰ Timeout:    YES (test didn't complete)")

    # Duration
    if report.start_time and report.end_time:
        duration = (report.end_time - report.start_time).total_seconds()
        print(f"⏱  Duration:   {duration:.1f}s")

    # Status
    print(f"\n{'─'*70}")
    if report.total_failed == 0 and report.total_errors == 0 and report.total_passed > 0:
        print("🎉  ALL TESTS PASSED!")
    elif report.total_passed == 0:
        print("⚠️  NO TESTS EXECUTED")
    else:
        print("⚠️  SOME TESTS FAILED OR ERRORED")

    # Failure details
    if report.total_failed > 0 or report.total_errors > 0:
        print(f"\n{'─'*70}")
        print("Failed/Error Details:")
        print(f"{'─'*70}")

        for r in report.results:
            if r.result in ("FAIL", "ERROR"):
                print(f"  [{r.result}] {r.module}::{r.name}")
                if r.message:
                    print(f"           └─ {r.message}")

    # Diagnose issues if no tests ran
    if total == 0:
        print(f"\n{'─'*70}")
        print("Diagnosis:")
        print(f"{'─'*70}")

        issues = diagnose_qemu_failure(report.qemu_stderr, report.raw_output)
        if issues:
            for issue in issues:
                print(f"  {issue}")
        else:
            print("  Unable to determine cause. Check logs manually.")
            print(f"  Output length: {len(report.raw_output)} bytes")
            print(f"  Stderr length: {len(report.qemu_stderr)} bytes")

    # Show output if requested
    if show_output and report.raw_output.strip():
        print(f"\n{'─'*70}")
        print("Raw Output (last 100 lines):")
        print(f"{'─'*70}")
        lines = report.raw_output.split('\n')[-100:]
        print('\n'.join(lines))

    print(f"\n{'='*70}\n")


def save_report(report: TestReport, prefix: str = "test") -> Path:
    """Save report to JSON file"""
    REPORTS_DIR.mkdir(parents=True, exist_ok=True)

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    report_file = REPORTS_DIR / f"{prefix}_{timestamp}.json"

    with open(report_file, 'w', encoding='utf-8') as f:
        json.dump(report.to_dict(), f, indent=2, ensure_ascii=False)

    print(f"📄 Report saved to: {report_file}")
    return report_file


def main():
    parser = argparse.ArgumentParser(
        description="QueenX Enhanced Test Runner",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  %(prog)s                  # Run all tests with default settings
  %(prog)s --unit            # Unit tests only
  %(prog)s --quick           # Quick mode (30s timeout)
  %(prog)s --verbose         # Show QEMU debug information
  %(prog)s --show-output     # Display captured serial output
        """
    )

    parser.add_argument(
        "--unit", action="store_true",
        help="Run unit tests only"
    )
    parser.add_argument(
        "--quick", action="store_true",
        help="Quick mode: shorter timeout (30s)"
    )
    parser.add_argument(
        "--verbose", action="store_true",
        help="Enable verbose QEMU debug output"
    )
    parser.add_argument(
        "--show-output", action="store_true",
        help="Display captured serial output after test"
    )
    parser.add_argument(
        "--timeout", type=int, default=120,
        help="Timeout in seconds (default: 120)"
    )
    parser.add_argument(
        "--no-save", action="store_true",
        help="Don't save report to file"
    )

    args = parser.parse_args()

    # Determine timeout
    timeout = 30 if args.quick else args.timeout

    # Determine which kernel to use
    kernel = KERNEL_TEST_BIN if KERNEL_TEST_BIN.exists() else KERNEL_BIN
    iso = ISO_FILE if ISO_FILE.exists() else None

    if not kernel.exists():
        print(f"❌ ERROR: Kernel binary not found: {kernel}")
        print("   Run 'make' or 'make test-unit' first.")
        sys.exit(1)

    # Run the test
    report, stderr = run_qemu_test(
        kernel_path=kernel,
        iso_path=iso,
        timeout=timeout,
        verbose=args.verbose
    )

    # Print report
    print_report(report, show_output=args.show_output)

    # Save report
    if not args.no_save:
        save_report(report, prefix="enhanced")

    # Exit code
    if report.total_failed > 0 or report.total_errors > 0:
        sys.exit(1)
    elif report.total_passed == 0:
        sys.exit(2)
    else:
        sys.exit(0)


if __name__ == "__main__":
    main()
