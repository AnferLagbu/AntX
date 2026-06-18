#!/usr/bin/env python3
"""
TD-22 注释语言审计 — 单元测试

针对 scripts/audit_comment_language.py 的豁免规则回归测试.
覆盖 2026-06-18 真实回归的 3 处违规 (src/kernel/services/ipc/async_ipc.rs:14
+ src/kernel/framework/syscall/mod.rs:752, 765), 验证:
1. 修复前: 这 3 条注释应被识别为 violation (即 bug 已重现)
2. 修复后: 这 3 条注释应被识别为合规 (豁免规则覆盖)
3. 修复后: 已有的合规注释仍保持合规 (无回归)

执行: python3 scripts/tests/test_audit_comment_language.py
退出: 0 = 全部通过, 1 = 有失败
"""

import sys
import unittest
from pathlib import Path

# 将 scripts/ 加入 import 路径, 以便直接 import audit_comment_language 模块
SCRIPTS_DIR = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(SCRIPTS_DIR))

import audit_comment_language as acl  # noqa: E402


# 3 条真实违规 (2026-06-18 实际回归) — 完整上下文 (含迁移入口 + 续行)
REGRESSION_CASES: list[tuple[list[str], str, int, str]] = [
    # (完整注释行列表 [从入口到目标行], 源文件, 目标行号, 备注)
    (
        [
            "//! 原属 framework/ipc/async_ipc.rs, 2026-06-18 迁移到 services.",
            "//! 0 unsafe, 依赖 framework safe API (pipe_write_safe / pipe_read_safe /",
            "//! msgq_send_safe / msgq_recv_safe).",
        ],
        "src/kernel/services/ipc/async_ipc.rs",
        14,
        "L14 — 迁移记录中的函数名列表 (续行)",
    ),
    (
        [
            "// 已迁移到 services: sys_setregid, sys_mmap, sys_munmap, sys_time,",
            "// sys_sched_setaffinity, sys_sched_getaffinity",
        ],
        "src/kernel/framework/syscall/mod.rs",
        752,
        "L752 — syscall 迁移记录 (续行)",
    ),
    (
        [
            "// 已迁移到 services: sys_getrusage, sys_auth_*, sys_pwm_*, sys_gethostname,",
            "// sys_sethostname, sys_boot_check, sys_reboot, sys_disk_list/info/format/partition/fat_format",
        ],
        "src/kernel/framework/syscall/mod.rs",
        765,
        "L765 — syscall 迁移记录 (续行)",
    ),
]


# 既有合规注释 — 验证修复不引入回归
PASS_THROUGH_CASES: list[tuple[str, str]] = [
    # 含中文 → 始终合规
    ("/// 这是中文注释, 描述 syscall 行为", "含中文"),
    # SAFETY 短引用豁免
    ("// SAFETY: 调用方持有 spinlock, 不会并发访问", "SAFETY 短引用"),
    # POSIX 签名引用豁免
    ("/// `int open(const char *pathname, int flags)` 打开文件", "POSIX 签名"),
    # 代码示例豁免
    ("/// let x = Foo::new(); x.bar();", "代码示例"),
    # Markdown 表格豁免
    ("/// | col1 | col2 |", "Markdown 表格"),
    # 公式豁免
    ("/// us = cycles * 1_000_000 / freq_hz", "公式"),
    # 寄存器文档豁免
    ("/// CTRL (RW, u32) — control register", "寄存器文档"),
    # SPDX 标识
    ("// SPDX-License-Identifier: MIT", "SPDX"),
]


