#ifndef _PWID_H
#define _PWID_H

#include "types.h"

#define MAX_PWID_ENTRIES    64
#define PWID_NOTE_LEN       32
#define PWID_HASH_LEN       32
#define PWID_PASSWORD_MAX   64

#define PWID_LEVEL_ROOT         0
#define PWID_LEVEL_TRUSTWORTHY  1
#define PWID_LEVEL_UNTRUSTWORTHY 2

#define PWID_FLAG_ORIGINAL_ROOT 0x01
#define PWID_FLAG_TEMPORARY     0x02
#define PWID_FLAG_DISABLED      0x04
#define PWID_FLAG_MODIFIED      0x08
#define PWID_FLAG_DEFAULT_PW    0x10

#define PWID_LEVEL_MASK     0xF000000000000000ULL
#define PWID_ORIGINAL_MASK  0x0800000000000000ULL
#define PWID_TEMP_MASK      0x0400000000000000ULL
#define PWID_MODIFIED_MASK  0x0200000000000000ULL
#define PWID_HASH_MASK      0x00FFFFFFFFFFFFFFULL

struct pwid_entry {
    uint64_t pwid;
    uint8_t level;
    char note[PWID_NOTE_LEN];
    uint8_t password_hash[PWID_HASH_LEN];
    uint8_t flags;
    uint64_t created_time;
    uint64_t expires_at;
};

struct pwid_context {
    struct pwid_entry *current;
    uint64_t session_pwid;
};

#define PWID_OK                 0
#define PWID_ERR_INVALID       (-1)
#define PWID_ERR_NOT_FOUND     (-2)
#define PWID_ERR_DENIED        (-3)
#define PWID_ERR_EXISTS        (-4)
#define PWID_ERR_DISABLED      (-5)
#define PWID_ERR_PASSWORD      (-6)
#define PWID_ERR_FULL          (-7)

void pwid_init(void);
uint64_t pwid_generate(const char *password, const char *note, uint8_t level);
int pwid_verify_password(uint64_t pwid, const char *password);
int pwid_create(const char *password, const char *note, uint8_t level);
int pwid_delete(uint64_t pwid);
int pwid_disable(uint64_t pwid);
int pwid_enable(uint64_t pwid);
int pwid_change_password(uint64_t pwid, const char *old_password, const char *new_password);

struct pwid_entry* pwid_find(uint64_t pwid);
struct pwid_entry* pwid_find_by_note(const char *note);
uint8_t pwid_get_level(uint64_t pwid);
int pwid_is_original_root(uint64_t pwid);
int pwid_is_root(uint64_t pwid);
int pwid_check_permission(uint64_t pwid, uint8_t required_level);
int pwid_has_default_password(uint64_t pwid);
void pwid_clear_default_password_flag(uint64_t pwid);

int pwid_create_derived_root(const char *password, const char *note);
int pwid_delete_derived_root(uint64_t pwid);
int pwid_create_original_root(const char *password);
int pwid_has_original_root(void);

void pwid_set_context(uint64_t pwid);
uint64_t pwid_get_current(void);
struct pwid_entry* pwid_get_current_entry(void);
int pwid_login(const char *note, const char *password);
void pwid_logout(void);

int pwid_create_user(const char *password, const char *note, uint8_t level);
int pwid_can_create_level(uint8_t creator_level, uint8_t target_level);
int pwid_can_modify(uint64_t modifier_pwid, uint64_t target_pwid);

void pwid_list_all(void);

extern struct pwid_entry pwid_table[MAX_PWID_ENTRIES];
extern int pwid_count;
extern int original_root_created;

#endif
