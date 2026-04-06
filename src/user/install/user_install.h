#ifndef _USER_INSTALL_H
#define _USER_INSTALL_H

#include "types.h"

#define INSTALL_MIN_PASSWORD_LEN  4
#define INSTALL_DEFAULT_HOSTNAME  "localhost"
#define INSTALL_MARKER_FILE       "/.antx_installed"
#define INSTALL_HOSTNAME_FILE     "/etc/hostname"

int user_install_check_needed(void);
int user_install_create_marker(void);
void user_install_run(void);

#endif
