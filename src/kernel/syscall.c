#include "syscall.h"
#include "klog.h"
#include "vfs.h"
#include "serial.h"
#include "hvfs.h"
#include "hvfs_ffi.h"
#include "pwid.h"
#include "proc.h"
#include "proc_ffi.h"
#include "user_proc.h"
#include "string.h"
#include "gdt.h"
#include "mm.h"
#include "keyboard.h"
#include "ata.h"
#include "grub_install.h"

int64_t sys_boot_check(int check_type);
int64_t sys_auth_create_first(const char *password);
int64_t sys_auth_create_with_caps(const char *password, const char *note, uint8_t level,
                                   const uint64_t *caps_array);

static syscall_handler_t syscall_table[MAX_SYSCALLS];

static char sys_hostname[64] = "localhost";

void syscall_init(void) {
    for (int i = 0; i < MAX_SYSCALLS; i++) {
        syscall_table[i] = NULL;
    }
    
    syscall_register(SYS_PROC_CREATE, (syscall_handler_t)sys_proc_create);
    syscall_register(SYS_PROC_EXEC, (syscall_handler_t)sys_proc_exec);
    syscall_register(SYS_PROC_EXIT, (syscall_handler_t)sys_proc_exit);
    syscall_register(SYS_PROC_WAIT, (syscall_handler_t)sys_proc_wait);
    syscall_register(SYS_PROC_GETID, (syscall_handler_t)sys_proc_getid);
    syscall_register(SYS_PROC_GETPPID, (syscall_handler_t)sys_proc_getppid);
    syscall_register(SYS_PROC_GETPWID, (syscall_handler_t)sys_proc_getpwid);
    syscall_register(SYS_PROC_YIELD, (syscall_handler_t)sys_proc_yield);
    
    syscall_register(SYS_FS_OPEN, (syscall_handler_t)sys_fs_open);
    syscall_register(SYS_FS_CLOSE, (syscall_handler_t)sys_fs_close);
    syscall_register(SYS_FS_READ, (syscall_handler_t)sys_fs_read);
    syscall_register(SYS_FS_WRITE, (syscall_handler_t)sys_fs_write);
    syscall_register(SYS_FS_MKDIR, (syscall_handler_t)sys_fs_mkdir);
    syscall_register(SYS_FS_RMDIR, (syscall_handler_t)sys_fs_rmdir);
    syscall_register(SYS_FS_UNLINK, (syscall_handler_t)sys_fs_unlink);
    syscall_register(SYS_FS_STAT, (syscall_handler_t)sys_fs_stat);
    syscall_register(SYS_FS_CHMOD, (syscall_handler_t)sys_fs_chmod);
    syscall_register(SYS_FS_CHOWN, (syscall_handler_t)sys_fs_chown);
    syscall_register(SYS_FS_RENAME, (syscall_handler_t)sys_fs_rename);
    syscall_register(SYS_FS_SEEK, (syscall_handler_t)sys_fs_seek);
    syscall_register(SYS_FS_READDIR, (syscall_handler_t)sys_fs_readdir);
    
    syscall_register(SYS_AUTH_LOGIN, (syscall_handler_t)sys_auth_login);
    syscall_register(SYS_AUTH_LOGOUT, (syscall_handler_t)sys_auth_logout);
    syscall_register(SYS_AUTH_CREATE, (syscall_handler_t)sys_auth_create);
    syscall_register(SYS_AUTH_DELETE, (syscall_handler_t)sys_auth_delete);
    syscall_register(SYS_AUTH_LIST, (syscall_handler_t)sys_auth_list);
    syscall_register(SYS_AUTH_INFO, (syscall_handler_t)sys_auth_info);
    syscall_register(SYS_AUTH_SETNOTE, (syscall_handler_t)sys_auth_setnote);
    syscall_register(SYS_AUTH_CHANGEPW, (syscall_handler_t)sys_auth_changepw);
    syscall_register(SYS_AUTH_VERIFY, (syscall_handler_t)sys_auth_verify);
    syscall_register(SYS_AUTH_CREATE_FIRST, (syscall_handler_t)sys_auth_create_first);
    syscall_register(SYS_AUTH_ELEVATE, (syscall_handler_t)sys_auth_elevate);
    syscall_register(SYS_AUTH_TOKEN_CREATE, (syscall_handler_t)sys_auth_token_create);
    syscall_register(SYS_AUTH_TOKEN_USE, (syscall_handler_t)sys_auth_token_use);
    syscall_register(SYS_AUTH_TOKEN_REVOKE, (syscall_handler_t)sys_auth_token_revoke);
    syscall_register(SYS_AUTH_TRUST_ADD, (syscall_handler_t)sys_auth_trust_add);
    syscall_register(SYS_AUTH_TRUST_REMOVE, (syscall_handler_t)sys_auth_trust_remove);
    syscall_register(SYS_AUTH_CHECK, (syscall_handler_t)sys_auth_check);
    syscall_register(SYS_AUTH_CREATE_WITH_CAPS, (syscall_handler_t)sys_auth_create_with_caps);
    
    syscall_register(SYS_ENV_GETCWD, (syscall_handler_t)sys_env_getcwd);
    syscall_register(SYS_ENV_CHDIR, (syscall_handler_t)sys_env_chdir);
    syscall_register(SYS_GETHOSTNAME, (syscall_handler_t)sys_gethostname);
    syscall_register(SYS_SETHOSTNAME, (syscall_handler_t)sys_sethostname);
    syscall_register(SYS_FS_SYNC, (syscall_handler_t)sys_fs_sync);
    syscall_register(SYS_BOOT_CHECK, (syscall_handler_t)sys_boot_check);
    syscall_register(SYS_FS_MOUNT, (syscall_handler_t)sys_fs_mount);
    syscall_register(SYS_FS_UNMOUNT, (syscall_handler_t)sys_fs_unmount);
    syscall_register(SYS_DISK_LIST, (syscall_handler_t)sys_disk_list);
    syscall_register(SYS_DISK_INFO, (syscall_handler_t)sys_disk_info);
    syscall_register(SYS_DISK_FORMAT, (syscall_handler_t)sys_disk_format);
    syscall_register(SYS_DISK_PARTITION, (syscall_handler_t)sys_disk_partition);
    syscall_register(SYS_DISK_INSTALL_GRUB, (syscall_handler_t)sys_disk_install_grub);
}

