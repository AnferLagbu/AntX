#!/usr/bin/env python3
"""
QueenX Test Orchestrator (Python + QEMU)
v2.0 — Rust-powered test framework runner

用法:
  python3 tests/run_kernel_tests.py              # 运行全部测试
  python3 tests/run_kernel_tests.py --quick       # 快速测试 (60s)
  python3 tests/run_kernel_tests.py --verbose     # 详细输出
  python3 tests/run_kernel_tests.py --module barrier  # 按模块运行
"""

import subprocess, sys, os, re, time, json, argparse, tempfile
from dataclasses import dataclass, field
from typing import Optional

PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KERNEL_BIN = os.path.join(PROJECT_ROOT, "build", "kernel.bin")

@dataclass
class TestResult:
    module: str
    name: str
    status: str  # PASS, FAIL, SKIP
    message: str = ""

@dataclass
class TestRun:
    results: list[TestResult] = field(default_factory=list)
    exit_code: int = 0
    output: str = ""
    elapsed: float = 0.0

def build_kernel(verbose=False):
    """Build the test kernel with --features kernel_test"""
    print("[BUILD] Building test kernel...")
    
    # Build Rust with test feature
    rust_dir = os.path.join(PROJECT_ROOT, "src", "rust")
    cmd = ["cargo", "build", "--release", "--features", "kernel_test", 
           "--target-dir", os.path.join(rust_dir, "target", "test-release")]
    
    result = subprocess.run(cmd, cwd=rust_dir, capture_output=not verbose,
                          text=True, timeout=120)
    if result.returncode != 0:
        print(f"[FAIL] Rust build failed:\n{result.stderr}")
        return False
    
    # Build C components + link
    os.chdir(PROJECT_ROOT)
    result = subprocess.run(["make", "test-unit"], capture_output=not verbose,
                          text=True, timeout=120)
    if result.returncode != 0:
        print(f"[FAIL] Make test-unit failed:\n{result.stdout}\n{result.stderr}")
        return False
    
    print("[BUILD] Test kernel built successfully")
    return True

def find_qemu():
    """Find QEMU binary"""
    for name in ["qemu-system-x86_64", "qemu-system-x86_64.exe"]:
        for path in os.environ.get("PATH", "").split(os.pathsep):
            full = os.path.join(path, name)
            if os.path.isfile(full):
                return full
    return "qemu-system-x86_64"

def run_tests(timeout=60, verbose=False) -> TestRun:
    """Boot the test kernel in QEMU and collect results"""
    test_bin = os.path.join(PROJECT_ROOT, "build", "kernel_test.bin")
    
    if not os.path.isfile(test_bin):
        print(f"[ERROR] Test kernel not found: {test_bin}")
        print("[INFO] Run: python3 tests/run_kernel_tests.py --build")
        return TestRun(exit_code=-1)
    
    # Convert to flat binary
    flat_bin = os.path.join(PROJECT_ROOT, "build", "kernel_test.flat")
    subprocess.run(["objcopy", "-O", "binary", test_bin, flat_bin], capture_output=True)
    
    log_file = os.path.join(PROJECT_ROOT, "logs", "test_output.log")
    os.makedirs(os.path.dirname(log_file), exist_ok=True)
    
    qemu = find_qemu()
    cmd = [
        qemu, "-m", "512", "-no-reboot",
        "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04",
        "-kernel", flat_bin,
        "-serial", f"file:{log_file}",
        "-display", "none"
    ]
    
    print(f"[RUN] Booting test kernel (timeout={timeout}s)...")
    start = time.time()
    
    if verbose:
        cmd[-3] = "-serial"  # remove file: prefix
        cmd[-2] = "stdio"    # use stdio
        result = subprocess.run(cmd, timeout=timeout, capture_output=False)
    else:
        result = subprocess.run(cmd, timeout=timeout,
                              capture_output=True, text=True)
    
    elapsed = time.time() - start
    
    # Parse output
    with open(log_file, 'r') as f:
        output = f.read()
    
    return parse_output(output, elapsed)

def parse_output(output: str, elapsed: float) -> TestRun:
    """Parse serial output for test results"""
    run = TestRun(output=output, elapsed=elapsed)
    
    # Parse header
    for line in output.split('\n'):
        line = line.strip()
        
        # Parse: "=== Running N tests ==="
        if "=== Running" in line and "tests ===" in line:
            continue
        
        # Parse: "=== DONE: N/M passed, K FAILED ==="
        if "=== DONE:" in line:
            # Extract pass/fail counts
            m = re.search(r'(\d+)/(\d+)\s*passed', line)
            if m:
                passed = int(m.group(1))
                total = int(m.group(2))
                run.results.append(TestResult("SUMMARY", "", "STATS",
                    f"{passed}/{total} passed"))
            m = re.search(r'(\d+)\s+FAILED', line)
            if m:
                run.exit_code = 1
            continue
        
        # Parse: "=== ALL TESTS PASSED (N/N) ==="
        if "ALL TESTS PASSED" in line:
            continue
        
        # Parse: "FAIL module::name : message"
        if "FAIL" in line and "::" in line:
            # Format: [ERR] [TEST] FAIL module::name : message
            parts = line.split("::", 1)
            if len(parts) >= 2:
                module = parts[0].split()[-1]
                name_msg = parts[1].split(":", 1)
                name = name_msg[0].strip()
                msg = name_msg[1].strip() if len(name_msg) > 1 else ""
                run.results.append(TestResult(module, name, "FAIL", msg))
                run.exit_code = 1
        
        # Parse: "TEST module::name PASS" (implicit - registered but no FAIL)
        # We can't know PASS count from output alone without parsing registration
    
    return run

def print_report(run: TestRun):
    """Print formatted test report"""
    fails = [r for r in run.results if r.status == "FAIL"]
    stats = [r for r in run.results if r.status == "STATS"]
    
    print("\n" + "=" * 60)
    print("  QueenX Test Results")
    print("=" * 60)
    
    if fails:
        print(f"\n  FAILURES ({len(fails)}):")
        for r in fails:
            print(f"    [{r.status}] {r.module}::{r.name}")
            if r.message:
                print(f"          {r.message}")
    
    for s in stats:
        print(f"\n  {s.message}")
    
    print(f"\n  Elapsed: {run.elapsed:.1f}s")
    
    if fails:
        print(f"\n  Result: {len(fails)} FAILURE(S)")
    else:
        print(f"\n  Result: ALL PASSED")
    
    print("=" * 60)

def main():
    parser = argparse.ArgumentParser(description="QueenX Test Orchestrator")
    parser.add_argument("--build", action="store_true", help="Build test kernel first")
    parser.add_argument("--quick", action="store_true", help="Quick mode (60s timeout)")
    parser.add_argument("--verbose", "-v", action="store_true", help="Verbose output")
    parser.add_argument("--timeout", type=int, default=120, help="QEMU timeout in seconds")
    
    args = parser.parse_args()
    
    if args.build:
        if not build_kernel(args.verbose):
            sys.exit(1)
    
    timeout = 60 if args.quick else args.timeout
    run = run_tests(timeout, args.verbose)
    
    print_report(run)
    
    sys.exit(run.exit_code)

if __name__ == "__main__":
    main()
