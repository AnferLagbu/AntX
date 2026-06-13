// POSIX errno 命名约定 (EAGAIN/EACCES/...) — 全大写缩写是有意的
#![allow(clippy::upper_case_acronyms)]

/// Syscall 类型定义和常量
///
/// 编号空间分配 (遵循 queenx-naming-standpoint.md):
///   0-299   : 保留给未来 linuxulator (与 Linux 1:1 映射)
///   300-399 : 保留
///   400-499 : Credo 私有 syscall
///   500-599 : 进程 / 内存 / 文件基础
///   600-699 : 网络 / IPC
///   700-799 : 设备 / 系统
///   800-899 : 扩展

pub const SYSCALL_INT: u8 = 0x80;
pub const MAX_SYSCALLS: u64 = 800;

// ==================== POSIX 标准 syscall 编号 ====================

// 文件 I/O
pub const SYS_read: u64 = 0;
pub const SYS_write: u64 = 1;
pub const SYS_open: u64 = 2;
pub const SYS_close: u64 = 3;
pub const SYS_stat: u64 = 4;
pub const SYS_fstat: u64 = 5;
pub const SYS_lstat: u64 = 6;
pub const SYS_poll: u64 = 7;
pub const SYS_lseek: u64 = 8;

// 内存管理
pub const SYS_mmap: u64 = 9;
pub const SYS_mprotect: u64 = 10;
pub const SYS_munmap: u64 = 11;
pub const SYS_brk: u64 = 12;

// 信号 (基础存根)
pub const SYS_rt_sigaction: u64 = 13;
pub const SYS_rt_sigprocmask: u64 = 14;
pub const SYS_rt_sigreturn: u64 = 15;

// 设备 I/O
pub const SYS_ioctl: u64 = 16;

// 文件访问
pub const SYS_access: u64 = 21;
pub const SYS_pipe: u64 = 22;
pub const SYS_select: u64 = 23;
pub const SYS_sched_yield: u64 = 24;

// 内存重映射
// TODO(TRACK-90BFB0): Phase N — implement mremap
pub const SYS_mremap: u64 = 25;

// 文件描述符
pub const SYS_dup: u64 = 32;
pub const SYS_dup2: u64 = 33;

// 进程优先级
pub const SYS_nice: u64 = 34;

// 暂停
pub const SYS_nanosleep: u64 = 35;

// ITIMER
// TODO(TRACK-8B3C91): Phase N — implement getitimer
pub const SYS_getitimer: u64 = 36;
pub const SYS_alarm: u64 = 37;
// TODO(TRACK-6564B9): Phase N — implement setitimer
pub const SYS_setitimer: u64 = 38;

// 进程基础
pub const SYS_getpid: u64 = 39;

// 网络 socket
pub const SYS_socket: u64 = 41;
pub const SYS_connect: u64 = 42;
pub const SYS_accept: u64 = 43;
pub const SYS_sendto: u64 = 44;
pub const SYS_recvfrom: u64 = 45;
pub const SYS_sendmsg: u64 = 46;
pub const SYS_recvmsg: u64 = 47;
pub const SYS_shutdown: u64 = 48;
pub const SYS_bind: u64 = 49;
pub const SYS_listen: u64 = 50;
pub const SYS_getsockname: u64 = 51;
pub const SYS_getpeername: u64 = 52;
pub const SYS_setsockopt: u64 = 54;
pub const SYS_getsockopt: u64 = 55;

// 进程
// TODO(TRACK-0FF0F0): Phase N — implement clone for thread creation
pub const SYS_clone: u64 = 56;
pub const SYS_fork: u64 = 57;
pub const SYS_execve: u64 = 59;
pub const SYS_exit: u64 = 60;
pub const SYS_wait4: u64 = 61;
pub const SYS_kill: u64 = 62;

// 系统信息
pub const SYS_uname: u64 = 63;

// 文件描述符操作
pub const SYS_fcntl: u64 = 72;

// 文件截断
pub const SYS_truncate: u64 = 76;
pub const SYS_ftruncate: u64 = 77;

// 目录
pub const SYS_getdents: u64 = 78;

// 路径
pub const SYS_getcwd: u64 = 79;
pub const SYS_chdir: u64 = 80;

// 文件重命名
pub const SYS_rename: u64 = 82;

// 目录操作
pub const SYS_mkdir: u64 = 83;
pub const SYS_rmdir: u64 = 84;

// 文件创建
pub const SYS_creat: u64 = 85;

