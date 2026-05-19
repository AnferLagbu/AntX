#!/usr/bin/env python3
"""
Analyze chaos test logs from QueenX.
Parses serial output to compute fault injection recovery rate.
"""

import sys
import re
from pathlib import Path

def analyze_chaos_log(log_path: str):
    log = Path(log_path)
    if not log.exists():
        print(f"Error: log file not found: {log_path}")
        sys.exit(1)

    content = log.read_text(errors="replace")

    fault_injections = len(re.findall(r'\[FAULT-INJECT\]', content))
    recoveries = len(re.findall(r'mark_recovered|Recovery complete|recovered successfully', content, re.IGNORECASE))
    rollbacks = len(re.findall(r'rollback|RollingBack', content, re.IGNORECASE))
    quarantines = len(re.findall(r'Quarantined|quarantine', content, re.IGNORECASE))
    panics = len(re.findall(r'PANIC|panic!', content))
    triple_faults = len(re.findall(r'triple fault', content, re.IGNORECASE))

    barrier_captures = len(re.findall(r'barrier_capture|push_barrier_snapshot', content, re.IGNORECASE))
    undo_rollbacks = len(re.findall(r'rollback_to|UndoLog.*rollback', content, re.IGNORECASE))

    test_passed = len(re.findall(r'PASS|passed', content))
    test_failed = len(re.findall(r'FAIL(ED)?|FAILED', content))

    print("=" * 60)
    print("  AntX Chaos Test Analysis Report")
    print("=" * 60)
    print(f"  Log file: {log_path}")
    print(f"  Log size: {len(content)} bytes")
    print()
    print("  Fault Injection:")
    print(f"    Injections triggered:  {fault_injections}")
    print(f"    Barrier captures:      {barrier_captures}")
    print(f"    Undo rollbacks:        {undo_rollbacks}")
    print(f"    Domain recoveries:     {recoveries}")
    print(f"    Domain rollbacks:      {rollbacks}")
    print(f"    Quarantined domains:   {quarantines}")
    print()
    print("  Stability:")
    print(f"    Kernel panics:         {panics}")
    print(f"    Triple faults:         {triple_faults}")
    print()
    print("  Unit Tests:")
    print(f"    Passed:                {test_passed}")
    print(f"    Failed:                {test_failed}")
    print()

    if fault_injections > 0:
        recovery_rate = (recoveries / fault_injections) * 100
        print(f"  Recovery Rate: {recovery_rate:.1f}% ({recoveries}/{fault_injections})")
        if recovery_rate >= 99.0:
            print("  Status: EXCELLENT (>= 99%)")
        elif recovery_rate >= 95.0:
            print("  Status: GOOD (>= 95%)")
        elif recovery_rate >= 80.0:
            print("  Status: ACCEPTABLE (>= 80%)")
        else:
            print("  Status: POOR (< 80%) - needs investigation")
    else:
        print("  Recovery Rate: N/A (no fault injections detected)")
        print("  Hint: Ensure fault_injection feature is enabled and FAULT_RATE > 0")

    print()
    print("=" * 60)

    if fault_injections == 0:
        return 2
    if panics > 0 and recoveries == 0:
        return 1
    return 0

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: analyze_chaos.py <chaos_test_log>")
        sys.exit(2)
    sys.exit(analyze_chaos_log(sys.argv[1]))
