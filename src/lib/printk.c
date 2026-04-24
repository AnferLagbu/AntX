#include "printk.h"
#include "serial.h"
#include "string.h"

static char printk_buf[1024];
static int current_log_level = LOG_LEVEL_INFO;

void printk_set_level(int level) {
    if (level >= LOG_LEVEL_DEBUG && level <= LOG_LEVEL_CRITICAL) {
        current_log_level = level;
    }
}

int printk_get_level(void) {
    return current_log_level;
}

static void puts_serial(const char *s) {
    while (*s) {
        serial_putc(SERIAL_COM1, *s++);
    }
}

static char *num_to_str(char *buf, uint64_t num, int base, int is_signed, int width, char pad) {
    char tmp[32];
    int i = 0;
    int neg = 0;
    
    if (is_signed && (int64_t)num < 0) {
        neg = 1;
        num = -(int64_t)num;
    }
    
    if (num == 0) {
        tmp[i++] = '0';
    } else {
        while (num > 0) {
            int digit = num % base;
            tmp[i++] = (digit < 10) ? ('0' + digit) : ('a' + digit - 10);
            num /= base;
        }
    }
    
    if (neg) {
        tmp[i++] = '-';
    }
    
    int len = i;
    while (len < width) {
        *buf++ = pad;
        width--;
    }
    
    while (i > 0) {
        *buf++ = tmp[--i];
    }
    
    return buf;
}

int vsnprintf(char *buf, size_t size, const char *fmt, va_list args) {
    char *p = buf;
    char *end = buf + size - 1;
    
    while (*fmt && p < end) {
        if (*fmt != '%') {
            *p++ = *fmt++;
            continue;
        }
        
        fmt++;
        
        char pad = ' ';
        int width = 0;
        
        if (*fmt == '0') {
            pad = '0';
            fmt++;
        }
        
        while (*fmt >= '0' && *fmt <= '9') {
            width = width * 10 + (*fmt - '0');
            fmt++;
        }
        
        int is_long = 0;
        if (*fmt == 'l') {
            is_long = 1;
            fmt++;
        }
        
        switch (*fmt) {
            case 'd':
            case 'i': {
                int64_t num = is_long ? va_arg(args, int64_t) : va_arg(args, int);
                p = num_to_str(p, num, 10, 1, width, pad);
                break;
            }
            case 'u': {
                uint64_t num = is_long ? va_arg(args, uint64_t) : va_arg(args, unsigned int);
                p = num_to_str(p, num, 10, 0, width, pad);
                break;
            }
            case 'x':
            case 'X': {
                uint64_t num = is_long ? va_arg(args, uint64_t) : va_arg(args, unsigned int);
                p = num_to_str(p, num, 16, 0, width, pad);
                break;
            }
            case 'p': {
                uint64_t num = (uint64_t)va_arg(args, void *);
                if (p + 2 < end) {
                    *p++ = '0';
                    *p++ = 'x';
                }
                p = num_to_str(p, num, 16, 0, width, pad);
                break;
            }
            case 's': {
                const char *s = va_arg(args, const char *);
                if (s == NULL) s = "(null)";
                while (*s && p < end) {
                    *p++ = *s++;
                }
                break;
            }
            case 'c': {
                char c = (char)va_arg(args, int);
                if (p < end) *p++ = c;
                break;
            }
            case '%':
                if (p < end) *p++ = '%';
                break;
            default:
                if (p < end) *p++ = '%';
                if (p < end) *p++ = *fmt;
                break;
        }
        fmt++;
    }
    
    *p = '\0';
    return p - buf;
}

int snprintf(char *buf, size_t size, const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    int ret = vsnprintf(buf, size, fmt, args);
    va_end(args);
    return ret;
}

int vprintk(const char *fmt, va_list args) {
    int ret = vsnprintf(printk_buf, sizeof(printk_buf), fmt, args);
    puts_serial(printk_buf);
    return ret;
}

int printk(const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    int ret = vprintk(fmt, args);
    va_end(args);
    return ret;
}