void syscall_register(uint64_t num, syscall_handler_t handler) {
    if (num < MAX_SYSCALLS) {
        syscall_table[num] = handler;
    }
}

int64_t syscall_dispatch(uint64_t num, uint64_t arg0, uint64_t arg1, uint64_t arg2, uint64_t arg3) {
    if (num >= MAX_SYSCALLS || syscall_table[num] == NULL) {
        return E_INVAL;
    }
    return syscall_table[num](arg0, arg1, arg2, arg3);
}

int64_t sys_proc_create(void) {
    struct process *parent = process_get_current();
    uint64_t pwid = parent ? parent->pwid : 0;
    
    uint32_t child_pid = process_create(NULL, 0, pwid);
    if (child_pid == 0) {
        return E_NOMEM;
    }
    
    struct process *child = process_find_by_pid(child_pid);
    if (!child) return E_NOMEM;
    
    if (parent) {
        child->parent_pid = parent->pid;
        child->parent = parent;
        child->pwid = pwid;
        
        if (parent->cr3) {
            child->cr3 = vmm_create_user_page_table();
            if (child->cr3 == 0) {
                // Clean up child, do NOT kill parent
                child->state = PROC_ZOMBIE;
                return E_NOMEM;
            }
        }
    }
    
    child->state = PROC_READY;
    scheduler_add(child_pid);
    
    return child->pid;
}

