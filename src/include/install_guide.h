#ifndef _INSTALL_GUIDE_H
#define _INSTALL_GUIDE_H

#include "types.h"

#define INSTALL_MARKER_FILE "/.antx_installed"
#define INSTALL_HOSTNAME_FILE "/etc/hostname"

#define INSTALL_DEFAULT_HOSTNAME "localhost"
#define INSTALL_MIN_PASSWORD_LEN 4

void install_guide_run(void);
int install_guide_check_needed(void);
int install_guide_create_marker(void);

#endif
