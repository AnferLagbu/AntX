#include "klog.h"
#include "serial.h"
#include "string.h"
#include "io.h"

#define KLOG_SERIAL_PORT SERIAL_COM1

static char klog_buffer[KLOG_BUFFER_SIZE];
static uint64_t klog_head = 0;
static uint64_t klog_tail = 0;
static uint64_t klog_entry_count = 0;
static int klog_initialized = 0;

static klog_level_t klog_level = KLOG_DEFAULT_LEVEL;
static uint32_t klog_flags = KLOG_DEFAULT_FLAGS;
static klog_level_t klog_cat_levels[KLOG_CAT_MAX];

static const char *level_strings[] = {
    "DEBUG", "INFO", "NOTE", "WARN", "ERROR", "CRIT"
};

static const char *level_prefixes[] = {
    "[DBG]  ", "[INFO] ", "[NOTE] ", "[WARN] ", "[ERR]  ", "[CRIT] "
};

static const char *category_strings[] = {
    "GENERAL", "BOOT", "INIT", "KERNEL", "MEMORY", "PROCESS",
    "FS", "DRIVER", "SYSCALL", "IPC", "SECURITY", "NETWORK"
};

static uint64_t get_timestamp(void) {
    uint64_t tsc;
    __asm__ volatile("rdtsc" : "=A"(tsc));
    return tsc;
}

static uint32_t get_cpu_id(void) {
    return 0;
}

void klog_init(void) {
    memset(klog_buffer, 0, KLOG_BUFFER_SIZE);
    klog_head = 0;
    klog_tail = 0;
    klog_entry_count = 0;
    klog_level = KLOG_DEFAULT_LEVEL;
    klog_flags = KLOG_DEFAULT_FLAGS;
    
    for (int i = 0; i < KLOG_CAT_MAX; i++) {
        klog_cat_levels[i] = KLOG_DEFAULT_LEVEL;
    }
    
    klog_initialized = 1;
    
    klog_info(LOG_KERN, "KLog system v%s initialized", KLOG_VERSION);
    klog_info(LOG_KERN, "Buffer size: %d bytes", KLOG_BUFFER_SIZE);
}

void klog_set_level(klog_level_t level) {
    if (level >= KLOG_DEBUG && level < KLOG_MAX_LEVEL) {
        klog_level = level;
    }
}

klog_level_t klog_get_level(void) {
    return klog_level;
}

void klog_set_flags(uint32_t flags) {
    klog_flags = flags;
}

uint32_t klog_get_flags(void) {
    return klog_flags;
}

void klog_set_category_level(klog_category_t cat, klog_level_t level) {
    if (cat >= 0 && cat < KLOG_CAT_MAX && level >= KLOG_DEBUG && level < KLOG_MAX_LEVEL) {
        klog_cat_levels[cat] = level;
    }
}

const char *klog_level_string(klog_level_t level) {
    if (level >= KLOG_DEBUG && level < KLOG_MAX_LEVEL) {
        return level_strings[level];
    }
    return "UNKNOWN";
}

const char *klog_category_string(klog_category_t cat) {
    if (cat >= 0 && cat < KLOG_CAT_MAX) {
        return category_strings[cat];
    }
    return "UNKNOWN";
}

static void buffer_write_char(char c) {
    klog_buffer[klog_tail] = c;
    klog_tail = (klog_tail + 1) % KLOG_BUFFER_SIZE;
    
    if (klog_tail == klog_head) {
        klog_head = (klog_head + 1) % KLOG_BUFFER_SIZE;
    }
}

static void buffer_write_str(const char *s) {
    while (*s) {
        buffer_write_char(*s++);
    }
}

static void serial_write_char(char c) {
    serial_putc(KLOG_SERIAL_PORT, c);
}

static void serial_write_str(const char *s) {
    while (*s) {
        if (*s == '\n') {
            serial_write_char('\r');
        }
        serial_write_char(*s++);
    }
}

extern int vsnprintf(char *buf, size_t size, const char *fmt, va_list args);
extern int snprintf(char *buf, size_t size, const char *fmt, ...);

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
                p = num_to_str(p, num, 16, 0, 16, '0');
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

int printk(const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    int ret = klog_vwrite(KLOG_INFO, KLOG_CAT_GENERAL, NULL, NULL, 0, fmt, args);
    va_end(args);
    return ret;
}

int vprintk(const char *fmt, va_list args) {
    return klog_vwrite(KLOG_INFO, KLOG_CAT_GENERAL, NULL, NULL, 0, fmt, args);
}

