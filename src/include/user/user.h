#ifndef _USER_H
#define _USER_H

#include "types.h"
#include "user/syscall.h"

#define MAX_ARGS    16
#define MAX_LINE    256

#define HVFS_O_RDONLY    0
#define HVFS_O_WRONLY    1
#define HVFS_O_RDWR      2
#define HVFS_O_CREAT     0x100
#define HVFS_O_TRUNC     0x200
#define HVFS_O_APPEND    0x400

#define HVFS_TYPE_FILE   1
#define HVFS_TYPE_DIR    2

struct user_dirent {
    uint64_t inode;
    char name[256];
    uint8_t file_type;
    uint8_t reserved[7];
};

void user_print(const char *s);
void user_println(const char *s);
void user_print_char(char c);
void user_print_hex(uint64_t val);
void user_print_dec(int64_t val);

int user_read_line(char *buf, int max);
char **user_parse_args(char *line, int *argc);

int user_strcmp(const char *s1, const char *s2);
int user_strlen(const char *s);
void user_strcpy(char *dest, const char *src);
void user_memcpy(void *dest, const void *src, int n);
void user_memset(void *s, int c, int n);

int user_open(const char *path, int flags, int mode);
int user_close(int fd);
int user_read(int fd, void *buf, int count);
int user_write(int fd, const void *buf, int count);
int user_mkdir(const char *path, int mode);
int user_rmdir(const char *path);
int user_unlink(const char *path);
int user_getcwd(char *buf, int size);
int user_chdir(const char *path);

int user_auth_login(const char *password, const char *note);
void user_auth_logout(void);
int user_auth_create_pwid(const char *password, const char *note, uint8_t level);
int user_auth_create_first(const char *password);
int user_auth_change_password(const char *old_pw, const char *new_pw);
int user_auth_verify_password(const char *password);

int user_get_hostname(char *buf, int size);
int user_set_hostname(const char *name, int len);

void user_delay(int seconds);
void user_sync(void);

int user_mount(const char *source, const char *target, const char *fstype, const char *options);
int user_unmount(const char *target);

int64_t sys_fs_read_dir(int fd, void *dirent_buf);
int64_t sys_disk_partition(uint32_t disk_id, uint64_t total_sectors);
int64_t sys_disk_install_grub(uint32_t disk_id);

#endif
