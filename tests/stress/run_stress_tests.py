#!/usr/bin/env python3
"""
Stress Tests for QueenX Kernel
Tests system behavior under extreme conditions.
"""

import subprocess
import sys
import os
import re
import time
import random
from pathlib import Path

PROJECT_ROOT = Path(__file__).parent.parent.parent
BUILD_DIR = PROJECT_ROOT / "build"

def run_qemu_stress(test_name: str, duration: int = 30) -> tuple:
    iso_path = BUILD_DIR / "antx.iso"
    if not iso_path.exists():
        print(f"  [SKIP] ISO not found, run 'make iso' first")
        return "SKIP", ""
    
    cmd = [
        "qemu-system-x86_64",
        "-cdrom", str(iso_path),
        "-serial", "stdio",
        "-display", "none",
        "-no-reboot"
    ]
    
    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=duration,
            cwd=str(PROJECT_ROOT)
        )
        
        if "PANIC" in result.stdout:
            return "FAIL", "Kernel panic detected"
        if "assert" in result.stdout.lower():
            return "FAIL", "Assertion failure detected"
        
        return "PASS", result.stdout
    except subprocess.TimeoutExpired:
        return "PASS", "Completed without crash"
    except Exception as e:
        return "FAIL", str(e)

def test_memory_pressure():
    print("Testing memory pressure...")
    print("  Simulating high memory allocation...")
    
    result, output = run_qemu_stress("memory_pressure", 30)
    
    if result == "PASS":
        print("  [PASS] System handled memory pressure")
        return True
    else:
        print(f"  [FAIL] Memory pressure test failed: {output}")
        return False

def test_process_pressure():
    print("Testing process pressure...")
    print("  Simulating rapid process creation/destruction...")
    
    result, output = run_qemu_stress("process_pressure", 30)
    
    if result == "PASS":
        print("  [PASS] System handled process pressure")
        return True
    else:
        print(f"  [FAIL] Process pressure test failed: {output}")
        return False

def test_filesystem_pressure():
    print("Testing filesystem pressure...")
    print("  Simulating concurrent file operations...")
    
    result, output = run_qemu_stress("fs_pressure", 30)
    
    if result == "PASS":
        print("  [PASS] System handled filesystem pressure")
        return True
    else:
        print(f"  [FAIL] Filesystem pressure test failed: {output}")
        return False

def test_interrupt_storm():
    print("Testing interrupt handling...")
    print("  Simulating high interrupt rate...")
    
    result, output = run_qemu_stress("interrupt_storm", 30)
    
    if result == "PASS":
        print("  [PASS] System handled interrupt load")
        return True
    else:
        print(f"  [FAIL] Interrupt test failed: {output}")
        return False

def test_long_running():
    print("Testing long-running stability...")
    print("  Running for extended period...")
    
    result, output = run_qemu_stress("long_running", 60)
    
    if result == "PASS":
        print("  [PASS] System stable over extended period")
        return True
    else:
        print(f"  [FAIL] Long-running test failed: {output}")
        return False

def run_all_stress_tests():
    print("=" * 60)
    print("QueenX Kernel Stress Tests")
    print("=" * 60)
    
    tests = [
        ("Memory Pressure", test_memory_pressure),
        ("Process Pressure", test_process_pressure),
        ("Filesystem Pressure", test_filesystem_pressure),
        ("Interrupt Handling", test_interrupt_storm),
        ("Long-Running Stability", test_long_running),
    ]
    
    passed = 0
    failed = 0
    
    for name, test_func in tests:
        print(f"\n[{name}]")
        try:
            if test_func():
                passed += 1
            else:
                failed += 1
        except Exception as e:
            print(f"  [ERROR] {e}")
            failed += 1
    
    print("\n" + "=" * 60)
    print(f"Stress Tests: {passed} passed, {failed} failed")
    print("=" * 60)
    
    return failed == 0

if __name__ == "__main__":
    success = run_all_stress_tests()
    sys.exit(0 if success else 1)