// 文件链接
// TODO(TRACK-B62489): Phase N — implement hard links
pub const SYS_link: u64 = 86;
pub const SYS_unlink: u64 = 87;
// TODO(TRACK-CFB870): Phase N — implement symlinks
pub const SYS_symlink: u64 = 88;
pub const SYS_readlink: u64 = 89;

// 文件权限
pub const SYS_chmod: u64 = 90;
pub const SYS_fchmod: u64 = 91;
pub const SYS_chown: u64 = 92;
// TODO(TRACK-C3720B): Phase N — implement fchown as chown(fd→path) alias
pub const SYS_fchown: u64 = 93;

// 文件属性
pub const SYS_umask: u64 = 95;

// 时间
pub const SYS_gettimeofday: u64 = 96;
pub const SYS_getrlimit: u64 = 97;
pub const SYS_getrusage: u64 = 98;
pub const SYS_sysinfo: u64 = 99;

// 系统
// TODO(TRACK-1475D8): Phase N — implement times(2)
pub const SYS_times: u64 = 100;

// 用户/组
pub const SYS_getuid: u64 = 102;
pub const SYS_getgid: u64 = 104;
pub const SYS_setuid: u64 = 105;
pub const SYS_setgid: u64 = 106;
pub const SYS_geteuid: u64 = 107;
pub const SYS_getegid: u64 = 108;

pub const SYS_seteuid: u64 = 113;
pub const SYS_setegid: u64 = 114;
pub const SYS_setreuid: u64 = 115;
pub const SYS_setregid: u64 = 116;

// 进程组
pub const SYS_getppid: u64 = 110;
pub const SYS_getpgid: u64 = 111;
pub const SYS_setsid: u64 = 112;
pub const SYS_getsid: u64 = 156;
pub const SYS_setpgid: u64 = 157;

// 进程调度
pub const SYS_getpriority: u64 = 140;
pub const SYS_setpriority: u64 = 141;

// 文件同步
pub const SYS_sync: u64 = 162;
pub const SYS_fsync: u64 = 170;

// 挂载
pub const SYS_mount: u64 = 165;
pub const SYS_umount2: u64 = 166;

// 其他 POSIX
pub const SYS_gettid: u64 = 186;
pub const SYS_time: u64 = 201;
pub const SYS_clock_gettime: u64 = 228;
pub const SYS_exit_group: u64 = 231;
pub const SYS_tgkill: u64 = 234;

// 同步
pub const SYS_futex: u64 = 202;
// CPU 亲和性 (Linux 兼容号)
pub const SYS_sched_setaffinity: u64 = 203;
pub const SYS_sched_getaffinity: u64 = 204;

// 事件轮询
pub const SYS_epoll_create: u64 = 213;
pub const SYS_epoll_ctl: u64 = 233;
pub const SYS_epoll_wait: u64 = 232;

// eventfd / signalfd / timerfd (Linux x86_64 标准编号)
pub const SYS_eventfd: u64 = 284;
pub const SYS_eventfd2: u64 = 290;
pub const SYS_signalfd: u64 = 282;
pub const SYS_signalfd4: u64 = 289;
pub const SYS_timerfd_create: u64 = 283;
pub const SYS_timerfd_settime: u64 = 286;
pub const SYS_timerfd_gettime: u64 = 287;

// ==================== Credo 私有 syscall (400+ 不与 POSIX 冲突) ====================

pub const SYS_CREDO_LOGIN: u64 = 400;
pub const SYS_CREDO_LOGOUT: u64 = 401;
pub const SYS_CREDO_CREATE_IDENTITY: u64 = 402;
pub const SYS_CREDO_DELETE_IDENTITY: u64 = 403;
pub const SYS_CREDO_IDENTITY_INFO: u64 = 404;
pub const SYS_CREDO_CHANGE_PASSWORD: u64 = 405;
pub const SYS_CREDO_VERIFY_PASSWORD: u64 = 406;
pub const SYS_CREDO_CREATE_FIRST: u64 = 407;
pub const SYS_CREDO_GRANT: u64 = 411;
pub const SYS_CREDO_REVOKE: u64 = 412;
pub const SYS_CREDO_CHECK_CAP: u64 = 413;
pub const SYS_CREDO_GET_CAPS: u64 = 414;
pub const SYS_CREDO_GET_PWM: u64 = 415;
pub const SYS_CREDO_SET_PWM: u64 = 416;
pub const SYS_CREDO_DISK_LIST: u64 = 420;
pub const SYS_CREDO_DISK_INFO: u64 = 421;
pub const SYS_CREDO_DISK_FORMAT: u64 = 422;
pub const SYS_CREDO_DISK_PARTITION: u64 = 423;
pub const SYS_CREDO_DISK_INSTALL: u64 = 424;
pub const SYS_CREDO_FAT_FORMAT: u64 = 425;
pub const SYS_CREDO_PROC_LIST: u64 = 430;
pub const SYS_CREDO_PROC_SETPRI: u64 = 431;
pub const SYS_CREDO_PROC_SLEEP: u64 = 432;
pub const SYS_CREDO_GETHOSTNAME: u64 = 433;
pub const SYS_CREDO_SETHOSTNAME: u64 = 434;
pub const SYS_CREDO_BOOT_CHECK: u64 = 435;
pub const SYS_CREDO_REBOOT: u64 = 436;
pub const SYS_CREDO_HOTPLUG_STATUS: u64 = 437;
pub const SYS_CREDO_PROC_CPUTIME: u64 = 438;

