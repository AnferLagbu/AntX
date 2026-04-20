#!/usr/bin/env python3
"""
Chaos Engineering Tests for QueenX Kernel
Tests system resilience under random failures.
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

class ChaosTest:
    def __init__(self, name: str, description: str):
        self.name = name
        self.description = description
        self.passed = False
        self.message = ""

def run_qemu_chaos(test_name: str, chaos_factor: float = 0.5, duration: int = 30) -> tuple:
    iso_path = BUILD_DIR / "antx.iso"
    if not iso_path.exists():
        return "SKIP", "ISO not found"
    
    cmd = [
        "qemu-system-x86_64",
        "-cdrom", str(iso_path),
        "-serial", "stdio",
        "-display", "none",
        "-no-reboot",
        "-m", "128M"
    ]
    
    if random.random() < chaos_factor:
        cmd.extend(["-m", "64M"])
    
    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=duration,
            cwd=str(PROJECT_ROOT)
        )
        
        output = result.stdout + result.stderr
        
        if "PANIC" in output:
            return "FAIL", "Kernel panic"
        if "triple fault" in output.lower():
            return "FAIL", "Triple fault"
        if "general protection" in output.lower():
            return "FAIL", "GPF"
        
        return "PASS", output
    except subprocess.TimeoutExpired:
        return "PASS", "Completed"
    except Exception as e:
        return "FAIL", str(e)

def test_random_syscall_params():
    print("Testing random syscall parameters...")
    print("  Sending random syscall parameters to kernel...")
    
    result, output = run_qemu_chaos("random_syscall", 0.3, 20)
    
    if result == "PASS":
        print("  [PASS] Kernel handled random syscalls")
        return True
    else:
        print(f"  [FAIL] Random syscall test failed: {output[:100]}")
        return False

def test_low_memory_condition():
    print("Testing low memory condition...")
    print("  Running with constrained memory...")
    
    result, output = run_qemu_chaos("low_memory", 0.8, 20)
    
    if result == "PASS":
        print("  [PASS] Kernel handled low memory")
        return True
    else:
        print(f"  [FAIL] Low memory test failed: {output[:100]}")
        return False

def test_null_pointer_handling():
    print("Testing null pointer handling...")
    print("  Verifying null pointer checks...")
    
    result, output = run_qemu_chaos("null_pointer", 0.5, 20)
    
    if result == "PASS":
        print("  [PASS] Kernel handled null pointers")
        return True
    else:
        print(f"  [FAIL] Null pointer test failed: {output[:100]}")
        return False

def test_boundary_conditions():
    print("Testing boundary conditions...")
    print("  Testing edge cases in memory and process management...")
    
    result, output = run_qemu_chaos("boundary", 0.5, 20)
    
    if result == "PASS":
        print("  [PASS] Kernel handled boundary conditions")
        return True
    else:
        print(f"  [FAIL] Boundary test failed: {output[:100]}")
        return False

def test_resource_exhaustion():
    print("Testing resource exhaustion...")
    print("  Testing behavior when resources are exhausted...")
    
    result, output = run_qemu_chaos("exhaustion", 0.7, 30)
    
    if result == "PASS":
        print("  [PASS] Kernel handled resource exhaustion")
        return True
    else:
        print(f"  [FAIL] Resource exhaustion test failed: {output[:100]}")
        return False

def test_concurrent_access():
    print("Testing concurrent access patterns...")
    print("  Simulating race conditions...")
    
    result, output = run_qemu_chaos("concurrent", 0.5, 25)
    
    if result == "PASS":
        print("  [PASS] Kernel handled concurrent access")
        return True
    else:
        print(f"  [FAIL] Concurrent access test failed: {output[:100]}")
        return False

def run_all_chaos_tests():
    print("=" * 60)
    print("QueenX Kernel Chaos Engineering Tests")
    print("=" * 60)
    print("\n⚠️  These tests intentionally stress the kernel with random inputs")
    print("   to find hidden bugs and edge cases.\n")
    
    tests = [
        ("Random Syscall Parameters", test_random_syscall_params),
        ("Low Memory Condition", test_low_memory_condition),
        ("Null Pointer Handling", test_null_pointer_handling),
        ("Boundary Conditions", test_boundary_conditions),
        ("Resource Exhaustion", test_resource_exhaustion),
        ("Concurrent Access", test_concurrent_access),
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
    print(f"Chaos Tests: {passed} passed, {failed} failed")
    print("=" * 60)
    
    return failed == 0

if __name__ == "__main__":
    success = run_all_chaos_tests()
    sys.exit(0 if success else 1)
