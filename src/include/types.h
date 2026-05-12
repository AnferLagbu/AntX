#ifndef _KERNEL_TYPES_H
#define _KERNEL_TYPES_H

typedef unsigned char      uint8_t;
typedef unsigned short     uint16_t;
typedef unsigned int       uint32_t;
typedef unsigned long long uint64_t;
typedef signed char        int8_t;
typedef signed short       int16_t;
typedef signed int         int32_t;
typedef signed long long   int64_t;

typedef unsigned long      uintptr_t;
typedef unsigned long      size_t;
typedef long               ssize_t;
typedef long               ptrdiff_t;

#define SSIZE_MAX 0x7FFFFFFFFFFFFFFFLL

#define NULL ((void*)0)
typedef int                bool;
#define true  1
#define false 0

#define UINT32_MAX  0xFFFFFFFFu
#define UINT16_MAX  0xFFFFu
#define INT32_MAX   0x7FFFFFFF
#define INT_MAX     0x7FFFFFFF

#endif
