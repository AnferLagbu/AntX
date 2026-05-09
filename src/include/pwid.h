#ifndef _PWID_H
#define _PWID_H

#include "types.h"

#define MAX_PWID_ENTRIES    64
#define PWID_NOTE_LEN       32
#define PWID_HASH_LEN       48   /* 32 bytes digest + 16 bytes salt */
#define PWID_SALT_LEN       16
#define PWID_DIGEST_LEN     32   /* SHA-256 output */
#define PWID_PASSWORD_MAX   64

#define PWID_LEVEL_ROOT         0
#define PWID_LEVEL_TRUSTWORTHY  1
#define PWID_LEVEL_UNTRUSTWORTHY 2

#define PWID_FLAG_TEMPORARY     0x02
#define PWID_FLAG_DISABLED      0x04
#define PWID_FLAG_MODIFIED      0x08
#define PWID_FLAG_DEFAULT_PW    0x10
#define PWID_FLAG_LOCKED        0x20
#define PWID_FLAG_EXPIRED       0x40

#define PWID_MAX_LOGIN_ATTEMPTS  5
#define PWID_LOCKOUT_DURATION    300
#define PWID_DEFAULT_EXPIRY_DAYS 90

#define PWID_LEVEL_MASK     0xF000000000000000ULL
#define PWID_ORIGINAL_MASK  0x0800000000000000ULL
#define PWID_TEMP_MASK      0x0400000000000000ULL
#define PWID_MODIFIED_MASK  0x0200000000000000ULL
#define PWID_HASH_MASK      0x00FFFFFFFFFFFFFFULL

#define CAP_DOMAIN_SYSTEM    0x0000
#define CAP_DOMAIN_FS        0x0001
#define CAP_DOMAIN_NET       0x0002
#define CAP_DOMAIN_PROC      0x0003
#define CAP_DOMAIN_DEVICE    0x0004
#define CAP_DOMAIN_USER_MGMT 0x0005

#define FS_CAP_READ    (1ULL << 0)
#define FS_CAP_WRITE   (1ULL << 1)
#define FS_CAP_EXECUTE (1ULL << 2)
#define FS_CAP_CREATE  (1ULL << 3)
#define FS_CAP_DELETE  (1ULL << 4)
#define FS_CAP_CHMOD   (1ULL << 5)
#define FS_CAP_CHOWN   (1ULL << 6)
#define FS_CAP_MOUNT   (1ULL << 7)

#define PROC_CAP_FORK  (1ULL << 0)
#define PROC_CAP_EXEC  (1ULL << 1)
#define PROC_CAP_KILL  (1ULL << 2)
#define PROC_CAP_DEBUG (1ULL << 3)
#define PROC_CAP_RT_SCHED (1ULL << 4)  // v4: set SCHED_FIFO/SCHED_RR

#define TRUST_LEVEL_NONE      0
#define TRUST_LEVEL_BASIC     1
#define TRUST_LEVEL_OPERATE   2
#define TRUST_LEVEL_DELEGATE  3
#define TRUST_LEVEL_FULL      4

#define TOKEN_TYPE_ELEVATION  0
#define TOKEN_TYPE_DELEGATION 1
#define TOKEN_TYPE_SESSION    2
#define TOKEN_TYPE_ONETIME    3

#define TOKEN_FLAG_SINGLE_COMMAND 0x01
#define TOKEN_FLAG_NO_TTY         0x02
#define TOKEN_FLAG_REQUIRE_CONFIRM 0x04
#define TOKEN_FLAG_AUDIT_ALL      0x08

/* v4: Capability domains */
#define CAP_DOMAIN_SYSTEM_CFG  (1ULL << 0)
#define CAP_DOMAIN_DEVICE_DISK (1ULL << 1)
#define CAP_DOMAIN_USER_CREATE (1ULL << 2)
#define CAP_DOMAIN_USER_DELETE (1ULL << 3)
#define CAP_DOMAIN_USER_LIST   (1ULL << 4)
#define CAP_DOMAIN_TOKEN_ISSUE (1ULL << 5)
#define CAP_DOMAIN_TRUST_ADD   (1ULL << 6)
#define CAP_DOMAIN_SYS_ADMIN   0xFFFFFFFFFFFFFFFFULL

/* v4: FFI declarations */
uint64_t pwid_get_capability_raw(uint64_t pwid, uint16_t domain);
int pwid_has_capability(uint64_t pwid, uint16_t domain, uint64_t required);

/* v4: Helper — check if pwid has a specific capability in a domain */
static inline int pwid_has_cap_raw(uint64_t pwid, uint16_t domain, uint64_t cap) {
    uint64_t caps = pwid_get_capability_raw(pwid, domain);
    return (caps & cap) == cap ? 1 : 0;
}

