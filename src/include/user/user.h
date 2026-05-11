/**
 * AntX 用户态运行时头文件
 *
 * 为 Ring 3 用户程序提供系统调用包装和基础类型定义。
 * 所有 syscall 通过 int 0x80 触发, 参数使用标准 x86_64 调用约定:
 *   rax=syscall号, rdi=arg1, rsi=arg2, rdx=arg3, r10=arg4, r8=arg5
 */

#ifndef USER_USER_H
#define USER_USER_H

typedef unsigned long long uint64_t;
typedef long long int64_t;
typedef unsigned int uint32_t;
typedef int int32_t;
typedef unsigned short uint16_t;
typedef unsigned char uint8_t;
typedef uint64_t size_t;

#define NULL ((void *)0)

#define MAX_ARGS 32
#define MAX_LINE 256

/* ── 系统调用号 ──────────────────────────────────────── */
#define SYS_PROC_GETID    4
#define SYS_PROC_GETPWID  6
#define SYS_PROC_EXEC     1
#define SYS_PROC_EXIT     2
#define SYS_PROC_YIELD    9

#define SYS_FS_OPEN       20
#define SYS_FS_CLOSE      21
#define SYS_FS_READ       22
#define SYS_FS_WRITE      23
#define SYS_FS_SEEK       24
#define SYS_FS_STAT       25
#define SYS_FS_UNLINK     29
#define SYS_FS_RENAME     30
#define SYS_FS_MKDIR      31
#define SYS_FS_RMDIR      32
#define SYS_FS_READDIR    33
#define SYS_FS_SYNC       102
#define SYS_FS_MOUNT      111
#define SYS_FS_UNMOUNT    112

#define SYS_ENV_GETCWD    100
#define SYS_ENV_CHDIR     101

#define SYS_AUTH_LOGIN    40
#define SYS_AUTH_LOGOUT   41
#define SYS_AUTH_CREATE   43
#define SYS_AUTH_CHANGEPW 48
#define SYS_AUTH_VERIFY   49
#define SYS_AUTH_CREATE_FIRST 50

#define SYS_GETHOSTNAME   108
#define SYS_SETHOSTNAME   109
#define SYS_REBOOT        103
#define SYS_TIME          104
#define SYS_DISK_LIST     113
#define SYS_DISK_INFO     114
#define SYS_DISK_FORMAT   115
#define SYS_DISK_PARTITION 116
#define SYS_DISK_INSTALL_GRUB 117

/* HVFS 打开标志 */
#define HVFS_O_RDONLY 0
#define HVFS_O_WRONLY 1
#define HVFS_O_RDWR   2
#define HVFS_O_CREAT  0100
#define HVFS_O_TRUNC  01000

/* 文件类型 */
#define HVFS_TYPE_FILE 0
#define HVFS_TYPE_DIR  1
#define HVFS_TYPE_DEV  2

/* 目录条目 (与内核 VfsDirent #[repr(C)] 对齐) */
struct user_dirent {
    uint32_t inode;
    uint8_t  file_type;
    char     name[256];
};

/* 磁盘信息 (用于安装向导) */
struct user_disk_info {
    uint32_t disk_id;
    uint32_t present;
    uint32_t total_sectors;
    uint32_t sectors;
    char     model[64];
};

/* ── 原始 syscall 宏 ─────────────────────────────────── */
static inline int64_t syscall0(uint64_t num) {
    int64_t ret;
    __asm__ volatile ("int $0x80" : "=a"(ret) : "a"(num) : "memory");
    return ret;
}

static inline int64_t syscall1(uint64_t num, uint64_t a1) {
    int64_t ret;
    __asm__ volatile ("int $0x80" : "=a"(ret) : "a"(num), "D"(a1) : "memory");
    return ret;
}

static inline int64_t syscall2(uint64_t num, uint64_t a1, uint64_t a2) {
    int64_t ret;
    __asm__ volatile ("int $0x80" : "=a"(ret) : "a"(num), "D"(a1), "S"(a2) : "memory");
    return ret;
}