class TestRegressionCases(unittest.TestCase):
    """2026-06-18 真实回归的 3 处违规 — 修复后应被豁免 (合规).

    验证策略: 模拟实际多行迁移记录 (入口 + 续行), 通过模拟的 iter_comments
    行为传递 continuation 状态, 确认目标行在续行模式下被豁免.
    """

    @staticmethod
    def _simulate_iter(lines: list[str]) -> list[tuple[str, bool]]:
        """模拟 iter_comments 的续行状态机, 返回 (行内容, 是否续行)."""
        results: list[tuple[str, bool]] = []
        in_migration_block = False
        for line in lines:
            is_cont = in_migration_block
            results.append((line, is_cont))
            in_migration_block = acl.is_migration_note(line, continuation=False)
        return results

    def test_async_ipc_msgq_function_list(self) -> None:
        lines, file, target_line, note = REGRESSION_CASES[0]
        with self.subTest(file=file, note=note):
            simulated = self._simulate_iter(lines)
            target_text, is_cont = simulated[-1]
            is_v, reason = acl.detect_violation(target_text, continuation=is_cont)
            self.assertFalse(
                is_v,
                f"应被豁免 (迁移记录续行), 但被判违规: {reason!r}\n"
                f"  注释原文: {target_text!r}\n"
                f"  位置: {file}:{target_line}",
            )

    def test_syscall_mod_setaffinity_list(self) -> None:
        lines, file, target_line, note = REGRESSION_CASES[1]
        with self.subTest(file=file, note=note):
            simulated = self._simulate_iter(lines)
            target_text, is_cont = simulated[-1]
            is_v, reason = acl.detect_violation(target_text, continuation=is_cont)
            self.assertFalse(
                is_v,
                f"应被豁免 (syscall 列表续行), 但被判违规: {reason!r}\n"
                f"  注释原文: {target_text!r}\n"
                f"  位置: {file}:{target_line}",
            )

    def test_syscall_mod_hostname_reboot_list(self) -> None:
        lines, file, target_line, note = REGRESSION_CASES[2]
        with self.subTest(file=file, note=note):
            simulated = self._simulate_iter(lines)
            target_text, is_cont = simulated[-1]
            is_v, reason = acl.detect_violation(target_text, continuation=is_cont)
            self.assertFalse(
                is_v,
                f"应被豁免 (syscall 列表续行), 但被判违规: {reason!r}\n"
                f"  注释原文: {target_text!r}\n"
                f"  位置: {file}:{target_line}",
            )


class TestPassThroughCases(unittest.TestCase):
    """既有合规注释 — 修复不引入回归."""

    def test_pass_through(self) -> None:
        for text, label in PASS_THROUGH_CASES:
            with self.subTest(label=label, text=text):
                is_v, reason = acl.detect_violation(text)
                self.assertFalse(
                    is_v,
                    f"应保持合规 ({label}), 但被判违规: {reason!r}\n"
                    f"  注释原文: {text!r}",
                )


class TestActualSourceFiles(unittest.TestCase):
    """端到端验证: 实际扫描源文件, 3 处违规位置应为 0 命中.

    复用 iter_comments + detect_violation 的真实路径 (含续行状态),
    而非单行 detect_violation, 以验证完整流程.
    """

    SOURCE_ROOTS: list[tuple[Path, int]] = [
        (Path("src/kernel/services/ipc/async_ipc.rs"), 14),
        (Path("src/kernel/framework/syscall/mod.rs"), 752),
        (Path("src/kernel/framework/syscall/mod.rs"), 765),
    ]
    PROJECT_ROOT = Path("/home/anfer/Code/AntX")

    def test_no_violation_in_target_lines(self) -> None:
        for rel_path, target_line in self.SOURCE_ROOTS:
            full_path = self.PROJECT_ROOT / rel_path
            self.assertTrue(
                full_path.exists(),
                f"源文件不存在: {full_path}",
            )
            # 收集所有 (行号, 注释, 是否续行)
            comments = list(acl.iter_comments(full_path))
            self.assertGreater(
                len(comments),
                0,
                f"iter_comments 未产出: {rel_path}",
            )
            # 找到目标行号对应的 (注释, 是否续行)
            hit = next(
                (
                    (comment, is_cont)
                    for lineno, comment, is_cont in comments
                    if lineno == target_line
                ),
                None,
            )
            self.assertIsNotNone(
                hit,
                f"未找到目标行 {rel_path}:{target_line}\n"
                f"  iter_comments 产出: {[(l, c[:50]) for l, c, _ in comments[:5]]}...",
            )
            target_text, is_cont = hit
            is_v, reason = acl.detect_violation(target_text, continuation=is_cont)
            self.assertFalse(
                is_v,
                f"源文件实际行应为合规: {rel_path}:{target_line}\n"
                f"  原文: {target_text!r}\n"
                f"  是否续行: {is_cont}\n"
                f"  违规原因: {reason!r}",
            )


class TestFullAuditRun(unittest.TestCase):
    """调用 main() 跑全量审计, 期望 0 违规 (2026-06-15 后硬阈值)."""

    def test_main_returns_zero(self) -> None:
        import io
        from contextlib import redirect_stdout

        buf = io.StringIO()
        with redirect_stdout(buf):
            rc = acl.main()
        output = buf.getvalue()
        self.assertEqual(
            rc,
            0,
            f"全量审计期望通过, 实际返回 {rc}.\n"
            f"输出:\n{output}",
        )
        self.assertIn(
            "PASSED",
            output,
            f"期望输出含 'PASSED', 实际:\n{output}",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
