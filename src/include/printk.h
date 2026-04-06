#ifndef _PRINTK_H
#define _PRINTK_H

#include "types.h"
#include "stdarg.h"

int printk(const char *fmt, ...);
int vprintk(const char *fmt, va_list args);
int snprintf(char *buf, size_t size, const char *fmt, ...);
int vsnprintf(char *buf, size_t size, const char *fmt, va_list args);

#define KERN_INFO  "[INFO] "
#define KERN_WARN  "[WARN] "
#define KERN_ERR   "[ERR]  "
#define KERN_DEBUG "[DBG]  "

#define pr_info(fmt, ...)  printk(KERN_INFO fmt, ##__VA_ARGS__)
#define pr_warn(fmt, ...)  printk(KERN_WARN fmt, ##__VA_ARGS__)
#define pr_err(fmt, ...)   printk(KERN_ERR fmt, ##__VA_ARGS__)
#define pr_debug(fmt, ...) printk(KERN_DEBUG fmt, ##__VA_ARGS__)

#endif