static inline int64_t syscall3(uint64_t num, uint64_t a1, uint64_t a2, uint64_t a3) {
    int64_t ret;
    __asm__ volatile ("int $0x80" : "=a"(ret) : "a"(num), "D"(a1), "S"(a2), "d"(a3) : "memory");
    return ret;
}

static inline int64_t syscall4(uint64_t num, uint64_t a1, uint64_t a2, uint64_t a3, uint64_t a4) {
    int64_t ret;
    register uint64_t r10 __asm__("r10") = a4;
    __asm__ volatile ("int $0x80" : "=a"(ret) : "a"(num), "D"(a1), "S"(a2), "d"(a3), "r"(r10) : "memory");
    return ret;
}

/* ── 进程管理 ─────────────────────────────────────────── */
static inline int64_t sys_proc_getid(void)      { return syscall0(SYS_PROC_GETID); }
static inline int64_t sys_proc_get_pwid(void)   { return syscall0(SYS_PROC_GETPWID); }
static inline int64_t sys_proc_execute(const char *path, char *const argv[], char *const envp[]) {
    return syscall3(SYS_PROC_EXEC, (uint64_t)path, (uint64_t)argv, (uint64_t)envp);
}
static inline void sys_proc_exit(int32_t code)  { syscall1(SYS_PROC_EXIT, (uint64_t)code); }
static inline void sys_proc_yield_cpu(void)     { syscall0(SYS_PROC_YIELD); }

/* ── 文件系统 ─────────────────────────────────────────── */
static inline int64_t sys_fs_open(const char *path, int32_t flags, int32_t mode) {
    return syscall3(SYS_FS_OPEN, (uint64_t)path, (uint64_t)flags, (uint64_t)mode);
}
static inline int64_t sys_fs_close(int32_t fd)  { return syscall1(SYS_FS_CLOSE, (uint64_t)fd); }
static inline int64_t sys_fs_read(int32_t fd, void *buf, uint64_t count) {
    return syscall3(SYS_FS_READ, (uint64_t)fd, (uint64_t)buf, count);
}
static inline int64_t sys_fs_write(int32_t fd, const void *buf, uint64_t count) {
    return syscall3(SYS_FS_WRITE, (uint64_t)fd, (uint64_t)buf, count);
}
static inline int64_t sys_fs_make_dir(const char *path, int32_t mode) {
    return syscall2(SYS_FS_MKDIR, (uint64_t)path, (uint64_t)mode);
}
static inline int64_t sys_fs_remove_dir(const char *path) {
    return syscall1(SYS_FS_RMDIR, (uint64_t)path);
}
static inline int64_t sys_fs_delete(const char *path) {
    return syscall1(SYS_FS_UNLINK, (uint64_t)path);
}
static inline int64_t sys_fs_sync_all(void)     { return syscall0(SYS_FS_SYNC); }
static inline int64_t sys_fs_mount(const char *source, const char *target, const char *fstype, const char *options) {
    return syscall4(SYS_FS_MOUNT, (uint64_t)source, (uint64_t)target, (uint64_t)fstype, (uint64_t)options);
}
static inline int64_t sys_fs_unmount(const char *target) {
    return syscall1(SYS_FS_UNMOUNT, (uint64_t)target);
}
static inline int64_t sys_fs_read_dir(uint32_t fd, struct user_dirent *entry) {
    return syscall2(SYS_FS_READDIR, (uint64_t)fd, (uint64_t)entry);
}

/* ── 环境 / 工作目录 ─────────────────────────────────── */
static inline int64_t sys_env_get_current_dir(char *buf, uint64_t size) {
    return syscall2(SYS_ENV_GETCWD, (uint64_t)buf, size);
}
static inline int64_t sys_env_set_current_dir(const char *path) {
    return syscall1(SYS_ENV_CHDIR, (uint64_t)path);
}

