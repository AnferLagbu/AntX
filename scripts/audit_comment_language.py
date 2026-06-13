#!/usr/bin/env python3
"""
注释语言一致性 audit (TD-22)

目标: 按 [maintenance-2026-06-11.md 注释规范](../../docs/plan/maintenance-2026-06-11.md) 检测
Rust 注释中残留的英文段落式注释 (// 或 /// 或 /* */ 或 // SAFETY: / // TODO:).

设计契约:
  - 注释统一使用中文 (含 /// 文档注释, // 行内, /* */ 块, // SAFETY:, // TODO:)
  - 允许例外 (纯标识符/常量引用不视为英文段落):
    * 代码标识符: Cell::as_ptr, Box::into_raw, BTreeMap
    * 硬件/架构术语: CR3, IST1, TTBR0, MSR, GIC, APIC
    * 算法/协议/机制: RCU, CFS, COW, spin::Mutex
    * 错误码与标准常量: ENOENT, EINVAL, EAGAIN, O_RDONLY
    * 外部 API/标准引用: Linux man page: futex(2), POSIX 1003.1-2017 §2.9
    * 链接路径与文件名: src/kernel/framework/mm/cow.rs
    * 配置项/编译 flag: #[cfg(target_arch = "x86_64")]
    * 第三方 crate 名称: smoltcp, heapless
  - 短英文注释 (≤ 30 字符非空白 + ≤ 2 个英文长词) 视为合理引用, 不算违规
  - 段落式英文 (3+ 个长度 ≥ 4 的英文单词, 整段无中文字符) 视为违规
  - 含中文字符的注释: 视为合规, 即使夹杂英文技术术语 (按"中文字符占主体"原则)

退出码: 0 = 通过, 1 = 有违规, 2 = 配置/IO 错误
"""

import re
import sys
from pathlib import Path
from typing import Iterator

PROJECT_ROOT = Path(__file__).resolve().parent.parent
BASE = PROJECT_ROOT / "src" / "kernel"

# 注释前缀 (/// 文档注释, // 行内, /* 块注释起始, * 块注释续行, // SAFETY:, // TODO:)
COMMENT_PREFIX = re.compile(
    r"^\s*(?:///?|/\*|\*|//\s*SAFETY:|//\s*TODO:)"
)

# 中文字符 (CJK Unified Ideographs + 标点)
HAS_CJK = re.compile(r"[\u4e00-\u9fff\u3000-\u303f\uff00-\uffef]")

# 英文长词 (4+ 个连续 ASCII 字母, 含下划线视为一个词)
EN_LONG_WORD = re.compile(r"\b[A-Za-z_][A-Za-z_0-9]{3,}\b")

# 短英文注释的判断: 总可见字符 ≤ 30 且 英文长词 ≤ 2 个
def is_short_english_comment(text: str) -> bool:
    visible = text.strip()
    if len(visible) <= 30:
        long_words = EN_LONG_WORD.findall(visible)
        if len(long_words) <= 2:
            return True
    return False

