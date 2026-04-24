#ifndef _MODULE_CHECK_H
#define _MODULE_CHECK_H

#include "types.h"
#include "klog.h"

#define MODULE_INIT_SUCCESS 0
#define MODULE_INIT_FAIL    1

#define MODULE_CHECK(name, init_func) do { \
    int result = init_func(); \
    if (result != MODULE_INIT_SUCCESS) { \
        pr_err("%s initialization failed\n", name); \
        panic("Module initialization failed: " name); \
    } else { \
        pr_info("%s initialized\n", name); \
    } \
} while(0)

#define MODULE_CHECK_MSG(name, init_func, msg) do { \
    int result = init_func(); \
    if (result != MODULE_INIT_SUCCESS) { \
        pr_err("%s - %s\n", name, msg); \
        panic("Module initialization failed: " name); \
    } else { \
        pr_info("%s - %s\n", name, msg); \
    } \
} while(0)

#define MODULE_CHECK_VOID(name, init_func) do { \
    init_func(); \
    pr_info("%s initialized\n", name); \
} while(0)

#define MODULE_CHECK_VOID_MSG(name, init_func, msg) do { \
    init_func(); \
    pr_info("%s - %s\n", name, msg); \
} while(0)

#define MODULE_CHECK_CUSTOM(name, init_func, check_expr) do { \
    init_func(); \
    if (!(check_expr)) { \
        pr_err("%s check failed\n", name); \
        panic("Module initialization failed: " name); \
    } else { \
        pr_info("%s initialized\n", name); \
    } \
} while(0)

#endif