/* ── PWID 认证 ────────────────────────────────────────── */
static inline int64_t sys_auth_authenticate(const char *note, const char *password) {
    return syscall2(SYS_AUTH_LOGIN, (uint64_t)note, (uint64_t)password);
}
static inline void sys_auth_invalidate_session(void) { syscall0(SYS_AUTH_LOGOUT); }
static inline int64_t sys_auth_create_pwid(const char *password, const char *note, uint8_t level) {
    return syscall3(SYS_AUTH_CREATE, (uint64_t)password, (uint64_t)note, (uint64_t)level);
}
static inline int64_t sys_auth_change_pwid_password(const char *old_pw, const char *new_pw) {
    return syscall2(SYS_AUTH_CHANGEPW, (uint64_t)old_pw, (uint64_t)new_pw);
}
static inline int64_t sys_auth_verify_pwid_password(const char *password) {
    return syscall1(SYS_AUTH_VERIFY, (uint64_t)password);
}
static inline int64_t sys_auth_create_first(const char *password) {
    return syscall1(SYS_AUTH_CREATE_FIRST, (uint64_t)password);
}

/* ── 主机名 ───────────────────────────────────────────── */
static inline int64_t sys_get_hostname(char *buf, uint64_t size) {
    return syscall2(SYS_GETHOSTNAME, (uint64_t)buf, size);
}
static inline int64_t sys_set_hostname(const char *name, uint64_t len) {
    return syscall2(SYS_SETHOSTNAME, (uint64_t)name, len);
}

/* ── user 库函数声明 ─────────────────────────────────── */
void user_print(const char *s);
void user_println(const char *s);
void user_print_char(char c);
void user_print_hex(uint64_t val);
void user_print_dec(int64_t val);
int  user_read_line(char *buf, int max);
char **user_parse_args(char *line, int *argc);
int  user_strcmp(const char *s1, const char *s2);
int  user_strlen(const char *s);
void user_strcpy(char *dest, const char *src);
void user_memcpy(void *dest, const void *src, int n);
void user_memset(void *s, int c, int n);
int  user_open(const char *path, int flags, int mode);
int  user_close(int fd);
int  user_read(int fd, void *buf, int count);
int  user_write(int fd, const void *buf, int count);
int  user_mkdir(const char *path, int mode);
int  user_rmdir(const char *path);
int  user_unlink(const char *path);
int  user_getcwd(char *buf, int size);
int  user_chdir(const char *path);
int  user_auth_login(const char *password, const char *note);
void user_auth_logout(void);
int  user_auth_create_pwid(const char *password, const char *note, uint8_t level);
int  user_auth_change_password(const char *old_pw, const char *new_pw);
int  user_auth_verify_password(const char *password);
int  user_auth_create_first(const char *password);
int  user_get_hostname(char *buf, int size);
int  user_set_hostname(const char *name, int len);
void user_delay(int seconds);
void user_sync(void);
int  user_mount(const char *source, const char *target, const char *fstype, const char *options);
int  user_unmount(const char *target);

/* ── 磁盘管理 (内联 syscall 包装) ───────────────────── */
static inline int64_t sys_disk_list(uint64_t *disks, uint32_t max_count) {
    return syscall2(SYS_DISK_LIST, (uint64_t)disks, (uint64_t)max_count);
}
static inline int64_t sys_disk_info(uint32_t disk_id, struct user_disk_info *info) {
    return syscall2(SYS_DISK_INFO, (uint64_t)disk_id, (uint64_t)info);
}
static inline int64_t sys_disk_format(uint32_t disk_id, const char *fstype) {
    return syscall2(SYS_DISK_FORMAT, (uint64_t)disk_id, (uint64_t)fstype);
}
static inline int64_t sys_disk_partition(uint32_t disk_id, uint64_t total_sectors) {
    return syscall2(SYS_DISK_PARTITION, (uint64_t)disk_id, total_sectors);
}
static inline int64_t sys_disk_install_grub(uint32_t disk_id) {
    return syscall1(SYS_DISK_INSTALL_GRUB, (uint64_t)disk_id);
}
static inline int64_t sys_reboot(int32_t cmd) {
    return syscall1(SYS_REBOOT, (uint64_t)cmd);
}

#endif /* USER_USER_H */