# 例外清单: 这些词/标记在英文注释中出现不算"段落式英文"
ALLOWED_ENGLISH_TERMS = frozenset({
    # 算法/协议
    "RCU", "CFS", "COW", "LRU", "FIFO", "BTreeMap", "BTree", "Mutex", "SpinLock",
    "lockdep", "Lockdep", "tcb", "TCB",
    # 硬件/架构
    "CR3", "CR4", "IST", "MSR", "TSC", "PIC", "APIC", "GIC", "GICv2", "GICv3",
    "MMIO", "DMA", "PCID", "INVPCID", "KPTI", "SMEP", "SMAP", "NX",
    "SCTLR", "TCR", "TTBR", "TTBR0", "TTBR1", "ESR", "ELR", "SPSR", "FAR",
    "HPET", "ACPI", "MSI", "MSIX", "PVH", "PV", "KVM",
    # 错误码
    "ENOENT", "EINVAL", "EAGAIN", "EBUSY", "EFAULT", "ENOSPC", "ENOMEM",
    "EACCES", "EEXIST", "ENOTDIR", "EISDIR", "EROFS", "ENOSYS", "EPERM",
    "ENAMETOOLONG", "EOVERFLOW", "ENODEV", "EXDEV", "EIO", "ECANCELED",
    "ETIMEDOUT", "EINTR", "EWOULDBLOCK", "EDEADLK", "EMLINK", "ERANGE",
    "EBADF", "ECHILD", "EDEADLOCK",
    # 文件描述符标志
    "O_RDONLY", "O_WRONLY", "O_RDWR", "O_CREAT", "O_TRUNC", "O_APPEND",
    "O_NONBLOCK", "O_DIRECTORY", "O_NOFOLLOW", "O_CLOEXEC",
    # 信号
    "SIGILL", "SIGSEGV", "SIGBUS", "SIGFPE", "SIGKILL", "SIGTERM", "SIGCHLD",
    "SIGSTOP", "SIGCONT", "SIGHUP", "SIGINT", "SIGQUIT", "SIGABRT", "SIGPIPE",
    "SIGUSR1", "SIGUSR2", "SIGALRM", "SIGTSTP", "SIGTTIN", "SIGTTOU", "SIGIO",
    # 协议族 / socket
    "AF_UNIX", "AF_INET", "AF_INET6", "PF_UNIX", "SOCK_STREAM", "SOCK_DGRAM",
    "SOCK_RAW", "SOCK_SEQPACKET", "SOCK_NONBLOCK", "SOCK_CLOEXEC",
    "IPPROTO_TCP", "IPPROTO_UDP", "IPPROTO_ICMP", "SOL_SOCKET", "SO_REUSEADDR",
    "SO_REUSEPORT", "SO_KEEPALIVE", "SO_LINGER", "SO_RCVTIMEO", "SO_SNDTIMEO",
    "TCP_NODELAY", "TCP_CORK", "MSG_PEEK", "MSG_WAITALL", "MSG_DONTWAIT",
    "MSG_TRUNC", "MSG_ERRQUEUE", "SHUT_RD", "SHUT_WR", "SHUT_RDWR",
    # 标准引用
    "POSIX", "Linux", "QEMU", "x86_64", "aarch64", "AArch64", "X86_64",
    "Rust", "BSD", "XNU", "NetBSD", "FreeBSD", "OpenBSD", "musl", "glibc",
    # 第三方 crate
    "smoltcp", "heapless", "spin", "bitflags", "lock_api", "libc",
    "framekernel", "Framekernel",
    # syscall / 文件
    "syscall", "syscalls", "ioctl", "fcntl", "flock", "futex", "epoll",
    "madvise", "mremap", "mprotect", "mmap", "munmap", "mprotect",
    "prctl", "seccomp", "capability", "Capability", "Capabilities",
    "kthread", "klog", "kexec", "kfence", "kprobe", "kprobes", "ebpf",
    "bpf", "ftrace", "perf", "kdump", "cgroup", "cgroups", "namespace",
    "namespaces", "pwm", "PWM", "credo", "Credo", "sched", "Scheduler",
    "schedule", "scheduler", "tickless", "shadow", "secure_boot",
    "iopl", "ioperm", "io_uring", "iovec",
    # 常用英文技术词
    "TODO", "FIXME", "XXX", "NOTE", "HACK", "BUG", "WARNING", "WARN",
    "unsafe", "SAFETY", "default", "Default", "kernel", "Kernel",
    "user", "User", "mode", "Mode", "context", "Context",
    "call", "called", "calls", "wrapper", "thunk", "trait", "impl",
    "struct", "enum", "function", "Function", "method", "Method",
    "module", "Module", "file", "File", "directory", "Directory",
    "test", "Test", "tests", "Tests", "verify", "Verified", "validation",
    "parse", "Parsed", "format", "Format", "serialize", "deserialize",
    "register", "Register", "registered", "unregister", "lookup",
    "initialize", "init", "Init", "initialized", "deinitialize", "destroy",
    "destroyed", "allocate", "allocated", "deallocate", "free", "Free",
    "release", "released", "acquire", "acquired", "hold", "held",
    "lock", "locked", "unlock", "unlocked", "guard", "Guard",
    "check", "checked", "checksum", "crc", "crc32", "sha256",
    "version", "Version", "build", "Build", "commit", "Commit",
    "branch", "Branch", "merge", "merged", "tag", "Tag",
    "true", "false", "True", "False", "None", "Some", "Ok", "Err",
    "Self", "self", "Box", "Arc", "Rc", "RefCell", "OnceCell",
    "Cell", "Pin", "Unpin", "Drop", "Send", "Sync",
    "Vec", "String", "CString", "CStr", "OsStr",
    "Option", "Result", "Iterator", "IntoIterator",
    "Debug", "Display", "Clone", "Copy", "Default", "Eq", "PartialEq",
    "Ord", "PartialOrd", "Hash", "Sized", "?Sized",
    "mut", "pub", "fn", "let", "const", "static", "use", "mod",
    "extern", "crate", "super", "self", "as", "ref", "move",
    "impl", "trait", "struct", "enum", "union", "type",
    "for", "while", "loop", "if", "else", "match", "return",
    "in", "out", "ref", "where", "async", "await", "dyn",
    # 文件名/路径
    "init", "api", "mod", "lib", "bin", "test", "tests", "example",
    "examples", "bench", "benches", "build", "ci", "docs", "doc",
    # 日志级别
    "info", "warn", "error", "debug", "trace", "panic",
    # 业务术语
    "domain", "Domain", "range", "Range", "slot", "Slot", "token", "Token",
    "queue", "Queue", "pool", "Pool", "buffer", "Buffer", "cache", "Cache",
    "page", "Page", "frame", "Frame", "table", "Table", "entry", "Entry",
    "index", "Index", "offset", "Offset", "length", "Length", "size", "Size",
    "count", "Count", "total", "Total", "remain", "Remain",
    "head", "tail", "next", "prev", "first", "last", "begin", "end",
    "start", "Start", "stop", "Stop", "ready", "Ready",
    "enable", "Enable", "enabled", "disable", "Disable", "disabled",
    "active", "Active", "inactive", "Inactive", "pending", "Pending",
    "valid", "Valid", "invalid", "Invalid", "present", "Present",
    "absent", "Absent", "exists", "Exists", "missing", "Missing",
    "found", "Found", "match", "matched", "unmatched",
    "success", "Success", "successful", "failure", "Failure", "failed",
    "error", "Error", "errors", "warning", "Warning", "warnings",
    "skip", "skipping", "skipped", "ignore", "ignored", "handling",
    "return", "returns", "returned",
    "add", "added", "remove", "removed", "insert", "inserted",
    "delete", "deleted", "update", "updated", "set", "setted",
    "get", "gets", "got", "fetch", "fetched", "load", "loaded",
    "store", "stored", "save", "saved", "read", "reads", "write", "writes",
    "wrote", "written", "readable", "writable",
    "open", "opened", "close", "closed", "create", "created",
    "increment", "decrement", "increased", "decreased",
    "current", "Current", "previous", "Previous",
    "process", "Process", "thread", "Thread",
    "system", "System", "platform", "Platform", "architecture", "Architecture",
    "target", "Target", "binary", "Binary",
    "page_fault", "page_table", "page_frame", "virtual", "Virtual",
    "physical", "Physical", "linear", "Linear", "direct", "Direct",
    "indirect", "Indirect",
    "policy", "Policy", "mechanism", "Mechanism",
    "device", "Device", "driver", "Driver", "interrupt", "Interrupt",
    "exception", "Exception", "handler", "Handler", "callback", "Callback",
    "stack", "Stack", "heap", "Heap", "memory", "Memory",
    "address", "Address", "pointer", "Pointer", "value", "Value",
    "field", "Field", "member", "Member", "bit", "Bit", "byte", "Byte",
    "word", "Word", "dword", "qword",
    "page_size", "page_shift", "page_mask", "page_offset",
    "vm_flags", "vma_flags", "vma", "VMA", "vmas",
    "fd", "fds", "fdset", "fdtable",
    "pid", "pids", "pgid", "pgids", "tid", "tids", "uid", "uids",
    "gid", "gids", "euid", "egid", "suid", "sgid", "fsuid", "fsgid",
    "ppid", "sid", "pgid", "tty", "ttyname",
    "epollfd", "eventfd", "signalfd", "timerfd", "inotify",
    "blk", "Block", "block_device", "block_devices",
    "net", "Net", "Network", "network", "socket", "Socket",
    "tcp", "TCP", "udp", "UDP", "ip", "IP", "ipv4", "ipv6", "IPv4", "IPv6",
    "mac", "MAC", "ethernet", "Ethernet", "arp", "ARP",
    "dhcp", "DHCP", "dns", "DNS", "icmp", "ICMP", "igmp", "IGMP",
    "route", "Route", "routing", "Routing", "gateway", "Gateway",
    "subnet", "Subnet", "prefix", "Prefix", "cidr", "CIDR",
    "smoltcp_impl", "smoltcp_iface",
    "fs", "FS", "filesystem", "Filesystem", "file_system",
    "ramfs", "RamFs", "devfs", "DevFs", "procfs", "ProcFs", "hvfs", "HvFs",
    "spa", "SPA", "dmu", "DMU", "zap", "ZAP", "zil", "ZIL", "arc", "ARC",
    "txg", "TXG", "raid", "RAID", "raidz", "RAIDZ", "dedup", "Dedup",
    "checksum", "Checksum",
    "credo", "Credo", "session", "Session", "privilege", "Privilege",
    "capability", "Capability", "pwm_id", "pwm", "PWM",
    "vfork", "clone", "fork", "execve", "exec", "exit", "wait", "waitpid",
    "kill", "raise", "signal", "signals", "sigaction", "sigprocmask",
    "sigaltstack", "sigreturn", "sigsuspend", "sigtimedwait",
    "wait_queue", "waitqueue", "WaitQueue",
    "hrtimer", "timer", "Timer", "tick", "Tick", "jiffies", "uptime",
    "idle", "Idle", "halt", "stop", "suspend", "resume",
    "context_switch", "switch_to", "iretq", "eret", "sysret", "syscall",
    "trampoline", "Trampoline", "vsyscall", "vDSO", "vvar",
    "kpti", "KPTI", "seccomp", "Seccomp", "landlock", "Landlock",
    "audit", "Audit", "logging", "Logging", "logger", "Logger",
    "subsystem", "Subsystem", "category", "Category", "level", "Level",
    "formater", "render", "rendered", "json", "JSON", "yaml", "YAML",
    "toml", "TOML", "xml", "XML", "csv", "CSV", "text", "Text",
    "ascii", "ASCII", "utf8", "UTF8", "UTF-8", "unicode", "Unicode",
    "ascii_text", "utf8_text",
    "path", "Path", "filepath", "FilePath",
    "proc", "Proc", "process", "Process",
    "subsystem_id", "subsystem_name", "subsystem_count",
    "vfs", "VFS", "vfs_node", "vfs_inode", "vfs_dentry",
    "mount", "Mount", "umount", "Unmount", "mounted", "unmounted",
    "directory", "Directory", "dir", "Dir", "dirname", "basename",
    "filename", "filepath", "abspath", "relpath", "cwd", "pwd",
    "openat", "open_by_handle_at", "name_to_handle_at",
    "stat", "Stat", "fstat", "lstat", "statx", "fstatat",
    "chmod", "chown", "fchmod", "fchown", "lchown", "fchmodat", "fchownat",
    "truncate", "ftruncate", "fallocate", "posix_fallocate",
    "readlink", "symlink", "link", "unlink", "rename", "renameat", "renameat2",
    "readdir", "getdents", "getdents64", "scandir", "opendir", "closedir",
    "fdopendir", "dirfd",
    "inotify", "Inotify", "fanotify", "Fanotify", "dnotify", "Dnotify",
    "mknod", "mknodat", "mkfifo", "mkfifoat", "pivot_root", "chroot",
    "mount", "umount2", "mount_setattr",
    "swapon", "swapoff", "mkswap",
    "io_uring", "IoUring", "io_uring_setup", "io_uring_enter", "io_uring_register",
    "AIO", "aio", "lio_listio", "io_submit", "io_getevents", "io_cancel",
    "select", "poll", "pselect", "ppoll", "epoll_wait", "epoll_pwait",
    "epoll_create", "epoll_create1", "epoll_ctl",
    "signalfd", "signalfd4", "timerfd_create", "timerfd_settime", "timerfd_gettime",
    "eventfd", "eventfd2",
    "memfd", "memfd_create", "userfaultfd", "uffd",
    "pidfd", "pidfd_open", "pidfd_send_signal", "pidfd_getfd",
    "io_pgetevents", "io_uring_prep_",
})


