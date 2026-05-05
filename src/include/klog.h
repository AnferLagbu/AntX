#ifndef _KLOG_H
#define _KLOG_H

#include "types.h"
#include "stdarg.h"

#define KLOG_VERSION "1.0.0"

#define KLOG_BUFFER_SIZE    (128 * 1024)
#define KLOG_LINE_MAX       512
#define KLOG_MAX_MODULES    32

typedef enum {
    KLOG_DEBUG    = 0,
    KLOG_INFO     = 1,
    KLOG_NOTICE   = 2,
    KLOG_WARN     = 3,
    KLOG_ERROR    = 4,
    KLOG_CRITICAL = 5,
    KLOG_MAX_LEVEL
} klog_level_t;

typedef enum {
    KLOG_CAT_GENERAL   = 0,
    KLOG_CAT_BOOT      = 1,
    KLOG_CAT_INIT      = 2,
    KLOG_CAT_KERNEL    = 3,
    KLOG_CAT_MEMORY    = 4,
    KLOG_CAT_PROCESS   = 5,
    KLOG_CAT_FS        = 6,
    KLOG_CAT_DRIVER    = 7,
    KLOG_CAT_SYSCALL   = 8,
    KLOG_CAT_IPC       = 9,
    KLOG_CAT_SECURITY  = 10,
    KLOG_CAT_NETWORK   = 11,
    KLOG_CAT_MAX
} klog_category_t;

typedef struct {
    uint64_t timestamp;
    uint32_t level;
    uint32_t category;
    uint32_t cpu_id;
    uint32_t line;
    const char *file;
    const char *func;
    char message[KLOG_LINE_MAX];
} klog_entry_t;

#define KLOG_FLAG_OUTPUT_SERIAL   (1 << 0)
#define KLOG_FLAG_OUTPUT_BUFFER   (1 << 1)
#define KLOG_FLAG_OUTPUT_CONSOLE  (1 << 2)
#define KLOG_FLAG_TIMESTAMP       (1 << 3)
#define KLOG_FLAG_LOCATION        (1 << 4)
#define KLOG_FLAG_PERSIST         (1 << 5)

#ifndef KLOG_DEFAULT_LEVEL
#define KLOG_DEFAULT_LEVEL KLOG_INFO
#endif

#ifndef KLOG_DEFAULT_FLAGS
#define KLOG_DEFAULT_FLAGS (KLOG_FLAG_OUTPUT_SERIAL | KLOG_FLAG_OUTPUT_BUFFER | KLOG_FLAG_TIMESTAMP)
#endif

void klog_init(void);
void klog_set_level(klog_level_t level);
klog_level_t klog_get_level(void);
void klog_set_flags(uint32_t flags);
uint32_t klog_get_flags(void);
void klog_set_category_level(klog_category_t cat, klog_level_t level);

int klog_write(klog_level_t level, klog_category_t cat,
               const char *file, const char *func, int line,
               const char *fmt, ...);
int klog_vwrite(klog_level_t level, klog_category_t cat,
                const char *file, const char *func, int line,
                const char *fmt, va_list args);

void klog_flush(void);
void klog_dump(void);
void klog_clear(void);

int klog_save_to_disk(void);
int klog_load_from_disk(void);

uint64_t klog_get_entry_count(void);
int klog_get_entry(uint64_t index, klog_entry_t *entry);

const char *klog_level_string(klog_level_t level);
const char *klog_category_string(klog_category_t cat);