int64_t sys_proc_exec(const char *path, char *const argv[], char *const envp[]) {
    struct process *proc = process_get_current();
    if (proc == NULL) {
        return E_PERM;
    }
    
    uint64_t pwid = proc->pwid;
    
    int argc = 0;
    if (argv != NULL) {
        while (argv[argc] != NULL) argc++;
    }
    
    int envc = 0;
    if (envp != NULL) {
        while (envp[envc] != NULL) envc++;
    }
    
    int pid = user_proc_load_elf(path, pwid);
    if (pid < 0) {
        return E_NOTFOUND;
    }
    
    /* Set up argv/envp on the new process's user stack */
    if (argc > 0) {
        extern int user_proc_setup_argv(uint32_t pid, const char **argv, uint32_t argc,
                                        const char **envp, uint32_t envc);
        user_proc_setup_argv((uint32_t)pid, (const char **)argv, (uint32_t)argc,
                            (const char **)envp, (uint32_t)envc);
    }
    
    extern void sched_add_internal(uint32_t pid);
    sched_add_internal((uint32_t)pid);
    
    return pid;
}

int64_t sys_proc_exit(int status) {
    process_exit((uint32_t)status);
    return 0;
}

int64_t sys_proc_wait(int pid, int *status) {
    struct process *parent = process_get_current();
    if (parent == NULL) {
        return E_PERM;
    }
    
    struct process *child = NULL;
    
    if (pid == -1) {
        for (int i = 0; i < MAX_PROCESSES; i++) {
            struct process *p = process_find_by_pid(i + 1);
            if (p && p->parent_pid == parent->pid && p->state == PROC_ZOMBIE) {
                child = p;
                break;
            }
        }
    } else {
        child = process_find_by_pid(pid);
        if (child == NULL || child->parent_pid != parent->pid) {
            return E_CHILD;
        }
    }
    
    if (child == NULL) {
        return E_CHILD;
    }
    
    if (child->state != PROC_ZOMBIE) {
        parent->state = PROC_BLOCKED;
        scheduler_yield();
    }
    
    if (status != NULL) {
        *status = (int)child->exit_code;
    }
    
    int child_pid = (int)child->pid;
    child->state = PROC_NEW;
    child->pid = 0;
    
    return child_pid;
}

int64_t sys_proc_getid(void) {
    struct process *proc = process_get_current();
    return proc ? proc->pid : 0;
}

int64_t sys_proc_getppid(void) {
    struct process *proc = process_get_current();
    return proc ? proc->parent_pid : 0;
}

int64_t sys_proc_getpwid(void) {
    struct process *proc = process_get_current();
    return proc ? (int64_t)proc->pwid : 0;
}

int64_t sys_proc_setpwid(uint64_t pwid) {
    struct process *proc = process_get_current();
    if (!proc) {
        return E_PERM;
    }
    
    if (!pwid_has_cap_raw(pwid_get_current(), 0, CAP_DOMAIN_SYS_ADMIN)) {
        return E_AUTH_CAP;
    }
    
    proc->pwid = pwid;
    return 0;
}

int64_t sys_proc_setpri(int inc) {
    (void)inc;
    return E_NOSYS;
}

int64_t sys_proc_yield(void) {
    scheduler_yield();
    return 0;
}

int64_t sys_proc_sleep(uint64_t ms) {
    (void)ms;
    return E_NOSYS;
}

int64_t sys_fs_open(const char *path, int flags, int mode) {
    (void)mode;
    if (path == NULL) {
        return E_INVAL;
    }

    uint64_t pwid = pwid_get_current();

    int64_t result = vfs_open(path, (uint32_t)flags, pwid);

    if (result < 0) {
        klog_syscall("open failed: %s result=%d", path, (int32_t)result);
    }

    return result;
}

