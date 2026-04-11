#ifndef _USER_SYSCALL_H
#define _USER_SYSCALL_H

#include "types.h"

#define SYS_PROC_CREATE    0
#define SYS_PROC_EXECUTE   1
#define SYS_PROC_TERMINATE 2
#define SYS_PROC_WAITPID   3
#define SYS_PROC_GET_PID   4
#define SYS_PROC_GET_PARENT_PID 5
#define SYS_PROC_GET_PWID  6
#define SYS_PROC_SET_PWID  7
#define SYS_PROC_SET_PRIORITY 8
#define SYS_PROC_YIELD_CPU 9
#define SYS_PROC_SLEEP_MS  10

#define SYS_FS_OPEN        20
#define SYS_FS_CLOSE       21
#define SYS_FS_READ        22
#define SYS_FS_WRITE       23
#define SYS_FS_SEEK_OFFSET 24
#define SYS_FS_GET_STAT    25
#define SYS_FS_GET_STAT_FD 26
#define SYS_FS_SET_PERMISSIONS 27
#define SYS_FS_SET_OWNER   28
#define SYS_FS_DELETE      29
#define SYS_FS_RENAME      30
#define SYS_FS_MAKE_DIR    31
#define SYS_FS_REMOVE_DIR  32
#define SYS_FS_READ_DIR    33

#define SYS_AUTH_AUTHENTICATE 40
#define SYS_AUTH_INVALIDATE_SESSION 41
#define SYS_AUTH_ELEVATE_PRIVILEGES 42
#define SYS_AUTH_CREATE_PWID 43
#define SYS_AUTH_DELETE_PWID 44
#define SYS_AUTH_LIST_PWIDS 45
#define SYS_AUTH_GET_PWID_INFO 46
#define SYS_AUTH_SET_PWID_NOTE 47
#define SYS_AUTH_CHANGE_PWID_PASSWORD 48
#define SYS_AUTH_VERIFY_PWID_PASSWORD 49

#define SYS_ENV_GET_CURRENT_DIR 100
#define SYS_ENV_SET_CURRENT_DIR 101
#define SYS_FS_SYNC_ALL 102
#define SYS_GET_HOSTNAME 108
#define SYS_SET_HOSTNAME 109
#define SYS_BOOT_CHECK   110

static inline int64_t syscall0(uint64_t num) {
    int64_t ret;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(num)
        : "memory"
    );
    return ret;
}

static inline int64_t syscall1(uint64_t num, uint64_t arg0) {
    int64_t ret;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(num), "D"(arg0)
        : "memory"
    );
    return ret;
}

static inline int64_t syscall2(uint64_t num, uint64_t arg0, uint64_t arg1) {
    int64_t ret;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(num), "D"(arg0), "S"(arg1)
        : "memory"
    );
    return ret;
}

static inline int64_t syscall3(uint64_t num, uint64_t arg0, uint64_t arg1, uint64_t arg2) {
    int64_t ret;
    register uint64_t r10 __asm__("r10") = arg2;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(num), "D"(arg0), "S"(arg1), "r"(r10)
        : "memory"
    );
    return ret;
}

static inline int64_t syscall4(uint64_t num, uint64_t arg0, uint64_t arg1, uint64_t arg2, uint64_t arg3) {
    int64_t ret;
    register uint64_t r10 __asm__("r10") = arg2;
    register uint64_t r8 __asm__("r8") = arg3;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(num), "D"(arg0), "S"(arg1), "r"(r10), "r"(r8)
        : "memory"
    );
    return ret;
}

static inline int64_t sys_proc_exit(int status) {
    return syscall1(SYS_PROC_TERMINATE, status);
}

static inline int64_t sys_proc_get_pid(void) {
    return syscall0(SYS_PROC_GET_PID);
}

static inline int64_t sys_proc_get_pwid(void) {
    return syscall0(SYS_PROC_GET_PWID);
}

