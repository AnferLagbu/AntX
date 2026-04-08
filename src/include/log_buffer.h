#ifndef _LOG_BUFFER_H
#define _LOG_BUFFER_H

#include "types.h"

#define LOG_BUFFER_SIZE    (64 * 1024)
#define LOG_LINE_MAX       512

void log_init(void);
void log_write_char(char c);
void log_puts(const char *str);
void log_put_hex(uint64_t value);
void log_put_dec(int64_t value);
void log_flush(void);
void log_dump_all(void);

#endif