int64_t sys_fs_close(int fd) {
    if (fd < 0) {
        return E_BADFD;
    }

    int64_t result = vfs_close((uint32_t)fd);

    return result;
}

int64_t sys_fs_read(int fd, void *buf, uint64_t count) {
    if (fd == 0) {
        if (buf == NULL || count == 0) return -1;
        
        char *buffer = (char *)buf;
        uint64_t read_count = 0;
        
        while (read_count < count) {
            int c = -1;
            
            if (keyboard_has_data()) {
                c = keyboard_get_char();
            } else if (serial_has_data(SERIAL_COM1)) {
                c = serial_getc(SERIAL_COM1);
            }
            
            if (c == -1 || c == 0) {
                if (read_count > 0) break;
                __asm__ volatile ("pause");
                continue;
            }
            
            buffer[read_count++] = (char)c;
            
            if ((char)c == '\n') break;
        }
        
        return (int64_t)read_count;
    }
    
    return vfs_read(fd, buf, count);
}

int64_t sys_fs_write(int fd, const void *buf, uint64_t count) {
    if (fd == 1 || fd == 2) {
        serial_write(SERIAL_COM1, buf, count);
        return count;
    }
    
    return vfs_write(fd, buf, count);
}

int64_t sys_fs_seek(int fd, int64_t offset, int whence) {
    return vfs_seek(fd, offset, whence);
}

int64_t sys_fs_stat(const char *path, void *stat_buf) {
    uint64_t pwid = pwid_get_current();
    return vfs_stat(path, (struct vfs_stat *)stat_buf, pwid);
}

int64_t sys_fs_fstat(int fd, void *stat_buf) {
    (void)fd;
    (void)stat_buf;
    return E_NOSYS;
}

int64_t sys_fs_chmod(const char *path, int mode) {
    uint64_t pwid = pwid_get_current();
    return vfs_chmod(path, (uint16_t)mode, pwid);
}

int64_t sys_fs_chown(const char *path, uint64_t owner_pwid) {
    uint64_t pwid = pwid_get_current();
    return vfs_chown(path, owner_pwid, pwid);
}

int64_t sys_fs_unlink(const char *path) {
    uint64_t pwid = pwid_get_current();
    return vfs_unlink(path, pwid);
}

int64_t sys_fs_rename(const char *old_path, const char *new_path) {
    uint64_t pwid = pwid_get_current();
    return vfs_rename(old_path, new_path, pwid);
}

int64_t sys_fs_mkdir(const char *path, int mode) {
    (void)mode;
    if (path == NULL) {
        return -1;
    }

    uint64_t pwid = pwid_get_current();
    if (pwid == 0) {
        pwid = 0x0020F45A8B978417;
    }

    int64_t result = vfs_mkdir(path, pwid);

    if (result < 0) {
        klog_syscall("mkdir failed: %s result=%d", path, (int32_t)result);
    }

    return result;
}

int64_t sys_fs_rmdir(const char *path) {
    uint64_t pwid = pwid_get_current();
    return vfs_rmdir(path, pwid);
}

int64_t sys_fs_readdir(int fd, void *dirent_buf) {
    return vfs_readdir(fd, (struct vfs_dirent *)dirent_buf);
}

int64_t sys_auth_login(const char *password, const char *note) {
    int result = pwid_login(note, password);
    if (result == PWID_OK) {
        return 0;  /* Standard: 0 = success */
    } else if (result == PWID_ERR_PASSWORD) {
        return E_AUTH_PWERR;
    } else if (result == PWID_ERR_NOT_FOUND) {
        return E_AUTH_NOTFOUND;
    } else if (result == PWID_ERR_DISABLED) {
        return E_AUTH_DISABLED;
    }
    return E_AUTH_INVALID;
}

int64_t sys_auth_logout(void) {
    pwid_logout();
    return 0;
}