static inline int64_t sys_proc_yield_cpu(void) {
    return syscall0(SYS_PROC_YIELD_CPU);
}

static inline int64_t sys_proc_create(void) {
    return syscall0(SYS_PROC_CREATE);
}

static inline int64_t sys_proc_execute(const char *path, char *const argv[], char *const envp[]) {
    return syscall3(SYS_PROC_EXECUTE, (uint64_t)path, (uint64_t)argv, (uint64_t)envp);
}

static inline int64_t sys_proc_wait(int pid, int *status) {
    return syscall2(SYS_PROC_WAITPID, pid, (uint64_t)status);
}

static inline int64_t sys_fs_open(const char *path, int flags, int mode) {
    return syscall3(SYS_FS_OPEN, (uint64_t)path, flags, mode);
}

static inline int64_t sys_fs_close(int fd) {
    return syscall1(SYS_FS_CLOSE, fd);
}

static inline int64_t sys_fs_read(int fd, void *buf, int count) {
    return syscall3(SYS_FS_READ, fd, (uint64_t)buf, count);
}

static inline int64_t sys_fs_write(int fd, const void *buf, int count) {
    return syscall3(SYS_FS_WRITE, fd, (uint64_t)buf, count);
}

static inline int64_t sys_fs_make_dir(const char *path, int mode) {
    return syscall2(SYS_FS_MAKE_DIR, (uint64_t)path, mode);
}

static inline int64_t sys_fs_remove_dir(const char *path) {
    return syscall1(SYS_FS_REMOVE_DIR, (uint64_t)path);
}

static inline int64_t sys_fs_delete(const char *path) {
    return syscall1(SYS_FS_DELETE, (uint64_t)path);
}

static inline int64_t sys_fs_read_dir(int fd, void *dirent_buf) {
    return syscall2(SYS_FS_READ_DIR, fd, (uint64_t)dirent_buf);
}

static inline int64_t sys_env_get_current_dir(char *buf, int size) {
    return syscall2(SYS_ENV_GET_CURRENT_DIR, (uint64_t)buf, size);
}

static inline int64_t sys_env_set_current_dir(const char *path) {
    return syscall1(SYS_ENV_SET_CURRENT_DIR, (uint64_t)path);
}

static inline int64_t sys_auth_authenticate(const char *note, const char *password) {
    return syscall2(SYS_AUTH_AUTHENTICATE, (uint64_t)note, (uint64_t)password);
}

static inline int64_t sys_auth_invalidate_session(void) {
    return syscall0(SYS_AUTH_INVALIDATE_SESSION);
}

static inline int64_t sys_auth_create_pwid(const char *password, const char *note, uint8_t level) {
    return syscall3(SYS_AUTH_CREATE_PWID, (uint64_t)password, (uint64_t)note, level);
}

static inline int64_t sys_auth_change_pwid_password(const char *old_pw, const char *new_pw) {
    return syscall2(SYS_AUTH_CHANGE_PWID_PASSWORD, (uint64_t)old_pw, (uint64_t)new_pw);
}

static inline int64_t sys_auth_verify_pwid_password(const char *password) {
    return syscall1(SYS_AUTH_VERIFY_PWID_PASSWORD, (uint64_t)password);
}

static inline int64_t sys_get_hostname(char *buf, int size) {
    return syscall2(SYS_GET_HOSTNAME, (uint64_t)buf, size);
}

static inline int64_t sys_set_hostname(const char *name, int len) {
    return syscall2(SYS_SET_HOSTNAME, (uint64_t)name, len);
}

static inline int64_t sys_fs_sync_all(void) {
    return syscall0(SYS_FS_SYNC_ALL);
}

static inline int64_t sys_boot_check(int check_type) {
    return syscall1(SYS_BOOT_CHECK, check_type);
}

#define BOOT_CHECK_HAS_ROOT   0
#define BOOT_CHECK_INSTALLED  1

#endif