// ==================== 帧缓冲设备 ====================
pub const SYS_FB_OPEN: u64 = 450;
pub const SYS_FB_MMAP: u64 = 451;
pub const SYS_FB_RELEASE: u64 = 452;

// ============================================================================
// QueenX 原生 syscall 编号 (500+)
//
// 遵循 queenx-naming-standpoint.md:
//   500-599 : 进程 / 内存 / 文件基础
//   600-699 : 网络 / IPC
//   700-799 : 设备 / 系统
//   800-899 : 扩展
//
// 编号原则:
//   - 不抄任何 OS 编号
//   - 按功能分区, 每区留扩展空间
//   - 0-299 保留给未来 linuxulator (与 Linux 1:1 映射)
// ============================================================================

// ---------- 500-509: Core I/O ----------
pub const QX_EXIT: u64 = 501;
pub const QX_WRITE: u64 = 502;
pub const QX_READ: u64 = 503;
pub const QX_OPEN: u64 = 504;
pub const QX_CLOSE: u64 = 505;
pub const QX_STAT: u64 = 506;
pub const QX_FSTAT: u64 = 507;
pub const QX_LSTAT: u64 = 508;
pub const QX_LSEEK: u64 = 509;

// ---------- 510-519: 内存管理 ----------
pub const QX_MMAP: u64 = 510;
pub const QX_BRK: u64 = 511;
pub const QX_MPROTECT: u64 = 512;
pub const QX_MUNMAP: u64 = 513;
pub const QX_MREMAP: u64 = 514;
// 515-519: reserved (madvise, mlock, munlock, mlockall, munlockall)

// ---------- 520-539: 进程管理 ----------
pub const QX_GETPID: u64 = 520;
pub const QX_FORK: u64 = 521;
pub const QX_EXECVE: u64 = 522;
pub const QX_CLONE: u64 = 523;
pub const QX_WAIT4: u64 = 524;
pub const QX_EXIT_GROUP: u64 = 525;
pub const QX_GETPPID: u64 = 526;
pub const QX_GETTID: u64 = 527;
pub const QX_GETPGID: u64 = 528;
pub const QX_SETPGID: u64 = 529;
pub const QX_GETSID: u64 = 530;
pub const QX_SETSID: u64 = 531;
pub const QX_NICE: u64 = 532;
pub const QX_SCHED_YIELD: u64 = 533;
pub const QX_SCHED_SETAFFINITY: u64 = 534;
pub const QX_SCHED_GETAFFINITY: u64 = 535;
pub const QX_GETPRIORITY: u64 = 536;
pub const QX_SETPRIORITY: u64 = 537;
pub const QX_TCGETPGRP: u64 = 538;
pub const QX_TCSETPGRP: u64 = 539;

// ---------- 540-559: 信号 ----------
pub const QX_RT_SIGACTION: u64 = 540;
pub const QX_RT_SIGPROCMASK: u64 = 541;
pub const QX_RT_SIGRETURN: u64 = 542;
pub const QX_KILL: u64 = 543;
pub const QX_TGKILL: u64 = 544;
// 545-559: 保留 (tkill, sigaltstack, rt_sigsuspend, ...)  // syscall 编号预留
pub const QX_TKILL: u64 = 545;
// P1-I-45: 接线 sigaltstack 替代栈系统调用
pub const QX_SIGALTSTACK: u64 = 546;