int64_t sys_auth_elevate(const char *cmd_path, const char **argv) {
    (void)cmd_path;
    (void)argv;
    
    uint64_t current_pwid = pwid_get_current();
    if (current_pwid == 0) {
        return E_AUTH_NOTFOUND;
    }
    
    struct pwid_entry *entry = pwid_find(current_pwid);
    if (entry == NULL) {
        return E_AUTH_NOTFOUND;
    }
    
    int64_t token = pwid_create_token(current_pwid, CAP_DOMAIN_SYSTEM, 
                                       0xFFFFFFFFFFFFFFFFULL, 3600, 1);
    if (token < 0) {
        return E_PERM;
    }
    
    return token;
}

int64_t sys_auth_token_create(uint64_t holder, uint16_t domain, uint64_t caps,
                               uint64_t duration_secs, uint32_t max_uses) {
    uint64_t current_pwid = pwid_get_current();
    if (current_pwid == 0) {
        return E_AUTH_NOTFOUND;
    }
    
    if (!pwid_has_cap_raw(current_pwid, 0, CAP_DOMAIN_TOKEN_ISSUE)) {
        return E_AUTH_CAP;
    }
    
    return pwid_create_token(holder, domain, caps, duration_secs, max_uses);
}

int64_t sys_auth_token_use(uint64_t token_id) {
    return pwid_use_token_internal(token_id);
}

int64_t sys_auth_token_revoke(uint64_t token_id) {
    uint64_t current_pwid = pwid_get_current();
    return pwid_revoke_token_internal(token_id, current_pwid);
}

int64_t sys_auth_trust_add(uint64_t trusted, uint8_t trust_level, 
                            uint16_t domain, uint64_t cap_mask) {
    uint64_t current_pwid = pwid_get_current();
    if (current_pwid == 0) {
        return E_AUTH_NOTFOUND;
    }
    
    if (!pwid_has_cap_raw(current_pwid, 0, CAP_DOMAIN_TRUST_ADD)) {
        return E_AUTH_CAP;
    }
    
    return pwid_add_trust_relation(current_pwid, trusted, trust_level, domain, cap_mask);
}

int64_t sys_auth_trust_remove(uint64_t trusted, uint16_t domain) {
    uint64_t current_pwid = pwid_get_current();
    if (current_pwid == 0) {
        return E_AUTH_NOTFOUND;
    }
    
    return pwid_remove_trust_internal(current_pwid, trusted, domain);
}

int64_t sys_auth_check(uint64_t pwid, uint64_t owner_pwid, 
                        uint64_t access_type, uint16_t domain) {
    return pwid_enhanced_check(pwid, owner_pwid, access_type, domain);
}

int64_t sys_auth_create(const char *password, const char *note, uint8_t level) {
    if (!pwid_has_cap_raw(pwid_get_current(), 0, CAP_DOMAIN_SYS_ADMIN)) {
        return E_AUTH_CAP;
    }
    
    int result = pwid_create_user(password, note, level);
    if (result == PWID_OK) {
        return 0;
    } else if (result == PWID_ERR_FULL) {
        return E_BUSY;
    } else if (result == PWID_ERR_EXISTS) {
        return E_EXIST;
    }
    return E_PERM;
}

/* Create user with explicit capability mask (v4: precise delegation) */
int64_t sys_auth_create_with_caps(const char *password, const char *note, uint8_t level,
                                   const uint64_t *caps_array) {
    if (!pwid_has_cap_raw(pwid_get_current(), 0, CAP_DOMAIN_SYS_ADMIN)) {
        return E_AUTH_CAP;
    }
    
    int result = pwid_create_user_with_caps(password, note, level, caps_array);
    if (result == PWID_OK) {
        return 0;
    } else if (result == PWID_ERR_FULL) {
        return E_BUSY;
    } else if (result == PWID_ERR_EXISTS) {
        return E_EXIST;
    }
    return E_PERM;
}

