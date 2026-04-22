#ifndef _MODULE_CHECK_H
#define _MODULE_CHECK_H

#include "types.h"

#define MODULE_INIT_SUCCESS 0
#define MODULE_INIT_FAIL    1

#define MODULE_CHECK(name, init_func) do { \
    int result = init_func(); \
    if (result != MODULE_INIT_SUCCESS) { \
        serial_puts(SERIAL_COM1, "  [FAIL] "); \
        serial_puts(SERIAL_COM1, name); \
        serial_puts(SERIAL_COM1, "\n"); \
        panic("Module initialization failed: " name); \
    } else { \
        serial_puts(SERIAL_COM1, "  [OK] "); \
        serial_puts(SERIAL_COM1, name); \
        serial_puts(SERIAL_COM1, "\n"); \
    } \
} while(0)

#define MODULE_CHECK_MSG(name, init_func, msg) do { \
    int result = init_func(); \
    if (result != MODULE_INIT_SUCCESS) { \
        serial_puts(SERIAL_COM1, "  [FAIL] "); \
        serial_puts(SERIAL_COM1, name); \
        serial_puts(SERIAL_COM1, " - "); \
        serial_puts(SERIAL_COM1, msg); \
        serial_puts(SERIAL_COM1, "\n"); \
        panic("Module initialization failed: " name); \
    } else { \
        serial_puts(SERIAL_COM1, "  [OK] "); \
        serial_puts(SERIAL_COM1, name); \
        if (msg && msg[0]) { \
            serial_puts(SERIAL_COM1, " - "); \
            serial_puts(SERIAL_COM1, msg); \
        } \
        serial_puts(SERIAL_COM1, "\n"); \
    } \
} while(0)

#define MODULE_CHECK_VOID(name, init_func) do { \
    init_func(); \
    serial_puts(SERIAL_COM1, "  [OK] "); \
    serial_puts(SERIAL_COM1, name); \
    serial_puts(SERIAL_COM1, "\n"); \
} while(0)

#define MODULE_CHECK_VOID_MSG(name, init_func, msg) do { \
    init_func(); \
    serial_puts(SERIAL_COM1, "  [OK] "); \
    serial_puts(SERIAL_COM1, name); \
    if (msg && msg[0]) { \
        serial_puts(SERIAL_COM1, " - "); \
        serial_puts(SERIAL_COM1, msg); \
    } \
    serial_puts(SERIAL_COM1, "\n"); \
} while(0)

#define MODULE_CHECK_CUSTOM(name, init_func, check_expr) do { \
    init_func(); \
    if (!(check_expr)) { \
        serial_puts(SERIAL_COM1, "  [FAIL] "); \
        serial_puts(SERIAL_COM1, name); \
        serial_puts(SERIAL_COM1, "\n"); \
        panic("Module initialization failed: " name); \
    } else { \
        serial_puts(SERIAL_COM1, "  [OK] "); \
        serial_puts(SERIAL_COM1, name); \
        serial_puts(SERIAL_COM1, "\n"); \
    } \
} while(0)

#endif
