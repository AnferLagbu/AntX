#ifndef _BUILTINS_H
#define _BUILTINS_H

#include "user/user.h"

struct builtin_cmd {
    const char *name;
    int (*func)(int argc, char **argv);
};

extern struct builtin_cmd builtins[];

int shell_is_running(void);

#endif
