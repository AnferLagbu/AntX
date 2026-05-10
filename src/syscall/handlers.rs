/// 系统调用处理函数实现
/// 
/// 实现所有系统调用的业务逻辑，包括：
/// - 进程管理 (create, exec, exit, wait, etc.)
/// - 文件系统操作 (open, close, read, write, etc.)
/// - 认证/权限控制 (login, logout, create, delete, etc.)
/// - 内存管理 (brk, mmap, munmap, mprotect)
/// - IPC (pipe)
/// - 环境变量和系统信息
/// - 磁盘管理

use crate::syscall::types::*;

// FFI 声明 - 从 C 代码导入的函数
extern "C" {
    /// 键盘输入检测
    fn keyboard_has_data() -> bool;
    /// 键盘字符读取
    fn keyboard_get_char() -> i32;
    /// 串口数据检测
    fn serial_has_data(com: i32) -> bool;
    /// 串口字符读取
    fn serial_getc(com: i32) -> i32;
    /// 串口写入
    fn serial_write(com: i32, buf: *const core::ffi::c_void, count: u64);
    /// ATA 磁盘存在检测
    fn ata_disk_present(drive: u8) -> bool;
}

/// 创建新进程
/// 
/// # Safety
/// 需要当前进程的有效上下文
pub unsafe fn sys_proc_create() -> i64 {
    use crate::proc::ffi::*;
    
    let parent = process_get_current();
    let pwid = if !parent.is_null() { (*parent).pwid } else { 0 };
    
    let child_pid = process_create(core::ptr::null(), 0, pwid);
    if child_pid == 0 {
        return SyscallError::E_NOMEM.as_i64();
    }
    
    let child = process_find_by_pid(child_pid);
    if child.is_null() {
        return SyscallError::E_NOMEM.as_i64();
    }
    
    if !parent.is_null() {
        (*child).parent_pid = (*parent).pid;
        (*child).parent = parent;
        (*child).pwid = pwid;
        
        if (*parent).cr3 != 0 {
            // 使用 VMM 创建用户页表
            let cr3 = crate::mm::vmm_create_user_page_table();
            if cr3 == 0 {
                (*child).state = 3; // PROC_ZOMBIE
                return SyscallError::E_NOMEM.as_i64();
            }
            (*child).cr3 = cr3;
        }
    }
    
    (*child).state = 2; // PROC_READY
    scheduler_add(child_pid);
    
    (*child).pid as i64
}

/// 执行程序 (exec)
/// 
/// # Arguments
/// * `path` - 可执行文件路径
/// * `argv` - 参数数组
/// * `envp` - 环境变量数组
pub unsafe fn sys_proc_exec(path: *const i8, argv: *const *const i8, envp: *const *const i8) -> i64 {
    use crate::proc::ffi::*;
    
    let proc = process_get_current();
    if proc.is_null() {
        return SyscallError::E_PERM.as_i64();
    }
    
    let pwid = (*proc).pwid;
    
    // 计算 argc 和 envc
    let mut argc: u32 = 0;
    if !argv.is_null() {
        while !(*argv.add(argc as usize)).is_null() {
            argc += 1;
        }
    }
    
    let mut envc: u32 = 0;
    if !envp.is_null() {
        while !(*envp.add(envc as usize)).is_null() {
            envc += 1;
        }
    }
    
    // 加载 ELF 文件
    let pid = user_proc_load_elf(path, pwid);
    if pid < 0 {
        return SyscallError::E_NOTFOUND.as_i64();
    }
    
    // 设置 argv/envp 到用户栈
    if argc > 0 {
        user_proc_setup_argv(pid as u32, argv, argc, envp, envc);
    }
    
    // 添加到调度器
    sched_add_internal(pid as u32);
    
    pid
}

/// 退出进程
pub unsafe fn sys_proc_exit(status: i32) -> i64 {
    use crate::proc::ffi::*;
    process_exit(status as u32);
    0
}

