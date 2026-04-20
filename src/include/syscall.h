#ifndef _SYSCALL_H
#define _SYSCALL_H

#include "types.h"

#define SYSCALL_INT        0x80

#define MAX_SYSCALLS       128

#define SYS_PROC_CREATE    0
#define SYS_PROC_EXEC      1
#define SYS_PROC_EXIT      2
#define SYS_PROC_WAIT      3
#define SYS_PROC_GETID     4
#define SYS_PROC_GETPPID   5
#define SYS_PROC_GETPWID   6
#define SYS_PROC_SETPWID   7
#define SYS_PROC_SETPRI    8
#define SYS_PROC_YIELD     9
#define SYS_PROC_SLEEP     10

#define SYS_FS_OPEN        20
#define SYS_FS_CLOSE       21
#define SYS_FS_READ        22
#define SYS_FS_WRITE       23
#define SYS_FS_SEEK        24
#define SYS_FS_STAT        25
#define SYS_FS_FSTAT       26
#define SYS_FS_CHMOD       27
#define SYS_FS_CHOWN       28
#define SYS_FS_UNLINK      29
#define SYS_FS_RENAME      30
#define SYS_FS_MKDIR       31
#define SYS_FS_RMDIR       32
#define SYS_FS_READDIR     33

#define SYS_AUTH_LOGIN     40
#define SYS_AUTH_LOGOUT    41
#define SYS_AUTH_ELEVATE   42
#define SYS_AUTH_CREATE    43
#define SYS_AUTH_DELETE    44
#define SYS_AUTH_LIST      45
#define SYS_AUTH_INFO      46
#define SYS_AUTH_SETNOTE   47
#define SYS_AUTH_CHANGEPW  48
#define SYS_AUTH_VERIFY    49
#define SYS_AUTH_CREATE_ORIGINAL_ROOT 50
#define SYS_AUTH_TOKEN_CREATE 51
#define SYS_AUTH_TOKEN_USE    52
#define SYS_AUTH_TOKEN_REVOKE 53
#define SYS_AUTH_TRUST_ADD    54
#define SYS_AUTH_TRUST_REMOVE 55
#define SYS_AUTH_CHECK        56

#define SYS_MEM_BRK        60
#define SYS_MEM_MAP        61
#define SYS_MEM_UNMAP      62
#define SYS_MEM_PROTECT    63

#define SYS_IPC_PIPE       80
#define SYS_IPC_SIGNAL     81
#define SYS_IPC_SHM_CREATE 82
#define SYS_IPC_SHM_ATTACH 83
#define SYS_IPC_SHM_DETACH 84
#define SYS_IPC_SHM_DESTROY 85
#define SYS_IPC_MSGQ_CREATE 86
#define SYS_IPC_MSGQ_SEND  87
#define SYS_IPC_MSGQ_RECV  88
#define SYS_IPC_MSGQ_DESTROY 89
#define SYS_IPC_SEM_CREATE 90
#define SYS_IPC_SEM_WAIT   91
#define SYS_IPC_SEM_POST   92
#define SYS_IPC_SEM_DESTROY 93
#define SYS_NET_SOCKET     81
#define SYS_NET_BIND       82
#define SYS_NET_LISTEN     83
#define SYS_NET_ACCEPT     84
#define SYS_NET_CONNECT    85
#define SYS_NET_SEND       86
#define SYS_NET_RECV       87
#define SYS_NET_SHUTDOWN   88

#define SYS_ENV_GETCWD     100
#define SYS_ENV_CHDIR      101
#define SYS_FS_SYNC        102
#define SYS_REBOOT         103
#define SYS_TIME           104
#define SYS_INFO           105
#define SYS_ENV_GETVAR     106
#define SYS_ENV_SETVAR     107
#define SYS_GETHOSTNAME    108
#define SYS_SETHOSTNAME    109
#define SYS_BOOT_CHECK     110

#define SYS_DEV_IOCTL      120
#define SYS_DEV_READ       121
#define SYS_DEV_WRITE      122

#define E_PERM             (-1)
#define E_NOTFOUND         (-2)
#define E_INTR             (-4)
#define E_IO               (-5)
#define E_NOEXEC           (-8)
#define E_BADFD            (-9)
#define E_CHILD            (-10)
#define E_AGAIN            (-11)
#define E_NOMEM            (-12)
#define E_ACCES            (-13)
#define E_FAULT            (-14)
#define E_BUSY             (-16)
#define E_EXIST            (-17)
#define E_NOTDIR           (-20)
#define E_ISDIR            (-21)
#define E_INVAL            (-22)
#define E_NOSPC            (-28)
#define E_ROFS             (-30)
#define E_RANGE            (-34)
#define E_NAMETOOLONG      (-36)
#define E_NOTEMPTY         (-39)

#define E_AUTH_INVALID     (-100)
#define E_AUTH_NOTFOUND    (-101)
#define E_AUTH_DISABLED    (-102)
#define E_AUTH_EXPIRED     (-103)
#define E_AUTH_PWERR       (-104)
#define E_AUTH_NOROOT      (-105)
#define E_AUTH_DENY        (-106)

struct syscall_regs {
    uint64_t rax;
    uint64_t rbx;
    uint64_t rcx;
    uint64_t rdx;
    uint64_t rsi;
    uint64_t rdi;
    uint64_t rbp;
    uint64_t r8;
    uint64_t r9;
    uint64_t r10;
    uint64_t r11;
    uint64_t r12;
    uint64_t r13;
    uint64_t r14;
    uint64_t r15;
};