int klog_vwrite(klog_level_t level, klog_category_t cat,
                const char *file, const char *func, int line,
                const char *fmt, va_list args) {
    if (!klog_initialized) {
        return -1;
    }
    
    if (level < klog_level && level < klog_cat_levels[cat]) {
        return 0;
    }
    
    char msg_buf[KLOG_LINE_MAX];
    int msg_len = vsnprintf(msg_buf, sizeof(msg_buf), fmt, args);
    
    char output_buf[KLOG_LINE_MAX + 256];
    char *p = output_buf;
    
    if (klog_flags & KLOG_FLAG_TIMESTAMP) {
        uint64_t ts = get_timestamp();
        p = num_to_str(p, ts / 1000000000ULL, 10, 0, 0, '0');
        *p++ = '.';
        p = num_to_str(p, ts % 1000000000ULL, 10, 0, 9, '0');
        *p++ = ' ';
    }
    
    const char *prefix = (level < KLOG_MAX_LEVEL) ? level_prefixes[level] : "[????] ";
    while (*prefix) *p++ = *prefix++;
    
    *p++ = '[';
    const char *cat_str = (cat < KLOG_CAT_MAX) ? category_strings[cat] : "????";
    while (*cat_str) *p++ = *cat_str++;
    *p++ = ']';
    *p++ = ' ';
    
    char *msg_p = msg_buf;
    while (*msg_p && p < output_buf + sizeof(output_buf) - 2) {
        *p++ = *msg_p++;
    }
    
    if (p[-1] != '\n') {
        *p++ = '\n';
    }
    *p = '\0';
    
    if (klog_flags & KLOG_FLAG_OUTPUT_SERIAL) {
        serial_write_str(output_buf);
    }
    
    if (klog_flags & KLOG_FLAG_OUTPUT_BUFFER) {
        buffer_write_str(output_buf);
        klog_entry_count++;
    }
    
    return msg_len;
}

int klog_write(klog_level_t level, klog_category_t cat,
               const char *file, const char *func, int line,
               const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    int ret = klog_vwrite(level, cat, file, func, line, fmt, args);
    va_end(args);
    return ret;
}

void klog_flush(void) {
    serial_write_str("\n");
}

void klog_dump(void) {
    serial_write_str("\n");
    serial_write_str("========================================\n");
    serial_write_str("KERNEL LOG DUMP\n");
    serial_write_str("========================================\n");
    serial_write_str("Entries: ");
    
    char num_buf[32];
    num_to_str(num_buf, klog_entry_count, 10, 0, 0, '0');
    serial_write_str(num_buf);
    serial_write_str("\n\n");
    
    if (!klog_initialized || klog_head == klog_tail) {
        serial_write_str("[Log empty or not initialized]\n");
        serial_write_str("========================================\n");
        return;
    }
    
    uint64_t pos = klog_head;
    while (pos != klog_tail) {
        serial_write_char(klog_buffer[pos]);
        pos = (pos + 1) % KLOG_BUFFER_SIZE;
    }
    
    serial_write_str("\n========================================\n");
}

void klog_clear(void) {
    klog_head = 0;
    klog_tail = 0;
    klog_entry_count = 0;
    memset(klog_buffer, 0, KLOG_BUFFER_SIZE);
}

uint64_t klog_get_entry_count(void) {
    return klog_entry_count;
}

#define KLOG_DB_PATH "/cfg/system/klog.db"
#define KLOG_DB_MAGIC 0x4B4C4F47

int klog_save_to_disk(void) {
    extern int hvfs_open(const char *path, int flags, int mode);
    extern int hvfs_write(int fd, const void *buf, uint64_t count);
    extern int hvfs_close(int fd);
    
    int fd = hvfs_open(KLOG_DB_PATH, 0x200 | 0x01 | 0x40, 0);
    if (fd < 0) {
        return -1;
    }
    
    uint32_t header[4] = {KLOG_DB_MAGIC, KLOG_BUFFER_SIZE, (uint32_t)klog_head, (uint32_t)klog_tail};
    hvfs_write(fd, header, sizeof(header));
    hvfs_write(fd, klog_buffer, KLOG_BUFFER_SIZE);
    hvfs_close(fd);
    
    return 0;
}

int klog_load_from_disk(void) {
    extern int hvfs_open(const char *path, int flags, int mode);
    extern int hvfs_read(int fd, void *buf, uint64_t count);
    extern int hvfs_close(int fd);
    
    int fd = hvfs_open(KLOG_DB_PATH, 0, 0);
    if (fd < 0) {
        return -1;
    }
    
    uint32_t header[4];
    if (hvfs_read(fd, header, sizeof(header)) != sizeof(header)) {
        hvfs_close(fd);
        return -1;
    }
    
    if (header[0] != KLOG_DB_MAGIC) {
        hvfs_close(fd);
        return -1;
    }
    
    hvfs_read(fd, klog_buffer, KLOG_BUFFER_SIZE);
    klog_head = header[2];
    klog_tail = header[3];
    hvfs_close(fd);
    
    return 0;
}

int klog_get_entry(uint64_t index, klog_entry_t *entry) {
    if (entry == NULL || index >= klog_entry_count) {
        return -1;
    }

    memset(entry, 0, sizeof(klog_entry_t));
    return 0;
}

void klog_ffi_info(const char *msg) {
    klog_write(KLOG_INFO, KLOG_CAT_KERNEL, "ffi", NULL, 0, "%s", msg);
}

void klog_ffi_warn(const char *msg) {
    klog_write(KLOG_WARN, KLOG_CAT_KERNEL, "ffi", NULL, 0, "%s", msg);
}

void klog_ffi_error(const char *msg) {
    klog_write(KLOG_ERROR, KLOG_CAT_KERNEL, "ffi", NULL, 0, "%s", msg);
}