/// 等待子进程退出
pub unsafe fn sys_proc_wait(pid: i32, status: *mut i32) -> i64 {
    use crate::proc::ffi::*;
    
    let parent = process_get_current();
    if parent.is_null() {
        return SyscallError::E_PERM.as_i64();
    }
    
    let mut child: *mut ProcessStruct = core::ptr::null_mut();
    
    if pid == -1 {
        // 等待任意子进程
        for i in 0..256 { // MAX_PROCESSES
            let p = process_find_by_pid((i + 1) as u32);
            if !p.is_null() && (*p).parent_pid == (*parent).pid && (*p).state == 4 { // PROC_ZOMBIE
                child = p;
                break;
            }
        }
    } else {
        // 等待指定 PID
        child = process_find_by_pid(pid as u32);
        if child.is_null() || (*child).parent_pid != (*parent).pid {
            return SyscallError::E_CHILD.as_i64();
        }
    }
    
    if child.is_null() {
        return SyscallError::E_CHILD.as_i64();
    }
    
    // 如果子进程还未退出，阻塞等待
    if (*child).state != 4 { // PROC_ZOMBIE
        (*parent).state = 5; // PROC_BLOCKED
        scheduler_yield();
    }
    
    // 返回退出状态
    if !status.is_null() {
        *status = (*child).exit_code as i32;
    }
    
    let child_pid = (*child).pid as i32;
    (*child).state = 0; // PROC_NEW
    (*child).pid = 0;
    
    child_pid
}

/// 获取当前进程 ID
pub unsafe fn sys_proc_getid() -> i64 {
    use crate::proc::ffi::*;
    let proc = process_get_current();
    if proc.is_null() { 0 } else { (*proc).pid as i64 }
}

/// 获取父进程 ID
pub unsafe fn sys_proc_getppid() -> i64 {
    use crate::proc::ffi::*;
    let proc = process_get_current();
    if proc.is_null() { 0 } else { (*proc).parent_pid as i64 }
}

/// 获取当前进程 PWID
pub unsafe fn sys_proc_getpwid() -> i64 {
    use crate::proc::ffi::*;
    let proc = process_get_current();
    if proc.is_null() { 0 } else { (*proc).pwid as i64 }
}