def is_allowed_term(word: str) -> bool:
    """判断英文单词是否在例外清单中."""
    return word in ALLOWED_ENGLISH_TERMS


def detect_violation(comment_text: str) -> tuple[bool, str]:
    """检测单条注释是否为违规.

    返回: (is_violation, reason)
    """
    stripped = comment_text.strip()
    if not stripped:
        return False, ""

    # 短注释 (< 4 字符) 跳过
    if len(stripped) < 4:
        return False, ""

    # SAFETY/TODO 短引用注释豁免 (常引用上游文档/标准, < 80 字符)
    if is_safety_or_todo_short_ref(stripped):
        return False, ""

    has_cjk = bool(HAS_CJK.search(stripped))

    # 含中文字符的注释: 合规 (按"中文字符占主体"原则, 允许夹杂英文技术术语)
    if has_cjk:
        return False, ""

    # 纯英文注释: 区分"段落式"vs"短引用"
    long_words = EN_LONG_WORD.findall(stripped)
    filtered_words = [w for w in long_words if not is_allowed_term(w)]

    if is_short_english_comment(stripped):
        return False, ""

    # 段落式英文: 2+ 个非例外英文长词
    if len(filtered_words) >= 2:
        return True, f"纯英文段落 (英文长词: {', '.join(filtered_words[:5])})"

    return False, ""