struct pwid_entry {
    uint64_t pwid;
    uint8_t level;
    char note[PWID_NOTE_LEN];
    uint8_t password_hash[PWID_HASH_LEN];
    uint8_t flags;
    uint64_t created_time;
    uint64_t expires_at;
    uint8_t failed_attempts;
    uint64_t lockout_until;
    uint64_t last_login_time;
};

struct pwid_audit_entry {
    uint64_t timestamp;
    uint64_t pwid;
    uint32_t action;
    uint32_t result;
    uint64_t target_pwid;
    uint64_t details;
};

#define AUDIT_ACTION_LOGIN         1
#define AUDIT_ACTION_LOGOUT        2
#define AUDIT_ACTION_CREATE        3
#define AUDIT_ACTION_DELETE        4
#define AUDIT_ACTION_MODIFY        5
#define AUDIT_ACTION_PERMISSION    6
#define AUDIT_ACTION_TOKEN_CREATE  7
#define AUDIT_ACTION_TOKEN_USE     8
#define AUDIT_ACTION_ELEVATE       9

#define AUDIT_RESULT_SUCCESS 0
#define AUDIT_RESULT_FAILURE 1
#define AUDIT_RESULT_DENIED  2

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
uint64_t pwid_get_fs_capability(uint64_t pwid);
int pwid_is_root(uint64_t pwid);
int pwid_check_permission(uint64_t pwid, uint8_t required_level);
int pwid_has_default_password(uint64_t pwid);
void pwid_clear_default_password_flag(uint64_t pwid);

int pwid_create_derived_root(const char *password, const char *note);
int pwid_create_user_with_caps(const char *password, const char *note, uint8_t level, const uint64_t *caps_array);
int pwid_delete_derived_root(uint64_t pwid);
int pwid_create_first_identity(const char *password);
int pwid_any_identity_exists(void);

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

void pwid_enhanced_init(void);
int pwid_check_permission_enhanced(uint64_t pwid, uint64_t owner_pwid, 
                                uint8_t pwid_level, uint8_t pwid_flags,
                                uint64_t access_type, uint16_t domain,
                                uint16_t other_perms);
int64_t pwid_create_elevation_token_internal(uint64_t issuer, uint64_t holder,
                                         const uint16_t *domains, const uint64_t *caps,
                                         uint32_t count, uint64_t duration_secs,
                                         uint32_t max_uses);
int pwid_use_token_internal(uint64_t token_id);
int pwid_revoke_token_internal(uint64_t token_id, uint64_t revoker);
int pwid_add_trust_internal(uint64_t truster, uint64_t trusted, 
                        uint8_t trust_level, uint16_t domain,
                        uint64_t cap_mask, uint64_t expires_at);
int pwid_remove_trust_internal(uint64_t truster, uint64_t trusted, uint16_t domain);
void pwid_cleanup_internal(void);

int pwid_enhanced_check(uint64_t pwid, uint64_t owner_pwid, 
                        uint64_t access_type, uint16_t domain);
int64_t pwid_create_token(uint64_t holder, uint16_t domain, uint64_t caps,
                          uint64_t duration_secs, uint32_t max_uses);
int pwid_add_trust_relation(uint64_t truster, uint64_t trusted,
                            uint8_t trust_level, uint16_t domain, 
                            uint64_t cap_mask);

int pwid_save_to_disk(void);
int pwid_load_from_disk(void);
void pwid_set_modified(void);
int pwid_is_modified(void);
void pwid_try_load(void);
int pwid_try_genesis(const char *password);

int pwid_is_expired(uint64_t pwid);
int pwid_is_locked(uint64_t pwid);
int pwid_check_expiry(uint64_t pwid);
void pwid_set_expiry(uint64_t pwid, uint64_t expires_at);
void pwid_extend_expiry(uint64_t pwid, uint64_t days);
void pwid_clear_lockout(uint64_t pwid);

int pwid_login_with_bruteforce_protection(const char *note, const char *password);
void pwid_record_failed_login(uint64_t pwid);
void pwid_clear_failed_attempts(uint64_t pwid);

int pwid_elevate(uint64_t target_pwid, const char *password, uint64_t duration_secs);
int pwid_elevate_with_token(uint64_t token_id);
void pwid_end_elevation(void);
int pwid_is_elevated(void);

void pwid_audit_log(uint64_t pwid, uint32_t action, uint32_t result, 
                    uint64_t target_pwid, uint64_t details);
void pwid_audit_dump(void);
int pwid_audit_save_to_disk(void);
int pwid_audit_load_from_disk(void);

void pwid_periodic_cleanup(void);

#endif
