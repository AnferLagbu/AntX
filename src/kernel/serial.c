#include "serial.h"
#include "io.h"

void serial_init(uint16_t port) {
    outb(SERIAL_INT_ENABLE_REG(port), 0x00);
    outb(SERIAL_LINE_CTRL_REG(port), 0x80);
    outb(SERIAL_DATA_REG(port), 0x03);
    outb(SERIAL_INT_ENABLE_REG(port), 0x00);
    outb(SERIAL_LINE_CTRL_REG(port), 0x03);
    outb(SERIAL_FIFO_CTRL_REG(port), 0xC7);
    outb(SERIAL_MODEM_CTRL_REG(port), 0x0B);
    outb(SERIAL_INT_ENABLE_REG(port), 0x01);
}

static int serial_is_transmit_empty(uint16_t port) {
    return inb(SERIAL_LINE_STATUS_REG(port)) & 0x20;
}

void serial_putc(uint16_t port, char c) {
    while (serial_is_transmit_empty(port) == 0);
    outb(SERIAL_DATA_REG(port), c);
}

void serial_puts(uint16_t port, const char *s) {
    while (*s) {
        if (*s == '\n') {
            serial_putc(port, '\r');
        }
        serial_putc(port, *s++);
    }
}

void serial_write(uint16_t port, const void *buf, uint64_t count) {
    const char *s = (const char *)buf;
    for (uint64_t i = 0; i < count; i++) {
        if (s[i] == '\n') {
            serial_putc(port, '\r');
        }
        serial_putc(port, s[i]);
    }
}

void serial_put_hex(uint16_t port, uint64_t val) {
    const char hex_chars[] = "0123456789ABCDEF";
    char buf[17];
    
    for (int i = 15; i >= 0; i--) {
        buf[i] = hex_chars[val & 0xF];
        val >>= 4;
    }
    buf[16] = '\0';
    
    serial_puts(port, "0x");
    serial_puts(port, buf);
}

void serial_put_dec(uint16_t port, int64_t val) {
    char buf[21];
    int i = 20;
    int neg = 0;
    
    buf[i] = '\0';
    
    if (val < 0) {
        neg = 1;
        val = -val;
    }
    
    if (val == 0) {
        serial_putc(port, '0');
        return;
    }
    
    while (val > 0 && i > 0) {
        buf[--i] = '0' + (val % 10);
        val /= 10;
    }
    
    if (neg) {
        buf[--i] = '-';
    }
    
    serial_puts(port, &buf[i]);
}
