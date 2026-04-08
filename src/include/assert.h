#ifndef _ASSERT_H
#define _ASSERT_H

#include "kernel.h"

#define STRINGIFY_IMPL(x) #x
#define STRINGIFY(x) STRINGIFY_IMPL(x)

#ifdef NDEBUG
#define ASSERT(cond) ((void)0)
#else
#define ASSERT(cond) do { \
    if (!(cond)) { \
        serial_puts(SERIAL_COM1, "\n[ASSERT FAILED] "); \
        serial_puts(SERIAL_COM1, #cond); \
        serial_puts(SERIAL_COM1, "\n  File: " __FILE__); \
        serial_puts(SERIAL_COM1, "\n  Line: " STRINGIFY(__LINE__)); \
        serial_puts(SERIAL_COM1, "\n"); \
        panic("Assertion failed"); \
    } \
} while(0)
#endif

#define ASSERT_MSG(cond, msg) do { \
    if (!(cond)) { \
        serial_puts(SERIAL_COM1, "\n[ASSERT FAILED] "); \
        serial_puts(SERIAL_COM1, msg); \
        serial_puts(SERIAL_COM1, "\n  Condition: " #cond); \
        serial_puts(SERIAL_COM1, "\n  File: " __FILE__); \
        serial_puts(SERIAL_COM1, "\n  Line: " STRINGIFY(__LINE__)); \
        serial_puts(SERIAL_COM1, "\n"); \
        panic("Assertion failed: " msg); \
    } \
} while(0)

#define STATIC_ASSERT(cond, msg) _Static_assert(cond, msg)

#define PANIC_IF(cond, msg) do { \
    if (cond) { \
        panic(msg); \
    } \
} while(0)

#define UNREACHABLE() do { \
    serial_puts(SERIAL_COM1, "\n[UNREACHABLE] "); \
    serial_puts(SERIAL_COM1, __FILE__); \
    serial_puts(SERIAL_COM1, ":" STRINGIFY(__LINE__)); \
    serial_puts(SERIAL_COM1, "\n"); \
    panic("Unreachable code reached"); \
} while(0)

#define NOT_IMPLEMENTED() do { \
    serial_puts(SERIAL_COM1, "\n[NOT IMPLEMENTED] "); \
    serial_puts(SERIAL_COM1, __FILE__); \
    serial_puts(SERIAL_COM1, ":" STRINGIFY(__LINE__)); \
    serial_puts(SERIAL_COM1, "\n"); \
    panic("Function not implemented"); \
} while(0)

#endif
