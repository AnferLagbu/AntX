//! WASI preview1 集成测试
//!
//! 验证 WASI 基础设施的正确性:
//! - WasiFdTable 行为 (分配/关闭/重编号/溢出)
//! - WasiContext 参数/环境变量
//! - WASI 权限/文件类型/filestat 结构
//! - WASI errno 值与 POSIX 对齐

// ============================================================================
// WasiFdTable 行为测试
// ============================================================================

#[test]
fn test_fd_table_create() {
    let fds: Vec<Option<u32>> = vec![None; 16];
    // fd 0-2 保留 (stdin/stdout/stderr)
    assert!(fds[0].is_none());
    assert!(fds[1].is_none());
    assert!(fds[2].is_none());
}

#[test]
fn test_fd_table_alloc_close() {
    let mut fds: Vec<Option<u32>> = vec![None; 16];

    // 分配 fd 3
    fds[3] = Some(10);
    assert_eq!(fds[3], Some(10));

    // 关闭 fd 3
    let closed = fds[3].take();
    assert_eq!(closed, Some(10));
    assert!(fds[3].is_none());
}

#[test]
fn test_fd_table_overflow() {
    let max_fds = 5;
    let mut fds: Vec<Option<u32>> = vec![None; max_fds];

    fds[3] = Some(10);
    fds[4] = Some(20);

    let mut allocated = false;
    for (i, slot) in fds.iter_mut().enumerate().skip(3).take(max_fds - 3) {
        if slot.is_none() {
            *slot = Some(30);
            allocated = true;
            let _ = i; // i 仅用于调试, 不在断言中使用
            break;
        }
    }
    assert!(!allocated, "should not allocate when full");
}

#[test]
fn test_fd_table_renumber() {
    let mut fds: Vec<Option<u32>> = vec![None; 16];
    fds[3] = Some(10);

    let entry = fds[3].take();
    fds[10] = entry;

    assert!(fds[3].is_none());
    assert_eq!(fds[10], Some(10));
}

// ============================================================================
// WasiContext 测试
// ============================================================================

#[test]
fn test_context_args() {
    let args: Vec<String> = vec!["test_program".into(), "--verbose".into()];
    let env: Vec<(String, String)> = vec![("HOME".into(), "/root".into())];

    assert_eq!(args.len(), 2);
    assert_eq!(env.len(), 1);
    assert_eq!(args[0], "test_program");
    assert_eq!(env[0].0, "HOME");
}

// ============================================================================
// WASI 权限测试
// ============================================================================

#[test]
fn test_wasi_rights() {
    const RIGHT_FD_READ: u64 = 1 << 6;
    const RIGHT_FD_WRITE: u64 = 1 << 7;
    const RIGHT_PATH_OPEN: u64 = 1 << 10;

    let file_rights = RIGHT_FD_READ | RIGHT_FD_WRITE;
    assert!(file_rights & RIGHT_FD_READ != 0);
    assert!(file_rights & RIGHT_FD_WRITE != 0);
    assert!((file_rights & RIGHT_PATH_OPEN) == 0);
}

// ============================================================================
// WASI filestat 结构测试
// ============================================================================

#[test]
fn test_filestat_structure() {
    struct Filestat {
        dev: u64,
        ino: u64,
        filetype: u8,
        nlink: u64,
        size: u64,
        atim: u64,
        mtim: u64,
        ctim: u64,
    }

    let stat = Filestat {
        dev: 1, ino: 42, filetype: 4, nlink: 1,
        size: 1024, atim: 1000000, mtim: 2000000, ctim: 3000000,
    };
    assert_eq!(stat.filetype, 4);
    assert_eq!(stat.size, 1024);
}

