#ifndef _SERIAL_H
#define _SERIAL_H

#include "types.h"

#define SERIAL_COM1 0x3F8
#define SERIAL_COM2 0x2F8
#define SERIAL_COM3 0x3E8
#define SERIAL_COM4 0x2E8

#define SERIAL_DATA_REG(port)        (port)
#define SERIAL_INT_ENABLE_REG(port)  (port + 1)
#define SERIAL_FIFO_CTRL_REG(port)   (port + 2)
#define SERIAL_LINE_CTRL_REG(port)   (port + 3)
#define SERIAL_MODEM_CTRL_REG(port)  (port + 4)
#define SERIAL_LINE_STATUS_REG(port) (port + 5)

void serial_init(uint16_t port);
void serial_putc(uint16_t port, char c);
void serial_puts(uint16_t port, const char *s);
void serial_write(uint16_t port, const void *buf, uint64_t count);
void serial_put_hex(uint16_t port, uint64_t val);
void serial_put_dec(uint16_t port, int64_t val);

int serial_has_data(uint16_t port);
int serial_getc(uint16_t port);

#endif
