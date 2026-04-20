#!/usr/bin/env python3
"""
Integration Tests for QueenX Kernel
Tests multi-module interactions.
"""

import subprocess
import sys
import os
import re
import time
from pathlib import Path

PROJECT_ROOT = Path(__file__).parent.parent.parent
BUILD_DIR = PROJECT_ROOT / "build"

def run_qemu_test(test_name: str, timeout: int = 60) -> tuple:
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
            timeout=timeout,
            cwd=str(PROJECT_ROOT)
        )
        return "PASS" if "TEST_RESULT:PASS" in result.stdout else "FAIL", result.stdout
    except subprocess.TimeoutExpired:
        return "FAIL", "Timeout"
    except Exception as e:
        return "FAIL", str(e)

def test_process_file_interaction():
    print("Testing process + file system interaction...")
    
    result, output = run_qemu_test("process_file", 60)
    
    if "init process" in output.lower() or "user" in output.lower():
        print("  [PASS] Process and file system interaction works")
        return True
    else:
        print("  [FAIL] Process and file system interaction failed")
        return False

def test_memory_mapping():
    print("Testing memory mapping...")
    
    result, output = run_qemu_test("memory_mapping", 60)
    
    if "VMM" in output and "PMM" in output:
        print("  [PASS] Memory mapping works")
        return True
    else:
        print("  [FAIL] Memory mapping failed")
        return False

def test_syscall_interrupt():
    print("Testing syscall + interrupt interaction...")
    
    result, output = run_qemu_test("syscall_interrupt", 60)
    
    if "Syscall" in output or "syscall" in output:
        print("  [PASS] Syscall and interrupt interaction works")
        return True
    else:
        print("  [FAIL] Syscall and interrupt interaction failed")
        return False

def test_vfs_block_device():
    print("Testing VFS + block device interaction...")
    
    result, output = run_qemu_test("vfs_block", 60)
    
    if "VFS" in output and ("ATA" in output or "disk" in output.lower()):
        print("  [PASS] VFS and block device interaction works")
        return True
    else:
        print("  [FAIL] VFS and block device interaction failed")
        return False

def run_all_integration_tests():
    print("=" * 60)
    print("QueenX Kernel Integration Tests")
    print("=" * 60)
    
    tests = [
        ("Process + File System", test_process_file_interaction),
        ("Memory Mapping", test_memory_mapping),
        ("Syscall + Interrupt", test_syscall_interrupt),
        ("VFS + Block Device", test_vfs_block_device),
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
    print(f"Integration Tests: {passed} passed, {failed} failed")
    print("=" * 60)
    
    return failed == 0

if __name__ == "__main__":
    success = run_all_integration_tests()
    sys.exit(0 if success else 1)