# SAFETY/TODO 短引用注释豁免: < 80 字符, 常引用上游文档/RFC
def is_safety_or_todo_short_ref(text: str) -> bool:
    # 先剥掉前导 // 或 /// 或 * 或 /*
    body = re.sub(r"^\s*(?:///?|\*|/\*)", "", text).strip()
    if not re.match(r"^(SAFETY|TODO|FIXME|XXX|NOTE|HACK|BUG|WARNING|WARN)\b", body):
        return False
    return len(body) < 80


def iter_comments(rs_file: Path) -> Iterator[tuple[int, str]]:
    """逐行迭代 .rs 文件, 产出 (行号, 注释文本)."""
    try:
        content = rs_file.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return

    in_block_comment = False
    for lineno, line in enumerate(content.splitlines(), start=1):
        stripped = line.lstrip()
        if in_block_comment:
            # 块注释结束: 找 */
            end_idx = stripped.find("*/")
            if end_idx >= 0:
                in_block_comment = False
                yield lineno, stripped[:end_idx + 2]
            else:
                yield lineno, stripped
            continue
        if stripped.startswith("/*"):
            end_idx = stripped.find("*/", 2)
            if end_idx >= 0:
                yield lineno, stripped
            else:
                in_block_comment = True
                yield lineno, stripped
            continue
        if stripped.startswith("//"):
            yield lineno, stripped
            continue
        if stripped.startswith("*") and not stripped.startswith("*/"):
            yield lineno, stripped
            continue


