#ifndef _TYPES_H
#define _TYPES_H

typedef unsigned char      uint8_t;
typedef unsigned short     uint16_t;
typedef unsigned int       uint32_t;
typedef unsigned long long uint64_t;

typedef signed char        int8_t;
typedef signed short       int16_t;
typedef signed int         int32_t;
typedef signed long long   int64_t;

typedef unsigned int       size_t;
typedef signed int         ssize_t;
typedef unsigned int       uintptr_t;
typedef signed int         intptr_t;
typedef signed long long   ptrdiff_t;

#define NULL ((void*)0)

#define true  1
#define false 0
typedef int bool;

#endif
