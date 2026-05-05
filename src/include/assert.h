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
        klog_kern_crit("ASSERT FAILED: %s", #cond); \
        klog_kern_crit("  File: %s:%d", __FILE__, __LINE__); \
        panic("Assertion failed"); \
    } \
} while(0)
#endif

#define ASSERT_MSG(cond, msg) do { \
    if (!(cond)) { \
        klog_kern_crit("ASSERT FAILED: %s", msg); \
        klog_kern_crit("  Condition: %s", #cond); \
        klog_kern_crit("  File: %s:%d", __FILE__, __LINE__); \
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
    klog_kern_crit("UNREACHABLE: %s:%d", __FILE__, __LINE__); \
    panic("Unreachable code reached"); \
} while(0)

#define NOT_IMPLEMENTED() do { \
    klog_kern_crit("NOT IMPLEMENTED: %s:%d", __FILE__, __LINE__); \
    panic("Function not implemented"); \
} while(0)

#endif