// ---------- 560-579: 文件系统操作 ----------
pub const QX_MKDIR: u64 = 560;
pub const QX_RMDIR: u64 = 561;
pub const QX_RENAME: u64 = 562;
pub const QX_LINK: u64 = 563;
pub const QX_UNLINK: u64 = 564;
pub const QX_SYMLINK: u64 = 565;
pub const QX_READLINK: u64 = 566;
pub const QX_CHMOD: u64 = 567;
pub const QX_FCHMOD: u64 = 568;
pub const QX_CHOWN: u64 = 569;
pub const QX_FCHOWN: u64 = 570;
pub const QX_FCHMODAT: u64 = 570; // fchmodat 映射到 FCHOWN 空间, 实际由 dispatch 区分
pub const QX_UMASK: u64 = 571;
pub const QX_ACCESS: u64 = 572;
pub const QX_TRUNCATE: u64 = 573;
pub const QX_FTRUNCATE: u64 = 574;
pub const QX_GETDENTS: u64 = 575;
pub const QX_GETCWD: u64 = 576;
pub const QX_CHDIR: u64 = 577;
pub const QX_CREAT: u64 = 578;
pub const QX_PIPE: u64 = 579;
pub const QX_PIPE2: u64 = 579; // pipe2 映射到 PIPE, flags 差异由 libc 处理

// ---------- 580-589: FD / 同步 / 挂载 ----------
pub const QX_DUP: u64 = 580;
pub const QX_DUP2: u64 = 581;
pub const QX_DUP3: u64 = 581; // dup3 映射到 DUP2, flags 差异由 libc 处理
pub const QX_FCNTL: u64 = 582;
pub const QX_IOCTL: u64 = 583;
pub const QX_SYNC: u64 = 584;
pub const QX_FSYNC: u64 = 585;
pub const QX_MOUNT: u64 = 586;
pub const QX_UMOUNT2: u64 = 587;
pub const QX_POLL: u64 = 588;
pub const QX_SELECT: u64 = 589;

// ---------- 590-599: 身份 + 文件锁 ----------
pub const QX_FLOCK: u64 = 590;
pub const QX_GETUID: u64 = 591;
pub const QX_GETGID: u64 = 592;
pub const QX_SETUID: u64 = 593;
pub const QX_SETGID: u64 = 594;
pub const QX_GETEUID: u64 = 595;
pub const QX_GETEGID: u64 = 596;
pub const QX_SETEUID: u64 = 597;
pub const QX_SETEGID: u64 = 598;
pub const QX_SETREUID: u64 = 599;
// QX_SETREGID 映射到 QX_SETREUID, 由 dispatch 区分

// ---------- 600-619: 网络 ----------
pub const QX_SOCKET: u64 = 600;
pub const QX_SOCKETPAIR: u64 = 600; // socketpair 映射到 SOCKET, 由 dispatch 区分
pub const QX_BIND: u64 = 601;
pub const QX_LISTEN: u64 = 602;
pub const QX_ACCEPT: u64 = 603;
pub const QX_CONNECT: u64 = 604;
pub const QX_SENDTO: u64 = 605;
pub const QX_RECVFROM: u64 = 606;
pub const QX_SENDMSG: u64 = 607;
pub const QX_RECVMSG: u64 = 608;
pub const QX_SHUTDOWN: u64 = 609;
pub const QX_SETSOCKOPT: u64 = 610;
pub const QX_GETSOCKOPT: u64 = 611;
pub const QX_GETSOCKNAME: u64 = 612;
pub const QX_GETPEERNAME: u64 = 613;
// 614-619: 保留 (socketpair, ...)  // syscall 编号预留

// ---------- 620-639: 同步 / IPC ----------
pub const QX_FUTEX: u64 = 620;
pub const QX_EPOLL_CREATE: u64 = 621;
pub const QX_EPOLL_CTL: u64 = 622;
pub const QX_EPOLL_WAIT: u64 = 623;
pub const QX_EVENTFD: u64 = 624;
pub const QX_EVENTFD2: u64 = 625;
pub const QX_SIGNALFD: u64 = 626;
pub const QX_SIGNALFD4: u64 = 627;
pub const QX_TIMERFD_CREATE: u64 = 628;
pub const QX_TIMERFD_SETTIME: u64 = 629;
pub const QX_TIMERFD_GETTIME: u64 = 630;
// 631-639: 保留 (msgqueue, shm, sem)  // syscall 编号预留

// ---------- 640-649: inotify ----------
pub const QX_INOTIFY_INIT1: u64 = 640;
pub const QX_INOTIFY_ADD_WATCH: u64 = 641;
pub const QX_INOTIFY_RM_WATCH: u64 = 642;