int64_t sys_auth_create_first(const char *password) {
    if (pwid_any_identity_exists()) {
        return E_EXIST;
    }
    
    int result = pwid_create_first_identity(password);
    if (result == 0) {
        pwid_login("root", password);
        return 0;
    }
    return E_PERM;
}

int64_t sys_auth_delete(uint64_t target_pwid) {
    if (!pwid_has_cap_raw(pwid_get_current(), 0, CAP_DOMAIN_SYS_ADMIN)) {
        return E_AUTH_CAP;
    }
    
    int result = pwid_delete(target_pwid);
    if (result == PWID_OK) {
        return 0;
    } else if (result == PWID_ERR_NOT_FOUND) {
        return E_AUTH_NOTFOUND;
    }
    return E_PERM;
}

int64_t sys_auth_list(void) {
    if (!pwid_has_cap_raw(pwid_get_current(), 0, CAP_DOMAIN_SYS_ADMIN)) {
        return E_AUTH_CAP;
    }
    pwid_list_all();
    return 0;
}

int64_t sys_auth_info(uint64_t target_pwid) {
    struct pwid_entry *entry = pwid_find(target_pwid);
    if (!entry) {
        return E_AUTH_NOTFOUND;
    }
    return (int64_t)entry->level;
}

int64_t sys_auth_setnote(const char *new_note) {
    (void)new_note;
    return E_PERM;
}

int64_t sys_auth_changepw(const char *old_pw, const char *new_pw) {
    uint64_t current_pwid = pwid_get_current();
    int result = pwid_change_password(current_pwid, old_pw, new_pw);
    if (result == PWID_OK) {
        return 0;
    } else if (result == PWID_ERR_PASSWORD) {
        return E_AUTH_PWERR;
    }
    return E_PERM;
}

int64_t sys_auth_verify(const char *password) {
    uint64_t current_pwid = pwid_get_current();
    int result = pwid_verify_password(current_pwid, password);
    return result == PWID_OK ? 0 : E_AUTH_PWERR;
}

int64_t sys_mem_brk(void *addr) {
    (void)addr;
    return E_NOSYS;
}

int64_t sys_mem_map(void *addr, uint64_t len, int prot, int flags, int fd, int64_t offset) {
    (void)addr;
    (void)len;
    (void)prot;
    (void)flags;
    (void)fd;
    (void)offset;
    return E_NOSYS;
}

int64_t sys_mem_unmap(void *addr, uint64_t len) {
    (void)addr;
    (void)len;
    return E_NOSYS;
}

int64_t sys_mem_protect(void *addr, uint64_t len, int prot) {
    (void)addr;
    (void)len;
    (void)prot;
    return E_NOSYS;
}

int64_t sys_ipc_pipe(int fd[2]) {
    (void)fd;
    return E_NOSYS;
}

int64_t sys_env_getcwd(char *buf, uint64_t size) {
    if (buf == NULL || size == 0) {
        return E_INVAL;
    }
    
    const char *cwd = vfs_get_cwd();
    int len = 0;
    while (cwd[len] && len < (int)(size - 1)) {
        buf[len] = cwd[len];
        len++;
    }
    buf[len] = '\0';
    
    return len;
}

int64_t sys_env_chdir(const char *path) {
    if (path == NULL) {
        return E_INVAL;
    }
    
    uint64_t pwid = pwid_get_current();
    struct vfs_stat st;
    
    if (vfs_stat(path, &st, pwid) != 0) {
        return E_NOTFOUND;
    }
    
    if (st.type != VFS_TYPE_DIR) {
        return E_NOTDIR;
    }
    
    vfs_set_cwd(path);
    return 0;
}

int64_t sys_fs_sync(void) {
    return vfs_sync();
}