typedef int64_t (*syscall_handler_t)(uint64_t arg0, uint64_t arg1, uint64_t arg2, uint64_t arg3);

void syscall_init(void);
void syscall_register(uint64_t num, syscall_handler_t handler);
int64_t syscall_dispatch(uint64_t num, uint64_t arg0, uint64_t arg1, uint64_t arg2, uint64_t arg3);

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
        : "a"(num), "b"(arg0)
        : "memory"
    );
    return ret;
}

static inline int64_t syscall2(uint64_t num, uint64_t arg0, uint64_t arg1) {
    int64_t ret;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(num), "b"(arg0), "c"(arg1)
        : "memory"
    );
    return ret;
}

static inline int64_t syscall3(uint64_t num, uint64_t arg0, uint64_t arg1, uint64_t arg2) {
    int64_t ret;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(num), "b"(arg0), "c"(arg1), "d"(arg2)
        : "memory"
    );
    return ret;
}

static inline int64_t syscall4(uint64_t num, uint64_t arg0, uint64_t arg1, uint64_t arg2, uint64_t arg3) {
    int64_t ret;
    __asm__ volatile (
        "int $0x80"
        : "=a"(ret)
        : "a"(num), "b"(arg0), "c"(arg1), "d"(arg2), "S"(arg3)
        : "memory"
    );
    return ret;
}

int64_t sys_proc_create(void);
int64_t sys_proc_exec(const char *path, char *const argv[], char *const envp[]);
int64_t sys_proc_exit(int status);
int64_t sys_proc_wait(int pid, int *status);
int64_t sys_proc_getid(void);
int64_t sys_proc_getppid(void);
int64_t sys_proc_getpwid(void);
int64_t sys_proc_setpwid(uint64_t pwid);
int64_t sys_proc_setpri(int inc);
int64_t sys_proc_yield(void);
int64_t sys_proc_sleep(uint64_t ms);

int64_t sys_fs_open(const char *path, int flags, int mode);
int64_t sys_fs_close(int fd);
int64_t sys_fs_read(int fd, void *buf, uint64_t count);
int64_t sys_fs_write(int fd, const void *buf, uint64_t count);
int64_t sys_fs_seek(int fd, int64_t offset, int whence);
int64_t sys_fs_stat(const char *path, void *stat_buf);
int64_t sys_fs_fstat(int fd, void *stat_buf);
int64_t sys_fs_chmod(const char *path, int mode);
int64_t sys_fs_chown(const char *path, uint64_t pwid);
int64_t sys_fs_unlink(const char *path);
int64_t sys_fs_rename(const char *old_path, const char *new_path);
int64_t sys_fs_mkdir(const char *path, int mode);
int64_t sys_fs_rmdir(const char *path);
int64_t sys_fs_readdir(int fd, void *dirent_buf);

int64_t sys_auth_login(const char *password, const char *note);
int64_t sys_auth_logout(void);
int64_t sys_auth_elevate(const char *cmd_path, const char **argv);
int64_t sys_auth_create(const char *password, const char *note, uint8_t level);
int64_t sys_auth_delete(uint64_t target_pwid);
int64_t sys_auth_list(void);
int64_t sys_auth_info(uint64_t target_pwid);
int64_t sys_auth_setnote(const char *new_note);
int64_t sys_auth_changepw(const char *old_pw, const char *new_pw);
int64_t sys_auth_verify(const char *password);
int64_t sys_auth_token_create(uint64_t holder, uint16_t domain, uint64_t caps,
                               uint64_t duration_secs, uint32_t max_uses);
int64_t sys_auth_token_use(uint64_t token_id);
int64_t sys_auth_token_revoke(uint64_t token_id);
int64_t sys_auth_trust_add(uint64_t trusted, uint8_t trust_level, 
                            uint16_t domain, uint64_t cap_mask);
int64_t sys_auth_trust_remove(uint64_t trusted, uint16_t domain);
int64_t sys_auth_check(uint64_t pwid, uint64_t owner_pwid, 
                        uint64_t access_type, uint16_t domain);

int64_t sys_mem_brk(void *addr);
int64_t sys_mem_map(void *addr, uint64_t len, int prot, int flags, int fd, int64_t offset);
int64_t sys_mem_unmap(void *addr, uint64_t len);
int64_t sys_mem_protect(void *addr, uint64_t len, int prot);

int64_t sys_ipc_pipe(int fd[2]);

int64_t sys_env_getcwd(char *buf, uint64_t size);
int64_t sys_env_chdir(const char *path);
int64_t sys_fs_sync(void);
int64_t sys_reboot(int cmd);
int64_t sys_time(void);
int64_t sys_info(void *info_buf);
int64_t sys_env_getvar(const char *name);
int64_t sys_env_setvar(const char *name, const char *value, int overwrite);
int64_t sys_gethostname(char *buf, uint64_t size);
int64_t sys_sethostname(const char *name, uint64_t len);

int64_t sys_dev_ioctl(int fd, int cmd, void *arg);
int64_t sys_dev_read(int fd, void *buf, uint64_t n);
int64_t sys_dev_write(int fd, const void *buf, uint64_t n);

#endif