// ---------- 650-659: sendfile / splice ----------  // 高效拷贝/拼接 syscall
pub const QX_SENDFILE: u64 = 650;
pub const QX_SPLICE: u64 = 651;

// ---------- 700-709: 系统信息 ----------
pub const QX_UNAME: u64 = 700;
pub const QX_SYSINFO: u64 = 701;
pub const QX_GETRLIMIT: u64 = 702;
pub const QX_SETRLIMIT: u64 = 703;
pub const QX_GETRUSAGE: u64 = 704;

// ---------- 710-719: 时间 ----------
pub const QX_CLOCK_GETTIME: u64 = 710;
pub const QX_GETTIMEOFDAY: u64 = 711;
// 712: 保留 (settimeofday)  // syscall 编号预留
pub const QX_CLOCK_SETTIME: u64 = 712; // reserved
pub const QX_NANOSLEEP: u64 = 713;
pub const QX_ALARM: u64 = 714;
pub const QX_GETITIMER: u64 = 715;
pub const QX_SETITIMER: u64 = 716;
pub const QX_TIME: u64 = 717;
pub const QX_TIMES: u64 = 718;

// ---------- 720-729: 设备 ----------
pub const QX_FB_OPEN: u64 = 720;
pub const QX_FB_MMAP: u64 = 721;
pub const QX_FB_RELEASE: u64 = 722;

// ---------- 730-739: 设备固件加载 ----------
pub const QX_FW_LOAD: u64 = 730;
pub const QX_FW_GET: u64 = 731;
pub const QX_FW_GET_INFO: u64 = 732;
pub const QX_FW_DETACH: u64 = 733;

// ---------- 740-745: POSIX Timer ----------
/// 创建 per-process 定时器 (timer_create)
pub const QX_TIMER_CREATE: u64 = 740;
/// 启动 / 调整 / 停止定时器 (timer_settime)
pub const QX_TIMER_SETTIME: u64 = 741;
/// 查询定时器剩余时间 (timer_gettime)
pub const QX_TIMER_GETTIME: u64 = 742;
/// 释放定时器 (timer_delete)
pub const QX_TIMER_DELETE: u64 = 743;
/// 返回补打次数 (timer_getoverrun)
pub const QX_TIMER_GETOVERRUN: u64 = 744;
/// 时钟分辨率 (clock_getres)
pub const QX_CLOCK_GETRES: u64 = 745;

// ---------- 746-747: 熵源 / Stack Canary (P1 #14) ----------
/// 从内核熵源填充用户 buffer (Linux getrandom 兼容)
pub const QX_GETRANDOM: u64 = 746;
/// 读取当前进程 8 字节 stack canary (低字节恒为 0)
pub const QX_GET_CANARY: u64 = 747;

// ---------- 760-765: 内存建议与锁定 (madvise / mlock, P1 #15) ----------
/// 设置内存区域访问模式建议 (madvise)
pub const QX_MADVISE: u64 = 760;
/// 锁定 [addr, addr+len) 物理页禁止换出 (mlock)
pub const QX_MLOCK: u64 = 761;
/// 解除锁定 (munlock)
pub const QX_MUNLOCK: u64 = 762;
/// 进程级锁定所有/未来映射 (mlockall)
pub const QX_MLOCKALL: u64 = 763;
/// 解除进程级所有锁定 (munlockall)
pub const QX_MUNLOCKALL: u64 = 764;
/// 查询每页驻留性 (mincore)
pub const QX_MINCORE: u64 = 765;

// ---------- 800-809: 内核调试 / 跟踪 (ftrace / KGDB) ----------
/// 启用 ftrace 全局开关
pub const QX_FTRACE_ENABLE: u64 = 800;
/// 禁用 ftrace 全局开关
pub const QX_FTRACE_DISABLE: u64 = 801;
/// 从 ftrace ring buffer 读取一条事件到用户缓冲
pub const QX_FTRACE_READ: u64 = 802;
/// 查询 ftrace 状态 (event_count / overflow_count)
pub const QX_FTRACE_STAT: u64 = 803;
/// KGDB 主动断点 (用户态调试器触发)
pub const QX_KGDB_ENTER: u64 = 804;

// ==================== C7: Seccomp / prctl ====================

/// seccomp — 安装 Seccomp 过滤器
pub const QX_SECCOMP: u64 = 805;
/// prctl — 进程控制 (Seccomp/no_new_privs 子集)
pub const QX_PRCTL: u64 = 806;

