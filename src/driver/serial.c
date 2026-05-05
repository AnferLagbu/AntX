#include "serial.h"
#include "io.h"

static int serial_is_transmit_empty(uint16_t port) {
    return inb(SERIAL_LINE_STATUS_REG(port)) & 0x20;
}

void serial_init(uint16_t port) {
    outb(SERIAL_INT_ENABLE_REG(port), 0x00);
    outb(SERIAL_LINE_CTRL_REG(port), 0x80);
    outb(SERIAL_DATA_REG(port), 0x03);
    outb(SERIAL_INT_ENABLE_REG(port), 0x00);
    outb(SERIAL_LINE_CTRL_REG(port), 0x03);
    outb(SERIAL_FIFO_CTRL_REG(port), 0xC7);
    outb(SERIAL_MODEM_CTRL_REG(port), 0x0B);
    outb(SERIAL_INT_ENABLE_REG(port), 0x00);
}

int serial_has_data(uint16_t port) {
    return inb(SERIAL_LINE_STATUS_REG(port)) & 0x01;
}

int serial_getc(uint16_t port) {
    while (!serial_has_data(port)) {
        __asm__ volatile ("pause");
    }
    return inb(SERIAL_DATA_REG(port));
}

void serial_putc(uint16_t port, char c) {
    while (serial_is_transmit_empty(port) == 0);
    outb(SERIAL_DATA_REG(port), c);
}

void serial_write(uint16_t port, const void *buf, uint64_t count) {
    if (buf == NULL || count == 0) return;
    const char *s = (const char *)buf;
    for (uint64_t i = 0; i < count; i++) {
        if (s[i] == '\n') {
            serial_putc(port, '\r');
        }
        serial_putc(port, s[i]);
    }
}
