#include "user/user.h"

static char stdout_buf[1024];

void user_print(const char *s) {
    int len = 0;
    while (s[len]) len++;
    sys_fs_write(1, s, len);
}

void user_println(const char *s) {
    user_print(s);
    user_print("\n");
}

void user_print_char(char c) {
    sys_fs_write(1, &c, 1);
}

void user_print_hex(uint64_t val) {
    char buf[17];
    buf[16] = '\0';
    for (int i = 15; i >= 0; i--) {
        int digit = val & 0xF;
        buf[i] = digit < 10 ? '0' + digit : 'A' + digit - 10;
        val >>= 4;
    }
    user_print("0x");
    user_print(buf);
}

void user_print_dec(int64_t val) {
    char buf[21];
    int i = 20;
    buf[i] = '\0';
    
    int neg = 0;
    if (val < 0) {
        neg = 1;
        val = -val;
    }
    
    if (val == 0) {
        buf[--i] = '0';
    } else {
        while (val > 0) {
            buf[--i] = '0' + (val % 10);
            val /= 10;
        }
    }
    
    if (neg) {
        buf[--i] = '-';
    }
    
    user_print(&buf[i]);
}

int user_read_line(char *buf, int max) {
    int i = 0;
    char c;
    
    while (i < max - 1) {
        int result = sys_fs_read(0, &c, 1);
        if (result <= 0) {
            continue;
        }
        
        if (c == '\n') {
            user_print("\n");
            break;
        } else if (c == '\b' || c == 0x7F) {
            if (i > 0) {
                i--;
                user_print("\b \b");
            }
        } else if (c >= ' ' && c <= '~') {
            buf[i++] = c;
            user_print_char(c);
        }
    }
    
    buf[i] = '\0';
    return i;
}

static char *arg_ptrs[MAX_ARGS];
static char arg_buf[MAX_LINE];

char **user_parse_args(char *line, int *argc) {
    *argc = 0;
    int in_arg = 0;
    int in_quote = 0;
    char *p = line;
    char *out = arg_buf;
    
    while (*p && *argc < MAX_ARGS - 1) {
        if (*p == '"') {
            in_quote = !in_quote;
            p++;
        } else if (*p == ' ' && !in_quote) {
            if (in_arg) {
                *out++ = '\0';
                in_arg = 0;
            }
            p++;
        } else {
            if (!in_arg) {
                arg_ptrs[*argc] = out;
                (*argc)++;
                in_arg = 1;
            }
            *out++ = *p++;
        }
    }
    *out = '\0';
    
    arg_ptrs[*argc] = NULL;
    return arg_ptrs;
}

int user_strcmp(const char *s1, const char *s2) {
    while (*s1 && *s2 && *s1 == *s2) {
        s1++;
        s2++;
    }
    return *s1 - *s2;
}

int user_strlen(const char *s) {
    int len = 0;
    while (s[len]) len++;
    return len;
}

void user_strcpy(char *dest, const char *src) {
    while (*src) {
        *dest++ = *src++;
    }
    *dest = '\0';
}

void user_memcpy(void *dest, const void *src, int n) {
    char *d = (char *)dest;
    const char *s = (const char *)src;
    while (n--) {
        *d++ = *s++;
    }
}

void user_memset(void *s, int c, int n) {
    char *p = (char *)s;
    while (n--) {
        *p++ = c;
    }
}

int user_open(const char *path, int flags, int mode) {
    return sys_fs_open(path, flags, mode);
}

int user_close(int fd) {
    return sys_fs_close(fd);
}

int user_read(int fd, void *buf, int count) {
    return sys_fs_read(fd, buf, count);
}

int user_write(int fd, const void *buf, int count) {
    return sys_fs_write(fd, buf, count);
}

int user_mkdir(const char *path, int mode) {
    return sys_fs_make_dir(path, mode);
}

int user_rmdir(const char *path) {
    return sys_fs_remove_dir(path);
}

int user_unlink(const char *path) {
    return sys_fs_delete(path);
}

int user_getcwd(char *buf, int size) {
    return sys_env_get_current_dir(buf, size);
}

int user_chdir(const char *path) {
    return sys_env_set_current_dir(path);
}

int user_auth_login(const char *password, const char *note) {
    return sys_auth_authenticate(note, password);
}

void user_auth_logout(void) {
    sys_auth_invalidate_session();
}

int user_auth_create_pwid(const char *password, const char *note, uint8_t level) {
    return sys_auth_create_pwid(password, note, level);
}

int user_auth_change_password(const char *old_pw, const char *new_pw) {
    return sys_auth_change_pwid_password(old_pw, new_pw);
}

int user_auth_verify_password(const char *password) {
    return sys_auth_verify_pwid_password(password);
}

int user_auth_create_original_root(const char *password) {
    return sys_auth_create_original_root(password);
}

int user_get_hostname(char *buf, int size) {
    return sys_get_hostname(buf, size);
}

int user_set_hostname(const char *name, int len) {
    return sys_set_hostname(name, len);
}

void user_delay(int seconds) {
    volatile int i = 0;
    while (seconds-- > 0) {
        for (i = 0; i < 10000000; i++);
    }
}

void user_sync(void) {
    sys_fs_sync_all();
}

int user_mount(const char *source, const char *target, const char *fstype, const char *options) {
    return sys_fs_mount(source, target, fstype, options);
}

int user_unmount(const char *target) {
    return sys_fs_unmount(target);
}