// ==================== C5: 路由表 ====================

/// route_add — 添加路由条目
pub const QX_ROUTE_ADD: u64 = 807;
/// route_del — 删除路由条目
pub const QX_ROUTE_DEL: u64 = 808;
/// route_query — 查询路由 (最长前缀匹配)
pub const QX_ROUTE_QUERY: u64 = 809;

// ==================== C5: Netfilter ====================

/// nf_add_rule — 添加 Netfilter 规则
pub const QX_NF_ADD_RULE: u64 = 810;
/// nf_del_rule — 删除 Netfilter 规则
pub const QX_NF_DEL_RULE: u64 = 811;

// ==================== C4: io_uring ====================

/// io_uring_setup — 创建 io_uring 实例
pub const QX_IO_URING_SETUP: u64 = 812;
/// io_uring_enter — 提交/等待完成
pub const QX_IO_URING_ENTER: u64 = 813;
/// io_uring_register — 注册缓冲区/文件
pub const QX_IO_URING_REGISTER: u64 = 814;
/// io_uring_submit_sqe — 提交单个 SQE (简化版)
pub const QX_IO_URING_SUBMIT: u64 = 815;

// ==================== D1: Namespace ====================

/// unshare — 取消共享指定 namespace
pub const QX_UNSHARE: u64 = 820;
/// setns — 切换到指定 namespace
pub const QX_SETNS: u64 = 821;

// ==================== D2: cgroup ====================

/// cgroup_create — 创建子 cgroup
pub const QX_CGROUP_CREATE: u64 = 830;
/// cgroup_destroy — 删除 cgroup
pub const QX_CGROUP_DESTROY: u64 = 831;
/// cgroup_attach — 将进程迁移到 cgroup
pub const QX_CGROUP_ATTACH: u64 = 832;
/// cgroup_set_limit — 设置 cgroup 资源限制
pub const QX_CGROUP_SET_LIMIT: u64 = 833;
/// cgroup_get_stat — 获取 cgroup 统计信息
pub const QX_CGROUP_GET_STAT: u64 = 834;

// ==================== D3: NUMA ====================

/// get_mempolicy — 获取 NUMA 内存策略
pub const QX_GET_MEMPOLICY: u64 = 840;
/// set_mempolicy — 设置 NUMA 内存策略
pub const QX_SET_MEMPOLICY: u64 = 841;
/// migrate_pages — 迁移进程页面到目标节点
pub const QX_MIGRATE_PAGES: u64 = 842;
/// getcpu — 获取当前 CPU 和 NUMA 节点
pub const QX_GETCPU: u64 = 843;

// ==================== D4: eBPF ====================

/// bpf — BPF 系统调用多路复用
pub const QX_BPF: u64 = 850;

// ==================== D5: 电源管理 ====================

/// pm — 电源管理系统调用
pub const QX_PM: u64 = 860;

// ==================== D6: 安全启动 + TPM ====================

/// secure_boot — 安全启动系统调用
pub const QX_SECURE_BOOT: u64 = 870;

/// tpm — TPM 系统调用
pub const QX_TPM: u64 = 871;

// ==================== D7: Shadow Stack (CET) ====================

/// cet — CET/Shadow Stack 系统调用
pub const QX_CET: u64 = 880;

// ==================== D8: Tickless (NO_HZ) ====================  // 动态时钟节拍模式

/// tickless — Tickless 系统调用
pub const QX_TICKLESS: u64 = 881;

// ==================== D9: NTP/PTP 时钟同步 ====================

/// timesync — 时间同步系统调用
pub const QX_TIMESYNC: u64 = 882;

// ==================== D10: kexec ====================

/// kexec — 直接内核引导系统调用
pub const QX_KEXEC: u64 = 883;

// ==================== D11: UEFI ====================

/// uefi — UEFI 运行时服务系统调用
pub const QX_UEFI: u64 = 884;