int64_t sys_reboot(int cmd) {
    if (cmd == 0) {
        klog_kern("Rebooting...");
        
        vfs_sync();
        
        for (int i = 0; i < 100000000; i++) {
            __asm__ volatile ("nop");
        }
        
        __asm__ volatile (
            "mov $0x64, %rax\n"
            "mov $0x2000, %rdx\n"
            "out %al, %dx\n"
            "1: hlt\n"
            "jmp 1b\n"
        );
        
        return 0;
    }
    
    return E_PERM;
}

int64_t sys_time(void) {
    return E_NOSYS;
}

int64_t sys_info(void *info_buf) {
    (void)info_buf;
    return E_NOSYS;
}

int64_t sys_env_getvar(const char *name) {
    (void)name;
    return E_NOSYS;
}

int64_t sys_env_setvar(const char *name, const char *value, int overwrite) {
    (void)name;
    (void)value;
    (void)overwrite;
    return E_NOSYS;
}

int64_t sys_gethostname(char *buf, uint64_t size) {
    if (buf == NULL || size == 0) {
        return E_INVAL;
    }
    
    uint64_t len = 0;
    while (sys_hostname[len] && len < size - 1) {
        buf[len] = sys_hostname[len];
        len++;
    }
    buf[len] = '\0';
    
    return 0;
}

int64_t sys_sethostname(const char *name, uint64_t len) {
    if (!pwid_has_cap_raw(pwid_get_current(), 0, CAP_DOMAIN_SYS_ADMIN)) {
        return E_AUTH_CAP;
    }
    
    if (name == NULL || len == 0 || len > 63) {
        return E_INVAL;
    }
    
    for (uint64_t i = 0; i < len; i++) {
        char c = name[i];
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
              (c >= '0' && c <= '9') || c == '-' || c == '.')) {
            return E_INVAL;
        }
    }
    
    for (uint64_t i = 0; i < len; i++) {
        sys_hostname[i] = name[i];
    }
    sys_hostname[len] = '\0';
    
    return 0;
}

static const char *INSTALL_MARKER_FILE = "/.antx_installed";

int64_t sys_boot_check(int check_type) {
    switch (check_type) {
        case 0:
            return pwid_any_identity_exists() ? 1 : 0;
        case 1: {
            int fd = sys_fs_open(INSTALL_MARKER_FILE, HVFS_O_RDONLY, 0);
            if (fd >= 0) {
                sys_fs_close(fd);
                return 1;
            }
            return 0;
        }
        default:
            return -1;
    }
}

int64_t sys_dev_ioctl(int fd, int cmd, void *arg) {
    (void)fd;
    (void)cmd;
    (void)arg;
    return E_NOSYS;
}

int64_t sys_dev_read(int fd, void *buf, uint64_t n) {
    (void)fd;
    (void)buf;
    (void)n;
    return E_NOSYS;
}

int64_t sys_dev_write(int fd, const void *buf, uint64_t n) {
    (void)fd;
    (void)buf;
    (void)n;
    return E_NOSYS;
}

int64_t sys_fs_mount(const char *source, const char *target, const char *fstype, const char *options) {
    (void)source;
    (void)options;
    
    if (target == NULL || fstype == NULL) {
        return E_INVAL;
    }
    
    struct process *proc = process_get_current();
    uint64_t pwid = proc ? proc->pwid : 0;
    
    if (!pwid_has_cap_raw(pwid, 0, CAP_DOMAIN_SYS_ADMIN)) {
        return E_PERM;
    }
    
    int result = vfs_mount(target, fstype);
    return result == 0 ? 0 : E_IO;
}

int64_t sys_fs_unmount(const char *target) {
    if (target == NULL) {
        return E_INVAL;
    }
    
    struct process *proc = process_get_current();
    uint64_t pwid = proc ? proc->pwid : 0;
    
    if (!pwid_has_cap_raw(pwid, 0, CAP_DOMAIN_SYS_ADMIN)) {
        return E_PERM;
    }
    
    /* Forward to VFS unmount (currently a placeholder, returns 0 for now) */
    (void)target;
    return E_NOSYS;
}