#define klog(level, cat, fmt, ...) \
    klog_write(level, cat, __FILE__, __func__, __LINE__, fmt, ##__VA_ARGS__)

#define klog_debug(cat, fmt, ...)   klog(KLOG_DEBUG, cat, fmt, ##__VA_ARGS__)
#define klog_info(cat, fmt, ...)    klog(KLOG_INFO, cat, fmt, ##__VA_ARGS__)
#define klog_notice(cat, fmt, ...)  klog(KLOG_NOTICE, cat, fmt, ##__VA_ARGS__)
#define klog_warn(cat, fmt, ...)    klog(KLOG_WARN, cat, fmt, ##__VA_ARGS__)
#define klog_error(cat, fmt, ...)   klog(KLOG_ERROR, cat, fmt, ##__VA_ARGS__)
#define klog_crit(cat, fmt, ...)    klog(KLOG_CRITICAL, cat, fmt, ##__VA_ARGS__)

#define LOG_BOOT    KLOG_CAT_BOOT
#define LOG_INIT    KLOG_CAT_INIT
#define LOG_KERN    KLOG_CAT_KERNEL
#define LOG_MEM     KLOG_CAT_MEMORY
#define LOG_PROC    KLOG_CAT_PROCESS
#define LOG_FS      KLOG_CAT_FS
#define LOG_DRV     KLOG_CAT_DRIVER
#define LOG_SYSCALL KLOG_CAT_SYSCALL
#define LOG_IPC     KLOG_CAT_IPC
#define LOG_SEC     KLOG_CAT_SECURITY
#define LOG_NET     KLOG_CAT_NETWORK

#define LOG_DBG     KLOG_DEBUG
#define LOG_INFO    KLOG_INFO
#define LOG_NOTE    KLOG_NOTICE
#define LOG_WARN    KLOG_WARN
#define LOG_ERR     KLOG_ERROR
#define LOG_CRIT    KLOG_CRITICAL

#define klog_boot(fmt, ...)    klog_info(LOG_BOOT, fmt, ##__VA_ARGS__)
#define klog_init_msg(fmt, ...) klog_info(LOG_INIT, fmt, ##__VA_ARGS__)
#define klog_kern(fmt, ...)    klog_info(LOG_KERN, fmt, ##__VA_ARGS__)
#define klog_mem(fmt, ...)     klog_info(LOG_MEM, fmt, ##__VA_ARGS__)
#define klog_proc(fmt, ...)    klog_info(LOG_PROC, fmt, ##__VA_ARGS__)
#define klog_fs(fmt, ...)      klog_info(LOG_FS, fmt, ##__VA_ARGS__)
#define klog_drv(fmt, ...)     klog_info(LOG_DRV, fmt, ##__VA_ARGS__)
#define klog_net(fmt, ...)     klog_info(LOG_NET, fmt, ##__VA_ARGS__)
#define klog_syscall(fmt, ...) klog_info(LOG_SYSCALL, fmt, ##__VA_ARGS__)
#define klog_ipc(fmt, ...)    klog_info(LOG_IPC, fmt, ##__VA_ARGS__)

#define klog_boot_warn(fmt, ...)  klog_warn(LOG_BOOT, fmt, ##__VA_ARGS__)
#define klog_init_warn(fmt, ...)  klog_warn(LOG_INIT, fmt, ##__VA_ARGS__)
#define klog_kern_warn(fmt, ...)  klog_warn(LOG_KERN, fmt, ##__VA_ARGS__)
#define klog_mem_warn(fmt, ...)   klog_warn(LOG_MEM, fmt, ##__VA_ARGS__)
#define klog_proc_warn(fmt, ...)  klog_warn(LOG_PROC, fmt, ##__VA_ARGS__)
#define klog_fs_warn(fmt, ...)    klog_warn(LOG_FS, fmt, ##__VA_ARGS__)
#define klog_drv_warn(fmt, ...)   klog_warn(LOG_DRV, fmt, ##__VA_ARGS__)
#define klog_net_warn(fmt, ...)    klog_warn(LOG_NET, fmt, ##__VA_ARGS__)
#define klog_ipc_warn(fmt, ...)    klog_warn(LOG_IPC, fmt, ##__VA_ARGS__)
#define klog_syscall_warn(fmt, ...) klog_warn(LOG_SYSCALL, fmt, ##__VA_ARGS__)

#define klog_boot_err(fmt, ...)   klog_error(LOG_BOOT, fmt, ##__VA_ARGS__)
#define klog_init_err(fmt, ...)   klog_error(LOG_INIT, fmt, ##__VA_ARGS__)
#define klog_kern_err(fmt, ...)   klog_error(LOG_KERN, fmt, ##__VA_ARGS__)
#define klog_mem_err(fmt, ...)    klog_error(LOG_MEM, fmt, ##__VA_ARGS__)
#define klog_proc_err(fmt, ...)   klog_error(LOG_PROC, fmt, ##__VA_ARGS__)
#define klog_fs_err(fmt, ...)     klog_error(LOG_FS, fmt, ##__VA_ARGS__)
#define klog_drv_err(fmt, ...)    klog_error(LOG_DRV, fmt, ##__VA_ARGS__)
#define klog_net_err(fmt, ...)    klog_error(LOG_NET, fmt, ##__VA_ARGS__)
#define klog_ipc_err(fmt, ...)    klog_error(LOG_IPC, fmt, ##__VA_ARGS__)
#define klog_syscall_err(fmt, ...) klog_error(LOG_SYSCALL, fmt, ##__VA_ARGS__)

#define klog_boot_crit(fmt, ...)   klog_crit(LOG_BOOT, fmt, ##__VA_ARGS__)
#define klog_kern_crit(fmt, ...)   klog_crit(LOG_KERN, fmt, ##__VA_ARGS__)
#define klog_mem_crit(fmt, ...)    klog_crit(LOG_MEM, fmt, ##__VA_ARGS__)
#define klog_drv_crit(fmt, ...)    klog_crit(LOG_DRV, fmt, ##__VA_ARGS__)
#define klog_net_crit(fmt, ...)    klog_crit(LOG_NET, fmt, ##__VA_ARGS__)

#define klog_sec_info(fmt, ...)  klog_info(LOG_SEC, fmt, ##__VA_ARGS__)
#define klog_sec_warn(fmt, ...)  klog_warn(LOG_SEC, fmt, ##__VA_ARGS__)
#define klog_sec_err(fmt, ...)   klog_error(LOG_SEC, fmt, ##__VA_ARGS__)

int printk(const char *fmt, ...);
int vprintk(const char *fmt, va_list args);
int snprintf(char *buf, size_t size, const char *fmt, ...);
int vsnprintf(char *buf, size_t size, const char *fmt, va_list args);

#define pr_debug(fmt, ...)  klog_debug(KLOG_CAT_GENERAL, fmt, ##__VA_ARGS__)
#define pr_info(fmt, ...)   klog_info(KLOG_CAT_GENERAL, fmt, ##__VA_ARGS__)
#define pr_notice(fmt, ...) klog_notice(KLOG_CAT_GENERAL, fmt, ##__VA_ARGS__)
#define pr_warn(fmt, ...)   klog_warn(KLOG_CAT_GENERAL, fmt, ##__VA_ARGS__)
#define pr_err(fmt, ...)    klog_error(KLOG_CAT_GENERAL, fmt, ##__VA_ARGS__)
#define pr_crit(fmt, ...)   klog_crit(KLOG_CAT_GENERAL, fmt, ##__VA_ARGS__)

#endif