// ==================== POSIX errno (使用 Linux 风格: 返回值 = -errno) ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Errno {
    EPERM = 1,
    ENOENT = 2,
    ESRCH = 3,
    EINTR = 4,
    EIO = 5,
    ENXIO = 6,
    E2BIG = 7,
    ENOEXEC = 8,
    EBADF = 9,
    ECHILD = 10,
    EAGAIN = 11,
    ENOMEM = 12,
    EACCES = 13,
    EFAULT = 14,
    ENOTBLK = 15,
    EBUSY = 16,
    EEXIST = 17,
    EXDEV = 18,
    ENODEV = 19,
    ENOTDIR = 20,
    EISDIR = 21,
    EINVAL = 22,
    ENFILE = 23,
    EMFILE = 24,
    ENOTTY = 25,
    ETXTBSY = 26,
    EFBIG = 27,
    ENOSPC = 28,
    ESPIPE = 29,
    EROFS = 30,
    EMLINK = 31,
    EPIPE = 32,
    EDOM = 33,
    ERANGE = 34,
    EDEADLK = 35,
    ENAMETOOLONG = 36,
    ENOLCK = 37,
    ENOSYS = 38,
    ENOTEMPTY = 39,
    ELOOP = 40,
    EWOULDBLOCK = 41,
    ENOMSG = 42,
    EIDRM = 43,
    ENOSTR = 60,
    ENODATA = 61,
    ETIME = 62,
    ENOSR = 63,
    ENONET = 64,
    EPROTO = 71,
    EBADMSG = 74,
    EOVERFLOW = 75,
    ENOTSOCK = 88,
    EDESTADDRREQ = 89,
    EMSGSIZE = 90,
    EPROTOTYPE = 91,
    ENOPROTOOPT = 92,
    EPROTONOSUPPORT = 93,
    ESOCKTNOSUPPORT = 94,
    ENOTSUP = 95,
    EPFNOSUPPORT = 96,
    EAFNOSUPPORT = 97,
    EADDRINUSE = 98,
    EADDRNOTAVAIL = 99,
    ENETDOWN = 100,
    ENETUNREACH = 101,
    ENETRESET = 102,
    ECONNABORTED = 103,
    ECONNRESET = 104,
    ENOBUFS = 105,
    EISCONN = 106,
    ENOTCONN = 107,
    ESHUTDOWN = 108,
    ETIMEDOUT = 110,
    ECONNREFUSED = 111,
    EHOSTDOWN = 112,
    EHOSTUNREACH = 113,
    EALREADY = 114,
    EINPROGRESS = 115,
}

impl Errno {
    /// 返回 POSIX errno 数值 (正整数)
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    pub const fn as_ret(self) -> i64 {
        -(self as i64)
    }

    /// 从负返回值恢复 Errno
    ///
    /// 输入: framework 层返回的负错误码 (如 -ENOMEM)
    /// 输出: 对应的 Errno 枚举值
    pub fn from_ret(ret: i64) -> Self {
        let errno = (-ret) as u64;
        match errno {
            1 => Self::EPERM,
            2 => Self::ENOENT,
            3 => Self::ESRCH,
            4 => Self::EINTR,
            5 => Self::EIO,
            6 => Self::ENXIO,
            7 => Self::E2BIG,
            8 => Self::ENOEXEC,
            9 => Self::EBADF,
            10 => Self::ECHILD,
            11 => Self::EAGAIN,
            12 => Self::ENOMEM,
            13 => Self::EACCES,
            14 => Self::EFAULT,
            16 => Self::EBUSY,
            17 => Self::EEXIST,
            18 => Self::EXDEV,
            19 => Self::ENODEV,
            20 => Self::ENOTDIR,
            21 => Self::EISDIR,
            22 => Self::EINVAL,
            23 => Self::ENFILE,
            24 => Self::EMFILE,
            25 => Self::ENOTTY,
            27 => Self::EFBIG,
            28 => Self::ENOSPC,
            29 => Self::ESPIPE,
            30 => Self::EROFS,
            31 => Self::EMLINK,
            32 => Self::EPIPE,
            33 => Self::EDOM,
            34 => Self::ERANGE,
            35 => Self::EDEADLK,
            36 => Self::ENAMETOOLONG,
            38 => Self::ENOSYS,
            39 => Self::ENOTEMPTY,
            40 => Self::ELOOP,
            _ => Self::EINVAL, // 未知错误码回退到 EINVAL
        }
    }
}

// ==================== 错误码转换 (兼容旧的 SyscallError 语义) ====================

#[deprecated(note = "use Errno::ENOENT.as_ret() instead")]
pub type SyscallError = Errno;

