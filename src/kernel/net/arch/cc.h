#ifndef QX_CC_H
#define QX_CC_H

/* ============================================================
 * lwIP 编译器/平台适配 — AntX (QueenX) 内核
 * ============================================================ */

#include "types.h"
#include "string.h"

/* ---- 避免 lwIP arch.h 包含标准 C 头文件 ---- */
#define LWIP_NO_STDINT_H    1
#define LWIP_NO_INTTYPES_H  1
#define LWIP_NO_LIMITS_H    1
#define LWIP_NO_CTYPE_H     1
#define LWIP_NO_STDDEF_H    1
#define LWIP_NO_UNISTD_H    1
#define LWIP_NO_STDIO_H     1
#define LWIP_NO_STDLIB_H    1

/* ---- 标准常量 (替代 limits.h) ---- */
#ifndef INT_MAX
#define INT_MAX    2147483647
#endif
#ifndef UINT_MAX
#define UINT_MAX   4294967295U
#endif

/* ---- 让 lwIP 提供自己的 errno ---- */
#define LWIP_PROVIDE_ERRNO  1

/* ---- 基础类型 ---- */
typedef uint8_t   u8_t;
typedef int8_t    s8_t;
typedef uint16_t  u16_t;
typedef int16_t   s16_t;
typedef uint32_t  u32_t;
typedef int32_t   s32_t;
typedef uint64_t  u64_t;

typedef uintptr_t mem_ptr_t;

/* ---- 编译器宏 ---- */
#define PACK_STRUCT_STRUCT  __attribute__((packed))
#define PACK_STRUCT_BEGIN
#define PACK_STRUCT_END

#define LWIP_PLATFORM_BYTESWAP  1

#define BYTE_ORDER  LITTLE_ENDIAN

/* ---- 诊断宏 ---- */
#define LWIP_PLATFORM_DIAG(x)   do { } while (0)
#define LWIP_PLATFORM_ASSERT(x) do { } while (0)

/* ---- 随机数 ---- */
#define LWIP_RAND()  ((u32_t)0xDEADBEEF)  /* Phase 2: 替换为 TSC */

/* ---- printf 支持 (可选) ---- */
#define LWIP_PLATFORM_PRINTF

#endif /* QX_CC_H */