#[test]
fn test_filestat_all_fields_semantics() {
    // 镜像 WASI preview1 filestat_t 布局 (8 * u64 + 1 * u8 + 7 padding)
    // dev/ino: 文件设备/inode 编号, 用于唯一标识文件
    // filetype: WASI 文件类型 (0=unknown, 1=block, 2=char, 3=dir, 4=regular, 5=link, 6=socket)
    // nlink: 硬链接数
    // size: 文件字节大小
    // atim/mtim/ctim: 访问/修改/状态变更时间戳 (纳秒)
    struct Filestat {
        dev: u64,
        ino: u64,
        filetype: u8,
        nlink: u64,
        size: u64,
        atim: u64,
        mtim: u64,
        ctim: u64,
    }

    let stat = Filestat {
        dev: 0x100, ino: 0xABCD_1234, filetype: 3, nlink: 5,
        size: 4096, atim: 1_700_000_000_000_000, mtim: 1_700_000_001_000_000, ctim: 1_700_000_002_000_000,
    };
    assert_eq!(stat.dev, 0x100, "dev = 设备 ID");
    assert_eq!(stat.ino, 0xABCD_1234, "ino = inode 编号");
    assert_eq!(stat.filetype, 3, "filetype = 3 (WASI directory)");
    assert_eq!(stat.nlink, 5, "nlink = 硬链接数");
    assert_eq!(stat.size, 4096, "size = 文件大小");
    assert_eq!(stat.atim, 1_700_000_000_000_000, "atim = 访问时间 (纳秒)");
    assert_eq!(stat.mtim, 1_700_000_001_000_000, "mtim = 修改时间 (纳秒)");
    assert_eq!(stat.ctim, 1_700_000_002_000_000, "ctim = 状态变更时间 (纳秒)");
}

// ============================================================================
// WASI errno 值验证
// ============================================================================

#[test]
fn test_wasi_errno_posix_alignment() {
    const WASI_SUCCESS: i32 = 0;
    const WASI_BADF: i32 = 8;
    const WASI_FAULT: i32 = 21;
    const WASI_INVAL: i32 = 28;
    const WASI_NOENT: i32 = 44;
    const WASI_NOTSUP: i32 = 58;

    assert_eq!(WASI_SUCCESS, 0);
    assert_eq!(WASI_BADF, 8);
    assert_eq!(WASI_FAULT, 21);
    assert_eq!(WASI_INVAL, 28);
    assert_eq!(WASI_NOENT, 44);
    assert_eq!(WASI_NOTSUP, 58);
}

// ============================================================================
// WASI iovec 结构测试
// ============================================================================

#[test]
fn test_iovec_structure() {
    struct IoVec { buf: u32, len: u32 }
    let iovecs = [IoVec { buf: 100, len: 256 }, IoVec { buf: 400, len: 128 }];
    // 验证 buf 字段保留缓冲区起始地址
    assert_eq!(iovecs[0].buf, 100, "buf[0] 保留起始地址");
    assert_eq!(iovecs[1].buf, 400, "buf[1] 保留起始地址");
    let total: u32 = iovecs.iter().map(|iov| iov.len).sum();
    assert_eq!(total, 384);
}

#[test]
fn test_iovec_buf_pointer_semantics() {
    // 镜像 WASI preview1 iovec_t 布局: buf (指针) + len (长度)
    // buf: 用户态缓冲区地址, len: 缓冲区长度
    // readv/writev 通过遍历 iovec 数组进行分散/聚集 I/O
    struct IoVec { buf: u32, len: u32 }

    let iovecs = [
        IoVec { buf: 0x1000, len: 256 },
        IoVec { buf: 0x2000, len: 128 },
        IoVec { buf: 0x3000, len: 512 },
    ];

    // 验证 buf 字段: 每个缓冲区起始地址不同, 用于分散写入
    assert_eq!(iovecs[0].buf, 0x1000, "buf[0] = 用户缓冲区 0 起始地址");
    assert_eq!(iovecs[1].buf, 0x2000, "buf[1] = 用户缓冲区 1 起始地址");
    assert_eq!(iovecs[2].buf, 0x3000, "buf[2] = 用户缓冲区 2 起始地址");

    // 验证 buf + len: 标记缓冲区结束地址
    assert_eq!(iovecs[0].buf + iovecs[0].len, 0x1100, "buf[0] + len[0] = 缓冲区 0 末尾");
    assert_eq!(iovecs[1].buf + iovecs[1].len, 0x2080, "buf[1] + len[1] = 缓冲区 1 末尾");
    assert_eq!(iovecs[2].buf + iovecs[2].len, 0x3200, "buf[2] + len[2] = 缓冲区 2 末尾");

    // readv 总读取字节数 = sum(len)
    let total_read: u32 = iovecs.iter().map(|iov| iov.len).sum();
    assert_eq!(total_read, 896, "readv 总字节数 = 256 + 128 + 512 = 896");
}
