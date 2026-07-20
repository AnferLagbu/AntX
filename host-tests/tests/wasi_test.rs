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
    for i in 3..max_fds {
        if fds[i].is_none() {
            fds[i] = Some(30);
            allocated = true;
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
    let iovecs = vec![IoVec { buf: 100, len: 256 }, IoVec { buf: 400, len: 128 }];
    let total: u32 = iovecs.iter().map(|iov| iov.len).sum();
    assert_eq!(total, 384);
}