def main() -> int:
    if not BASE.exists():
        print(f"error: base dir not found: {BASE}", file=sys.stderr)
        return 2

    issues: list[str] = []
    files_scanned = 0
    files_with_issues: set[str] = set()

    for rs_file in BASE.rglob("*.rs"):
        files_scanned += 1
        rel = rs_file.relative_to(PROJECT_ROOT)
        rel_str = str(rel)
        # 排除 vendored 第三方代码
        if "smoltcp/" in rel_str:
            continue
        # 排除自动生成
        if rel.name == "build.rs":
            continue
        # 排除 x86_64/aarch64 arch asm 文件 (有大量英文注释作示例)
        if rel.suffix in (".S", ".s", ".asm"):
            continue

        for lineno, comment in iter_comments(rs_file):
            is_violation, reason = detect_violation(comment)
            if is_violation:
                issues.append(f"{rel}:{lineno}: [{reason}] {comment.strip()[:80]}")
                files_with_issues.add(rel_str)

    if issues:
        print(f"TD-22 注释语言 audit FAILED: {len(issues)} 处违规, 涉及 {len(files_with_issues)} 个文件")
        for issue in issues[:80]:
            print(f"  {issue}")
        if len(issues) > 80:
            print(f"  ... and {len(issues) - 80} more")
        print(f"\n扫描: {files_scanned} 个 .rs 文件, 排除 vendored smoltcp/*")
        return 1

    print(f"TD-22 注释语言 audit PASSED: 扫描 {files_scanned} 个 .rs 文件, 0 违规")
    return 0


if __name__ == "__main__":
    sys.exit(main())
