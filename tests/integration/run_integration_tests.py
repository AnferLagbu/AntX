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

def run_qemu_test(timeout: int = 30) -> str:
    iso_path = BUILD_DIR / "antx.iso"
    if not iso_path.exists():
        print(f"  [SKIP] ISO not found, run 'make iso' first")
        return ""
    
    cmd = [
        "qemu-system-x86_64",
        "-cdrom", str(iso_path),
        "-serial", "stdio",
        "-display", "none",
        "-no-reboot",
        "-m", "512M"
    ]
    
    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            cwd=str(PROJECT_ROOT)
        )
        output = result.stdout + result.stderr
        return output
    except subprocess.TimeoutExpired as e:
        if e.stdout:
            output = e.stdout.decode() if isinstance(e.stdout, bytes) else e.stdout
        else:
            output = ""
        if e.stderr:
            output += e.stderr.decode() if isinstance(e.stderr, bytes) else e.stderr
        return output
    except Exception as e:
        print(f"  [ERROR] {e}")
        return ""

def test_process_file_interaction(output: str) -> bool:
    print("Testing process + file system interaction...")
    
    if "init process" in output.lower() or "[INIT]" in output:
        print("  [PASS] Process and file system interaction works")
        return True
    else:
        print("  [FAIL] Process and file system interaction failed")
        return False

def test_memory_mapping(output: str) -> bool:
    print("Testing memory mapping...")
    
    if "VMM" in output and "PMM" in output:
        print("  [PASS] Memory mapping works")
        return True
    else:
        print("  [FAIL] Memory mapping failed")
        return False

def test_syscall_interrupt(output: str) -> bool:
    print("Testing syscall + interrupt interaction...")
    
    if "Syscall" in output or "syscall" in output.lower() or "[OK] Syscall" in output:
        print("  [PASS] Syscall and interrupt interaction works")
        return True
    else:
        print("  [FAIL] Syscall and interrupt interaction failed")
        return False

def test_vfs_block_device(output: str) -> bool:
    print("Testing VFS + block device interaction...")
    
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
    
    print("\nRunning kernel and capturing output...")
    output = run_qemu_test(30)
    
    if not output:
        print("  [ERROR] No output captured from kernel")
        return False
    
    print(f"  Captured {len(output)} bytes of output\n")
    
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
            if test_func(output):
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
