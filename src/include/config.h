#ifndef _CONFIG_H
#define _CONFIG_H

#ifdef BUILD_RELEASE
    #define CONFIG_MODE_RELEASE 1
    #define CONFIG_MODE_TEST    0
    #define CONFIG_MODE_DEV     0
#elif defined(BUILD_TEST)
    #define CONFIG_MODE_RELEASE 0
    #define CONFIG_MODE_TEST    1
    #define CONFIG_MODE_DEV     0
#else
    #define CONFIG_MODE_RELEASE 0
    #define CONFIG_MODE_TEST    0
    #define CONFIG_MODE_DEV     1
#endif

#define CONFIG_PERSISTENT_AUTO_FORMAT  1
#define CONFIG_PERSISTENT_ASK_CONFIRM  1

#endif
