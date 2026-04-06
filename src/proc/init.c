#include "user_proc.h"
#include "proc.h"
#include "mm.h"
#include "gdt.h"
#include "serial.h"
#include "string.h"
#include "vfs.h"

static uint8_t user_init_code[] = {
    0xB8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00,
    0x48, 0x89, 0xF8,
    0xEB, 0xFE
};

struct user_proc_info user_programs[] = {
    { (void(*)(void))USER_CODE_BASE, "init", sizeof(user_init_code), user_init_code },
    { NULL, NULL, 0, NULL }
};

static struct process *init_proc = NULL;

void init_start(void) {
    serial_puts(SERIAL_COM1, "Init process framework ready\n");
    serial_puts(SERIAL_COM1, "User mode support initialized\n");
}