/// 设置进程 PWID (需要 SYS_ADMIN 权限)
pub unsafe fn sys_proc_setpwid(pwid: u64) -> i64 {
    use crate::proc::ffi::*;
    use crate::pwid::ffi::*;
    
    let proc = process_get_current();
    if proc.is_null() {
        return SyscallError::E_PERM.as_i64();
    }
    
    // 检查权限
    if pwid_has_cap_raw(pwid_get_current(), 0, 9) == 0 { // CAP_DOMAIN_SYS_ADMIN
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    
    (*proc).pwid = pwid;
    0
}

/// 让出 CPU
pub unsafe fn sys_proc_yield() -> i64 {
    scheduler_yield();
    0
}

// ==================== 文件系统 syscall ====================

/// 打开文件
pub unsafe fn sys_fs_open(path: *const i8, flags: i32, _mode: i32) -> i64 {
    if path.is_null() {
        return SyscallError::E_INVAL.as_i64();
    }
    
    let pwid = crate::pwid::ffi::pwid_get_current();
    crate::fs::vfs::ffi::vfs_open(path, flags as u32, pwid)
}

/// 关闭文件
pub unsafe fn sys_fs_close(fd: i32) -> i64 {
    if fd < 0 {
        return SyscallError::E_BADFD.as_i64();
    }
    
    crate::fs::vfs::ffi::vfs_close(fd as u32)
}

/// 读取文件 (或 stdin)
pub unsafe fn sys_fs_read(fd: i32, buf: *mut u8, count: u64) -> i64 {
    // stdin (fd=0): 从键盘或串口读取
    if fd == 0 {
        if buf.is_null() || count == 0 {
            return -1;
        }
        
        let buffer = buf as *mut i8;
        let mut read_count: u64 = 0;
        
        while read_count < count {
            let mut c: i32 = -1;
            
            if keyboard_has_data() {
                c = keyboard_get_char();
            } else if serial_has_data(0) { // SERIAL_COM1
                c = serial_getc(0);
            }
            
            if c == -1 || c == 0 {
                if read_count > 0 { break; }
                core::arch::asm!("pause", options(nostack, nomem));
                continue;
            }
            
            *buffer.add(read_count as usize) = c as i8;
            read_count += 1;
            
            if c as i8 == b'\n' as i8 { break; }
        }
        
        return read_count as i64;
    }
    
    crate::fs::vfs::ffi::vfs_read(fd as u32, buf as *mut core::ffi::c_void, count)
}

/// 写入文件 (或 stdout/stderr)
pub unsafe fn sys_fs_write(fd: i32, buf: *const u8, count: u64) -> i64 {
    // stdout (fd=1) 或 stderr (fd=2): 输出到串口
    if fd == 1 || fd == 2 {
        serial_write(0, buf as *const core::ffi::c_void, count); // SERIAL_COM1
        return count as i64;
    }
    
    crate::fs::vfs::ffi::vfs_write(fd as u32, buf as *const core::ffi::c_void, count)
}

/// 文件定位
pub unsafe fn sys_fs_seek(fd: i32, offset: i64, whence: i32) -> i64 {
    crate::fs::vfs::ffi::vfs_seek(fd as u32, offset, whence)
}

/// 获取文件状态
pub unsafe fn sys_fs_stat(path: *const i8, stat_buf: *mut core::ffi::c_void) -> i64 {
    let pwid = crate::pwid::ffi::pwid_get_current();
    crate::fs::vfs::ffi::vfs_stat(path, stat_buf, pwid)
}

/// 修改文件权限
pub unsafe fn sys_fs_chmod(path: *const i8, mode: i32) -> i64 {
    let pwid = crate::pwid::ffi::pwid_get_current();
    crate::fs::vfs::ffi::vfs_chmod(path, mode as u16, pwid)
}

/// 修改文件所有者
pub unsafe fn sys_fs_chown(path: *const i8, owner_pwid: u64) -> i64 {
    let pwid = crate::pwid::ffi::pwid_get_current();
    crate::fs::vfs::ffi::vfs_chown(path, owner_pwid, pwid)
}

/// 删除文件
pub unsafe fn sys_fs_unlink(path: *const i8) -> i64 {
    let pwid = crate::pwid::ffi::pwid_get_current();
    crate::fs::vfs::ffi::vfs_unlink(path, pwid)
}

/// 重命名文件
pub unsafe fn sys_fs_rename(old_path: *const i8, new_path: *const i8) -> i64 {
    let pwid = crate::pwid::ffi::pwid_get_current();
    crate::fs::vfs::ffi::vfs_rename(old_path, new_path, pwid)
}

/// 创建目录
pub unsafe fn sys_fs_mkdir(path: *const i8, _mode: i32) -> i64 {
    if path.is_null() {
        return -1;
    }
    
    let pwid = crate::pwid::ffi::pwid_get_current();
    let pwid = if pwid == 0 { 0x0020F45A8B978417 } else { pwid };
    
    let result = crate::fs::vfs::ffi::vfs_mkdir(path, pwid);
    
    result
}

/// 删除目录
pub unsafe fn sys_fs_rmdir(path: *const i8) -> i64 {
    let pwid = crate::pwid::ffi::pwid_get_current();
    crate::fs::vfs::ffi::vfs_rmdir(path, pwid)
}

/// 读取目录项
pub unsafe fn sys_fs_readdir(fd: i32, dirent_buf: *mut core::ffi::c_void) -> i64 {
    crate::fs::vfs::ffi::vfs_readdir(fd as u32, dirent_buf)
}

// ==================== 认证/权限 syscall (PWID) ====================

/// 用户登录
pub unsafe fn sys_auth_login(password: *const i8, note: *const i8) -> i64 {
    use crate::pwid::ffi::*;
    
    let result = pwid_login(note, password);
    match result {
        0 => 0, // PWID_OK
        1 => SyscallError::E_AUTH_PWERR.as_i64(),   // PWID_ERR_PASSWORD
        2 => SyscallError::E_AUTH_NOTFOUND.as_i64(), // PWID_ERR_NOT_FOUND
        3 => SyscallError::E_AUTH_DISABLED.as_i64(), // PWID_ERR_DISABLED
        _ => SyscallError::E_AUTH_INVALID.as_i64(),
    }
}

/// 用户登出
pub unsafe fn sys_auth_logout() -> i64 {
    crate::pwid::ffi::pwid_logout();
    0
}

/// 权限提升 (创建 token)
pub unsafe fn sys_auth_elevate(_cmd_path: *const i8, _argv: *const *const i8) -> i64 {
    use crate::pwid::ffi::*;
    
    let current_pwid = pwid_get_current();
    if current_pwid == 0 {
        return SyscallError::E_AUTH_NOTFOUND.as_i64();
    }
    
    let entry = pwid_find(current_pwid);
    if entry.is_null() {
        return SyscallError::E_AUTH_NOTFOUND.as_i64();
    }
    
    // 创建 token (SYSTEM 域, 全部权限, 3600秒有效期, 1次使用)
    let token = pwid_create_token(current_pwid, 1, 0xFFFFFFFFFFFFFFFFu64, 3600, 1); // CAP_DOMAIN_SYSTEM
    if token < 0 {
        return SyscallError::E_PERM.as_i64();
    }
    
    token
}

/// 创建认证 token
pub unsafe fn sys_auth_token_create(holder: u64, domain: u16, caps: u64, 
                                     duration_secs: u64, max_uses: u32) -> i64 {
    use crate::pwid::ffi::*;
    
    let current_pwid = pwid_get_current();
    if current_pwid == 0 {
        return SyscallError::E_AUTH_NOTFOUND.as_i64();
    }
    
    if pwid_has_cap_raw(current_pwid, 0, 14) == 0 { // CAP_DOMAIN_TOKEN_ISSUE
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    
    pwid_create_token(holder, domain, caps, duration_secs, max_uses)
}

/// 使用 token
pub unsafe fn sys_auth_token_use(token_id: u64) -> i64 {
    crate::pwid::ffi::pwid_use_token_internal(token_id)
}

/// 撤销 token
pub unsafe fn sys_auth_token_revoke(token_id: u64) -> i64 {
    let current_pwid = crate::pwid::ffi::pwid_get_current();
    crate::pwid::ffi::pwid_revoke_token_internal(token_id, current_pwid)
}

/// 添加信任关系
pub unsafe fn sys_auth_trust_add(trusted: u64, trust_level: u8, 
                                  domain: u16, cap_mask: u64) -> i64 {
    use crate::pwid::ffi::*;
    
    let current_pwid = pwid_get_current();
    if current_pwid == 0 {
        return SyscallError::E_AUTH_NOTFOUND.as_i64();
    }
    
    if pwid_has_cap_raw(current_pwid, 0, 15) == 0 { // CAP_DOMAIN_TRUST_ADD
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    
    pwid_add_trust_relation(current_pwid, trusted, trust_level, domain, cap_mask)
}

/// 移除信任关系
pub unsafe fn sys_auth_trust_remove(trusted: u64, domain: u16) -> i64 {
    let current_pwid = crate::pwid::ffi::pwid_get_current();
    crate::pwid::ffi::pwid_remove_trust_internal(current_pwid, trusted, domain)
}

/// 权限检查
pub unsafe fn sys_auth_check(pwid: u64, owner_pwid: u64, access_type: u64, domain: u16) -> i64 {
    crate::pwid::ffi::pwid_enhanced_check(pwid, owner_pwid, access_type, domain)
}

/// 创建用户 (需要 SYS_ADMIN 权限)
pub unsafe fn sys_auth_create(password: *const i8, note: *const i8, level: u8) -> i64 {
    use crate::pwid::ffi::*;
    
    if pwid_has_cap_raw(pwid_get_current(), 0, 9) == 0 { // CAP_DOMAIN_SYS_ADMIN
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    
    let result = pwid_create_user(password, note, level);
    match result {
        0 => 0,                                    // PWID_OK
        3 => SyscallError::E_BUSY.as_i64(),       // PWID_ERR_FULL
        4 => SyscallError::E_EXIST.as_i64(),      // PWID_ERR_EXISTS
        _ => SyscallError::E_PERM.as_i64(),
    }
}

/// 创建用户 (带显式能力掩码)
pub unsafe fn sys_auth_create_with_caps(password: *const i8, note: *const i8, level: u8,
                                         caps_array: *const u64) -> i64 {
    use crate::pwid::ffi::*;
    
    if pwid_has_cap_raw(pwid_get_current(), 0, 9) == 0 { // CAP_DOMAIN_SYS_ADMIN
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    
    let result = pwid_create_user_with_caps(password, note, level, caps_array);
    match result {
        0 => 0,                                    // PWID_OK
        3 => SyscallError::E_BUSY.as_i64(),       // PWID_ERR_FULL
        4 => SyscallError::E_EXIST.as_i64(),      // PWID_ERR_EXISTS
        _ => SyscallError::E_PERM.as_i64(),
    }
}

/// 创建第一个用户 (root)
pub unsafe fn sys_auth_create_first(password: *const i8) -> i64 {
    use crate::pwid::ffi::*;
    
    if pwid_any_identity_exists() {
        return SyscallError::E_EXIST.as_i64();
    }
    
    let result = pwid_create_first_identity(password);
    if result == 0 {
        pwid_login("root\0".as_ptr() as *const i8, password);
        return 0;
    }
    
    SyscallError::E_PERM.as_i64()
}

/// 删除用户 (需要 SYS_ADMIN 权限)
pub unsafe fn sys_auth_delete(target_pwid: u64) -> i64 {
    use crate::pwid::ffi::*;
    
    if pwid_has_cap_raw(pwid_get_current(), 0, 9) == 0 { // CAP_DOMAIN_SYS_ADMIN
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    
    let result = pwid_delete(target_pwid);
    match result {
        0 => 0,                                    // PWID_OK
        2 => SyscallError::E_AUTH_NOTFOUND.as_i64(), // PWID_ERR_NOT_FOUND
        _ => SyscallError::E_PERM.as_i64(),
    }
}

/// 列出所有用户 (需要 SYS_ADMIN 权限)
pub unsafe fn sys_auth_list() -> i64 {
    use crate::pwid::ffi::*;
    
    if pwid_has_cap_raw(pwid_get_current(), 0, 9) == 0 { // CAP_DOMAIN_SYS_ADMIN
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    
    pwid_list_all();
    0
}

/// 获取用户信息
pub unsafe fn sys_auth_info(target_pwid: u64) -> i64 {
    use crate::pwid::ffi::*;
    
    let entry = pwid_find(target_pwid);
    if entry.is_null() {
        return SyscallError::E_AUTH_NOTFOUND.as_i64();
    }
    
    (*entry).level as i64
}

/// 修改密码
pub unsafe fn sys_auth_changepw(old_pw: *const i8, new_pw: *const i8) -> i64 {
    use crate::pwid::ffi::*;
    
    let current_pwid = pwid_get_current();
    let result = pwid_change_password(current_pwid, old_pw, new_pw);
    match result {
        0 => 0,                                   // PWID_OK
        1 => SyscallError::E_AUTH_PWERR.as_i64(), // PWID_ERR_PASSWORD
        _ => SyscallError::E_PERM.as_i64(),
    }
}

/// 验证密码
pub unsafe fn sys_auth_verify(password: *const i8) -> i64 {
    use crate::pwid::ffi::*;
    
    let current_pwid = pwid_get_current();
    let result = pwid_verify_password(current_pwid, password);
    if result == 0 { 0 } else { SyscallError::E_AUTH_PWERR.as_i64() } // PWID_OK
}

// ==================== 内存管理 syscall ====================

/// brk 系统调用 (未实现)
pub unsafe fn sys_mem_brk(_addr: *mut u8) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

/// mmap 系统调用 (未实现)
pub unsafe fn sys_mem_map(_addr: *mut u8, _len: u64, _prot: i32, 
                           _flags: i32, _fd: i32, _offset: i64) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

/// munmap 系统调用 (未实现)
pub unsafe fn sys_mem_unmap(_addr: *mut u8, _len: u64) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

/// mprotect 系统调用 (未实现)
pub unsafe fn sys_mem_protect(_addr: *mut u8, _len: u64, _prot: i32) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

// ==================== IPC syscall ====================

/// pipe 系统调用 (未实现)
pub unsafe fn sys_ipc_pipe(_fd: *mut [i32; 2]) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

// ==================== 环境/系统信息 syscall ====================

/// 获取当前工作目录
pub unsafe fn sys_env_getcwd(buf: *mut i8, size: u64) -> i64 {
    if buf.is_null() || size == 0 {
        return SyscallError::E_INVAL.as_i64();
    }
    
    let cwd = crate::fs::vfs::ffi::vfs_get_cwd();
    let mut len: u64 = 0;
    while *cwd.add(len as usize) != 0 && len < size - 1 {
        *buf.add(len as usize) = *cwd.add(len as usize);
        len += 1;
    }
    *buf.add(len as usize) = 0; // null terminator
    
    len as i64
}

/// 改变工作目录
pub unsafe fn sys_env_chdir(path: *const i8) -> i64 {
    if path.is_null() {
        return SyscallError::E_INVAL.as_i64();
    }
    
    let pwid = crate::pwid::ffi::pwid_get_current();
    
    // TODO: 使用正确的 vfs_stat 类型
    // let st: VfsStat = ...;
    // if vfs_stat(path, &st, pwid) != 0 {
    //     return SyscallError::E_NOTFOUND.as_i64();
    // }
    // 
    // if st.type != VFS_TYPE_DIR {
    //     return SyscallError::E_NOTDIR.as_i64();
    // }
    
    crate::fs::vfs::ffi::vfs_set_cwd(path);
    0
}

/// 同步文件系统
pub unsafe fn sys_fs_sync() -> i64 {
    crate::fs::vfs::ffi::vfs_sync()
}

/// 重启系统
pub unsafe fn sys_reboot(cmd: i32) -> i64 {
    if cmd == 0 {
        klog_kern("Rebooting...");
        
        crate::fs::vfs::ffi::vfs_sync();
        
        // 延迟等待
        for _ in 0..100000000 {
            core::arch::asm!("nop", options(nostack, nomem));
        }
        
        // 发送重启命令到 8042 端口
        core::arch::asm!(
            "mov $0x64, %rax",
            "mov $0x2000, %rdx",
            "out %al, %dx",
            "1: hlt",
            "jmp 1b",
            options(nostack)
        );
        
        return 0;
    }
    
    SyscallError::E_PERM.as_i64()
}

/// 获取时间 (未实现)
pub unsafe fn sys_time() -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

/// 获取系统信息 (未实现)
pub unsafe fn sys_info(_info_buf: *mut u8) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

/// 获取主机名
pub unsafe fn sys_gethostname(buf: *mut i8, size: u64) -> i64 {
    if buf.is_null() || size == 0 {
        return SyscallError::E_INVAL.as_i64();
    }
    
    // TODO: 使用全局 hostname 变量
    // let mut len: u64 = 0;
    // while SYS_HOSTNAME[len] != 0 && len < size - 1 {
    //     buf[len] = SYS_HOSTNAME[len];
    //     len += 1;
    // }
    // buf[len] = 0;
    
    0
}

/// 设置主机名 (需要 SYS_ADMIN 权限)
pub unsafe fn sys_sethostname(name: *const i8, len: u64) -> i64 {
    use crate::pwid::ffi::*;
    
    if pwid_has_cap_raw(pwid_get_current(), 0, 9) == 0 { // CAP_DOMAIN_SYS_ADMIN
        return SyscallError::E_AUTH_CAP.as_i64();
    }
    
    if name.is_null() || len == 0 || len > 63 {
        return SyscallError::E_INVAL.as_i64();
    }
    
    // 验证主机名字符
    for i in 0..len {
        let c = *name.add(i as usize);
        if !((c >= b'a' && c <= b'z') || (c >= b'A' && c <= b'Z') ||
             (c >= b'0' && c <= b'9') || c == b'-' || c == b'.') {
            return SyscallError::E_INVAL.as_i64();
        }
    }
    
    // TODO: 设置全局 hostname
    // for i in 0..len {
    //     SYS_HOSTNAME[i] = name[i];
    // }
    // SYS_HOSTNAME[len] = 0;
    
    0
}

/// 启动检查
pub unsafe fn sys_boot_check(check_type: i32) -> i64 {
    match check_type {
        0 => {
            // 检查是否存在任何身份
            if crate::pwid::ffi::pwid_any_identity_exists() { 1 } else { 0 }
        }
        1 => {
            // 检查安装标记文件
            let fd = sys_fs_open("/.antx_installed\0".as_ptr() as *const i8, 0, 0); // HVFS_O_RDONLY
            if fd >= 0 {
                sys_fs_close(fd);
                1
            } else {
                0
            }
        }
        _ => -1,
    }
}

// ==================== 设备 I/O syscall ====================

/// ioctl (未实现)
pub unsafe fn sys_dev_ioctl(_fd: i32, _cmd: i32, _arg: *mut u8) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

/// 设备读取 (未实现)
pub unsafe fn sys_dev_read(_fd: i32, _buf: *mut u8, _n: u64) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

/// 设备写入 (未实现)
pub unsafe fn sys_dev_write(_fd: i32, _buf: *const u8, _n: u64) -> i64 {
    SyscallError::E_NOSYS.as_i64()
}

// ==================== 文件系统挂载/卸载 ====================

/// 挂载文件系统
pub unsafe fn sys_fs_mount(_source: *const i8, target: *const i8, fstype: *const i8, 
                            _options: *const i8) -> i64 {
    use crate::pwid::ffi::*;
    
    if target.is_null() || fstype.is_null() {
        return SyscallError::E_INVAL.as_i64();
    }
    
    let proc = crate::proc::ffi::process_get_current();
    let pwid = if !proc.is_null() { (*proc).pwid } else { 0 };
    
    if pwid_has_cap_raw(pwid, 0, 9) == 0 { // CAP_DOMAIN_SYS_ADMIN
        return SyscallError::E_PERM.as_i64();
    }
    
    let result = crate::fs::vfs::ffi::vfs_mount(target, fstype);
    if result == 0 { 0 } else { SyscallError::E_IO.as_i64() }
}

/// 卸载文件系统
pub unsafe fn sys_fs_unmount(target: *const i8) -> i64 {
    use crate::pwid::ffi::*;
    
    if target.is_null() {
        return SyscallError::E_INVAL.as_i64();
    }
    
    let proc = crate::proc::ffi::process_get_current();
    let pwid = if !proc.is_null() { (*proc).pwid } else { 0 };
    
    if pwid_has_cap_raw(pwid, 0, 9) == 0 { // CAP_DOMAIN_SYS_ADMIN
        return SyscallError::E_PERM.as_i64();
    }
    
    // TODO: 实现 unmount
    SyscallError::E_NOSYS.as_i64()
}

// ==================== 磁盘管理 syscall ====================

/// 列出磁盘
pub unsafe fn sys_disk_list(disks: *mut u64, max_count: u32) -> i64 {
    if disks.is_null() || max_count == 0 {
        return SyscallError::E_INVAL.as_i64();
    }
    
    let mut count: u32 = 0;
    
    for drive in 0..4u8 {
        if count >= max_count { break; }
        
        if ata_disk_present(drive) {
            *disks.add(count as usize) = drive as u64;
            count += 1;
        }
    }
    
    count as i64
}

/// 获取磁盘信息
pub unsafe fn sys_disk_info(disk_id: u32, info: *mut u8) -> i64 {
    if info.is_null() {
        return SyscallError::E_INVAL.as_i64();
    }
    
    if disk_id >= 4 {
        return SyscallError::E_NOTFOUND.as_i64();
    }
    
    // TODO: 使用正确的 DiskInfo 结构体
    // let dinfo = info as *mut DiskInfo;
    // dinfo.disk_id = disk_id;
    // dinfo.present = ata_disk_present(disk_id as u8);
    // ...
    
    0
}

/// 格式化磁盘
pub unsafe fn sys_disk_format(disk_id: u32, fstype: *const i8) -> i64 {
    use crate::pwid::ffi::*;
    
    if fstype.is_null() {
        return SyscallError::E_INVAL.as_i64();
    }
    
    if disk_id >= 4 {
        return SyscallError::E_NOTFOUND.as_i64();
    }
    
    let proc = crate::proc::ffi::process_get_current();
    let pwid = if !proc.is_null() { (*proc).pwid } else { 0 };
    
    if pwid_has_cap_raw(pwid, 0, 11) == 0 { // CAP_DOMAIN_DEVICE_DISK
        return SyscallError::E_PERM.as_i64();
    }
    
    if !ata_disk_present(disk_id as u8) {
        return SyscallError::E_NOTFOUND.as_i64();
    }
    
    // 比较 fs 类型
    let hvfs_str = "hvfs\0";
    let diskfs_str = "diskfs\0";
    
    // 简单字符串比较
    let is_hvfs = {
        let mut i = 0;
        loop {
            let c = *fstype.add(i);
            if c == 0 { break; }
            if c != *hvfs_str.as_ptr().add(i) { break; }
            i += 1;
        }
        *fstype.add(i) == 0 && *hvfs_str.as_ptr().add(i) == 0
    };
    
    let is_diskfs = {
        let mut i = 0;
        loop {
            let c = *fstype.add(i);
            if c == 0 { break; }
            if c != *diskfs_str.as_ptr().add(i) { break; }
            i += 1;
        }
        *fstype.add(i) == 0 && *diskfs_str.as_ptr().add(i) == 0
    };
    
    if is_hvfs || is_diskfs {
        let result = crate::fs::hvfs::ffi::hvfs_format();
        if result == 0 { 0 } else { SyscallError::E_IO.as_i64() }
    } else {
        SyscallError::E_INVAL.as_i64()
    }
}

/// 分区磁盘
pub unsafe fn sys_disk_partition(disk_id: u32, total_sectors: u64) -> i64 {
    use crate::pwid::ffi::*;
    
    if disk_id >= 4 {
        return SyscallError::E_NOTFOUND.as_i64();
    }
    
    let proc = crate::proc::ffi::process_get_current();
    let pwid = if !proc.is_null() { (*proc).pwid } else { 0 };
    
    if pwid_has_cap_raw(pwid, 0, 11) == 0 { // CAP_DOMAIN_DEVICE_DISK
        return SyscallError::E_PERM.as_i64();
    }
    
    if !ata_disk_present(disk_id as u8) {
        return SyscallError::E_NOTFOUND.as_i64();
    }
    
    // TODO: 调用 grub_create_partition_table
    // let result = grub_create_partition_table(disk_id as u8, total_sectors);
    // if result == 0 { 0 } else { SyscallError::E_IO.as_i64() }
    
    SyscallError::E_NOSYS.as_i64()
}

/// 安装 GRUB
pub unsafe fn sys_disk_install_grub(disk_id: u32) -> i64 {
    use crate::pwid::ffi::*;
    
    if disk_id >= 4 {
        return SyscallError::E_NOTFOUND.as_i64();
    }
    
    let proc = crate::proc::ffi::process_get_current();
    let pwid = if !proc.is_null() { (*proc).pwid } else { 0 };
    
    if pwid_has_cap_raw(pwid, 0, 11) == 0 { // CAP_DOMAIN_DEVICE_DISK
        return SyscallError::E_PERM.as_i64();
    }
    
    if !ata_disk_present(disk_id as u8) {
        return SyscallError::E_NOTFOUND.as_i64();
    }
    
    // TODO: 调用 grub_install_mbr
    // let result = grub_install_mbr(disk_id as u8);
    // if result == 0 { 0 } else { SyscallError::E_IO.as_i64() }
    
    SyscallError::E_NOSYS.as_i64()
}