int64_t sys_disk_list(uint64_t *disks, uint32_t max_count) {
    if (disks == NULL || max_count == 0) {
        return E_INVAL;
    }
    
    uint32_t count = 0;
    
    for (uint8_t drive = 0; drive < 4 && count < max_count; drive++) {
        if (ata_disk_present(drive)) {
            disks[count++] = drive;
        }
    }
    
    return count;
}

struct disk_info {
    uint32_t disk_id;
    uint32_t sectors;
    uint32_t sector_size;
    char model[41];
    uint8_t present;
    uint8_t formatted;
};

int64_t sys_disk_info(uint32_t disk_id, void *info) {
    if (info == NULL) {
        return E_INVAL;
    }
    
    if (disk_id >= 4) {
        return E_NOTFOUND;
    }
    
    struct disk_info *dinfo = (struct disk_info *)info;
    
    dinfo->disk_id = disk_id;
    dinfo->present = ata_disk_present((uint8_t)disk_id);
    
    if (!dinfo->present) {
        return E_NOTFOUND;
    }
    
    uint16_t identify_data[256];
    if (ata_identify((uint8_t)disk_id, identify_data) == 0) {
        dinfo->sectors = identify_data[60] | (identify_data[61] << 16);
        dinfo->sector_size = 512;
        
        for (int i = 0; i < 20; i++) {
            dinfo->model[i * 2] = (char)(identify_data[27 + i] >> 8);
            dinfo->model[i * 2 + 1] = (char)(identify_data[27 + i] & 0xFF);
        }
        dinfo->model[40] = '\0';
    } else {
        dinfo->sectors = 0;
        dinfo->sector_size = 512;
        dinfo->model[0] = '\0';
    }
    
    dinfo->formatted = 0;
    
    return 0;
}

int64_t sys_disk_format(uint32_t disk_id, const char *fstype) {
    if (fstype == NULL) {
        return E_INVAL;
    }
    
    if (disk_id >= 4) {
        return E_NOTFOUND;
    }
    
    struct process *proc = process_get_current();
    uint64_t pwid = proc ? proc->pwid : 0;
    
    if (!pwid_has_cap_raw(pwid, 0, CAP_DOMAIN_DEVICE_DISK)) {
        return E_PERM;
    }
    
    if (!ata_disk_present((uint8_t)disk_id)) {
        return E_NOTFOUND;
    }
    
    if (strcmp(fstype, "hvfs") == 0 || strcmp(fstype, "diskfs") == 0) {
        int result = hvfs_format();
        return result == 0 ? 0 : E_IO;
    }
    
    return E_INVAL;
}

int64_t sys_disk_partition(uint32_t disk_id, uint64_t total_sectors) {
    if (disk_id >= 4) {
        return E_NOTFOUND;
    }
    
    struct process *proc = process_get_current();
    uint64_t pwid = proc ? proc->pwid : 0;
    
    if (!pwid_has_cap_raw(pwid, 0, CAP_DOMAIN_DEVICE_DISK)) {
        return E_PERM;
    }
    
    if (!ata_disk_present((uint8_t)disk_id)) {
        return E_NOTFOUND;
    }
    
    int result = grub_create_partition_table(disk_id, total_sectors);
    return result == 0 ? 0 : E_IO;
}

int64_t sys_disk_install_grub(uint32_t disk_id) {
    if (disk_id >= 4) {
        return E_NOTFOUND;
    }
    
    struct process *proc = process_get_current();
    uint64_t pwid = proc ? proc->pwid : 0;
    
    if (!pwid_has_cap_raw(pwid, 0, CAP_DOMAIN_DEVICE_DISK)) {
        return E_PERM;
    }
    
    if (!ata_disk_present((uint8_t)disk_id)) {
        return E_NOTFOUND;
    }
    
    int result = grub_install_mbr(disk_id);
    return result == 0 ? 0 : E_IO;
}
