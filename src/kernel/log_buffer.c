#include "log_buffer.h"
#include "io.h"
#include "string.h"

#define SERIAL_COM1 0x3F8

static char log_buffer[LOG_BUFFER_SIZE];
static uint32_t log_head = 0;
static uint32_t log_tail = 0;
static int log_initialized = 0;

void log_init(void) {
    memset(log_buffer, 0, LOG_BUFFER_SIZE);
    log_head = 0;
    log_tail = 0;
    log_initialized = 1;
}

void log_write_char(char c) {
    if (!log_initialized) return;
    
    log_buffer[log_tail] = c;
    log_tail = (log_tail + 1) % LOG_BUFFER_SIZE;
    
    if (log_tail == log_head) {
        log_head = (log_head + 1) % LOG_BUFFER_SIZE;
    }
}

void log_puts(const char *str) {
    if (!str) return;
    
    while (*str) {
        log_write_char(*str++);
    }
}

void log_put_hex(uint64_t value) {
    static const char hex_chars[] = "0123456789ABCDEF";
    char buffer[19];
    buffer[0] = '0';
    buffer[1] = 'x';
    
    for (int i = 15; i >= 0; i--) {
        buffer[17 - i] = hex_chars[value & 0xF];
        value >>= 4;
    }
    
    buffer[18] = '\0';
    log_puts(buffer);
}

void log_put_dec(int64_t value) {
    char buffer[21];
    int pos = 20;
    int negative = 0;
    uint64_t uval;
    
    buffer[pos] = '\0';
    
    if (value < 0) {
        negative = 1;
        uval = (uint64_t)(-value);
    } else {
        uval = (uint64_t)value;
    }
    
    if (uval == 0) {
        buffer[--pos] = '0';
    } else {
        while (uval > 0 && pos > 0) {
            buffer[--pos] = '0' + (uval % 10);
            uval /= 10;
        }
    }
    
    if (negative && pos > 0) {
        buffer[--pos] = '-';
    }
    
    log_puts(&buffer[pos]);
}

static void log_direct_putc(char c) {
    while ((inb(SERIAL_COM1 + 5) & 0x20) == 0);
    outb(SERIAL_COM1, c);
}

static void log_direct_puts(const char *s) {
    while (*s) {
        if (*s == '\n') {
            log_direct_putc('\r');
        }
        log_direct_putc(*s++);
    }
}

void log_flush(void) {
    log_direct_puts("\n");
}

void log_dump_all(void) {
    log_direct_puts("\n");
    log_direct_puts("========================================\n");
    log_direct_puts("KERNEL LOG BUFFER\n");
    log_direct_puts("========================================\n");
    log_direct_puts("\n");
    
    if (!log_initialized || log_head == log_tail) {
        log_direct_puts("[Log empty or not initialized]\n");
        return;
    }
    
    uint32_t pos = log_head;
    while (pos != log_tail) {
        log_direct_putc(log_buffer[pos]);
        pos = (pos + 1) % LOG_BUFFER_SIZE;
    }
    
    log_direct_puts("\n========================================\n");
}
