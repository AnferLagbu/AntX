#ifndef _PRINTK_H
#define _PRINTK_H

#include "types.h"
#include "stdarg.h"

#define LOG_LEVEL_DEBUG    0
#define LOG_LEVEL_INFO     1
#define LOG_LEVEL_NOTICE   2
#define LOG_LEVEL_WARN     3
#define LOG_LEVEL_ERROR    4
#define LOG_LEVEL_CRITICAL 5

#ifndef LOG_LEVEL
#define LOG_LEVEL LOG_LEVEL_INFO
#endif

#define KERN_DEBUG  "[DBG]  "
#define KERN_INFO   "[INFO] "
#define KERN_NOTICE "[NOTE] "
#define KERN_WARN   "[WARN] "
#define KERN_ERR    "[ERR]  "
#define KERN_CRIT   "[CRIT] "

int printk(const char *fmt, ...);
int vprintk(const char *fmt, va_list args);
int snprintf(char *buf, size_t size, const char *fmt, ...);
int vsnprintf(char *buf, size_t size, const char *fmt, va_list args);

void printk_set_level(int level);
int printk_get_level(void);

#if LOG_LEVEL <= LOG_LEVEL_DEBUG
#define pr_debug(fmt, ...) printk(KERN_DEBUG fmt, ##__VA_ARGS__)
#else
#define pr_debug(fmt, ...) ((void)0)
#endif

#if LOG_LEVEL <= LOG_LEVEL_INFO
#define pr_info(fmt, ...) printk(KERN_INFO fmt, ##__VA_ARGS__)
#else
#define pr_info(fmt, ...) ((void)0)
#endif

#if LOG_LEVEL <= LOG_LEVEL_NOTICE
#define pr_notice(fmt, ...) printk(KERN_NOTICE fmt, ##__VA_ARGS__)
#else
#define pr_notice(fmt, ...) ((void)0)
#endif

#if LOG_LEVEL <= LOG_LEVEL_WARN
#define pr_warn(fmt, ...) printk(KERN_WARN fmt, ##__VA_ARGS__)
#else
#define pr_warn(fmt, ...) ((void)0)
#endif

#if LOG_LEVEL <= LOG_LEVEL_ERROR
#define pr_err(fmt, ...) printk(KERN_ERR fmt, ##__VA_ARGS__)
#else
#define pr_err(fmt, ...) ((void)0)
#endif

#define pr_crit(fmt, ...) printk(KERN_CRIT fmt, ##__VA_ARGS__)

#define KLOG_BOOT   "[BOOT] "
#define KLOG_INIT   "[INIT] "
#define KLOG_KERN   "[KERN] "
#define KLOG_PROC   "[PROC] "
#define KLOG_MEM    "[MEM]  "
#define KLOG_FS     "[FS]   "
#define KLOG_DRV    "[DRV]  "

#define klog_boot(fmt, ...)   printk(KLOG_BOOT fmt, ##__VA_ARGS__)
#define klog_init(fmt, ...)   printk(KLOG_INIT fmt, ##__VA_ARGS__)
#define klog_kern(fmt, ...)   printk(KLOG_KERN fmt, ##__VA_ARGS__)
#define klog_proc(fmt, ...)   printk(KLOG_PROC fmt, ##__VA_ARGS__)
#define klog_mem(fmt, ...)    printk(KLOG_MEM fmt, ##__VA_ARGS__)
#define klog_fs(fmt, ...)     printk(KLOG_FS fmt, ##__VA_ARGS__)
#define klog_drv(fmt, ...)    printk(KLOG_DRV fmt, ##__VA_ARGS__)

#endif