#[allow(deprecated)]
impl SyscallError {
    #[allow(non_upper_case_globals)]
    pub const E_PERM: Self = Self::EPERM;
    pub const E_NOTFOUND: Self = Self::ENOENT;
    pub const E_NOSYS: Self = Self::ENOSYS;
    pub const E_INTR: Self = Self::EINTR;
    pub const E_IO: Self = Self::EIO;
    pub const E_NOEXEC: Self = Self::ENOEXEC;
    pub const E_BADFD: Self = Self::EBADF;
    pub const E_CHILD: Self = Self::ECHILD;
    pub const E_AGAIN: Self = Self::EAGAIN;
    pub const E_NOMEM: Self = Self::ENOMEM;
    pub const E_ACCES: Self = Self::EACCES;
    pub const E_FAULT: Self = Self::EFAULT;
    pub const E_BUSY: Self = Self::EBUSY;
    pub const E_EXIST: Self = Self::EEXIST;
    pub const E_NOTDIR: Self = Self::ENOTDIR;
    pub const E_ISDIR: Self = Self::EISDIR;
    pub const E_INVAL: Self = Self::EINVAL;
    pub const E_NOSPC: Self = Self::ENOSPC;
    pub const E_ROFS: Self = Self::EROFS;
    pub const E_RANGE: Self = Self::ERANGE;
    pub const E_NAMETOOLONG: Self = Self::ENAMETOOLONG;
    pub const E_NOTEMPTY: Self = Self::ENOTEMPTY;
    pub const E_AUTH_INVALID: Self = Self::EPERM;
    pub const E_AUTH_NOTFOUND: Self = Self::ENOENT;
    pub const E_AUTH_DISABLED: Self = Self::EPERM;
    pub const E_AUTH_EXPIRED: Self = Self::EPERM;
    pub const E_AUTH_PWERR: Self = Self::EACCES;
    pub const E_AUTH_CAP: Self = Self::EACCES;
    pub const E_AUTH_DENY: Self = Self::EACCES;

    pub fn as_i64(self) -> i64 {
        -(self as i64)
    }

    pub fn from_i64(code: i64) -> Option<Self> {
        let v = (-code) as i32;
        match v {
            1 => Some(Self::EPERM),
            2 => Some(Self::ENOENT),
            3 => Some(Self::ESRCH),
            4 => Some(Self::EINTR),
            5 => Some(Self::EIO),
            8 => Some(Self::ENOEXEC),
            9 => Some(Self::EBADF),
            10 => Some(Self::ECHILD),
            11 => Some(Self::EAGAIN),
            12 => Some(Self::ENOMEM),
            13 => Some(Self::EACCES),
            14 => Some(Self::EFAULT),
            16 => Some(Self::EBUSY),
            17 => Some(Self::EEXIST),
            20 => Some(Self::ENOTDIR),
            21 => Some(Self::EISDIR),
            22 => Some(Self::EINVAL),
            28 => Some(Self::ENOSPC),
            30 => Some(Self::EROFS),
            34 => Some(Self::ERANGE),
            36 => Some(Self::ENAMETOOLONG),
            38 => Some(Self::ENOSYS),
            39 => Some(Self::ENOTEMPTY),
            _ => None,
        }
    }
}

impl core::fmt::Display for Errno {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EPERM => write!(f, "Operation not permitted"),
            Self::ENOENT => write!(f, "No such file or directory"),
            Self::ESRCH => write!(f, "No such process"),
            Self::EINTR => write!(f, "Interrupted system call"),
            Self::EIO => write!(f, "I/O error"),
            Self::ENOEXEC => write!(f, "Exec format error"),
            Self::EBADF => write!(f, "Bad file descriptor"),
            Self::ECHILD => write!(f, "No child processes"),
            Self::EAGAIN => write!(f, "Resource temporarily unavailable"),
            Self::ENOMEM => write!(f, "Cannot allocate memory"),
            Self::EACCES => write!(f, "Permission denied"),
            Self::EFAULT => write!(f, "Bad address"),
            Self::EBUSY => write!(f, "Device or resource busy"),
            Self::EEXIST => write!(f, "File exists"),
            Self::ENOTDIR => write!(f, "Not a directory"),
            Self::EISDIR => write!(f, "Is a directory"),
            Self::EINVAL => write!(f, "Invalid argument"),
            Self::ENOSPC => write!(f, "No space left on device"),
            Self::EROFS => write!(f, "Read-only file system"),
            Self::ERANGE => write!(f, "Result too large"),
            Self::ENAMETOOLONG => write!(f, "File name too long"),
            Self::ENOTEMPTY => write!(f, "Directory not empty"),
            Self::ENOSYS => write!(f, "Function not implemented"),
            _ => write!(f, "Error {}", -(*self as i64)),
        }
    }
}

// ==================== 辅助类型 ====================

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SyscallRegs {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

pub type SyscallHandler = fn(u64, u64, u64, u64) -> i64;
pub type SyscallResult<T> = Result<T, Errno>;
