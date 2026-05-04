#ifndef _STDDEF_H
#define _STDDEF_H

#include "types.h"

#define offsetof(type, member) __builtin_offsetof(type, member)

typedef int ptrdiff_t;

#endif
