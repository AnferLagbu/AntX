#include "pwid.h"
#include "serial.h"
#include "kernel.h"
#include "string.h"
#include "hvfs_ffi.h"
#include "hvfs.h"

struct pwid_entry pwid_table[MAX_PWID_ENTRIES];
int pwid_count = 0;
int original_root_created = 0;
static int pwid_modified = 0;

static uint32_t k[64] = {
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
};

static uint32_t rotr(uint32_t x, int n) {
    return (x >> n) | (x << (32 - n));
}

static void sha256_transform(uint32_t *state, const uint8_t *block) {
    uint32_t w[64];
    uint32_t a, b, c, d, e, f, g, h;
    uint32_t t1, t2;
    
    for (int i = 0; i < 16; i++) {
        w[i] = ((uint32_t)block[i * 4] << 24) |
               ((uint32_t)block[i * 4 + 1] << 16) |
               ((uint32_t)block[i * 4 + 2] << 8) |
               ((uint32_t)block[i * 4 + 3]);
    }
    
    for (int i = 16; i < 64; i++) {
        uint32_t s0 = rotr(w[i-15], 7) ^ rotr(w[i-15], 18) ^ (w[i-15] >> 3);
        uint32_t s1 = rotr(w[i-2], 17) ^ rotr(w[i-2], 19) ^ (w[i-2] >> 10);
        w[i] = w[i-16] + s0 + w[i-7] + s1;
    }
    
    a = state[0]; b = state[1]; c = state[2]; d = state[3];
    e = state[4]; f = state[5]; g = state[6]; h = state[7];
    
    for (int i = 0; i < 64; i++) {
        uint32_t S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
        uint32_t ch = (e & f) ^ ((~e) & g);
        t1 = h + S1 + ch + k[i] + w[i];
        uint32_t S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
        uint32_t maj = (a & b) ^ (a & c) ^ (b & c);
        t2 = S0 + maj;
        
        h = g; g = f; f = e; e = d + t1;
        d = c; c = b; b = a; a = t1 + t2;
    }
    
    state[0] += a; state[1] += b; state[2] += c; state[3] += d;
    state[4] += e; state[5] += f; state[6] += g; state[7] += h;
}

static void sha256(const uint8_t *data, size_t len, uint8_t *hash) {
    uint32_t state[8] = {
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19
    };
    
    uint8_t block[64];
    size_t i = 0;
    
    while (i + 64 <= len) {
        sha256_transform(state, data + i);
        i += 64;
    }
    
    int remaining = len - i;
    for (int j = 0; j < remaining; j++) {
        block[j] = data[i + j];
    }
    
    block[remaining] = 0x80;
    for (int j = remaining + 1; j < 56; j++) {
        block[j] = 0;
    }
    
    uint64_t bit_len = len * 8;
    for (int j = 56; j < 64; j++) {
        block[j] = (bit_len >> ((63 - j) * 8)) & 0xFF;
    }
    
    sha256_transform(state, block);
    
    for (int j = 0; j < 8; j++) {
        hash[j * 4] = (state[j] >> 24) & 0xFF;
        hash[j * 4 + 1] = (state[j] >> 16) & 0xFF;
        hash[j * 4 + 2] = (state[j] >> 8) & 0xFF;
        hash[j * 4 + 3] = state[j] & 0xFF;
    }
}

void pwid_init(void) {
    for (int i = 0; i < MAX_PWID_ENTRIES; i++) {
        pwid_table[i].pwid = 0;
        pwid_table[i].level = 0;
        pwid_table[i].flags = 0;
        for (int j = 0; j < PWID_NOTE_LEN; j++) {
            pwid_table[i].note[j] = '\0';
        }
        for (int j = 0; j < PWID_HASH_LEN; j++) {
            pwid_table[i].password_hash[j] = 0;
        }
    }
    pwid_count = 0;
    original_root_created = 0;
    pwid_modified = 0;
    
    pwid_enhanced_init();
    
    serial_puts(SERIAL_COM1, "PWID manager initialized\n");
}

void pwid_try_load(void) {
    if (pwid_load_from_disk() == 0) {
        serial_puts(SERIAL_COM1, "PWID: Database loaded from disk\n");
    } else {
        serial_puts(SERIAL_COM1, "PWID: No database found, will create on first save\n");
    }
}

uint64_t pwid_generate(const char *password, const char *note, uint8_t level) {
    uint8_t input[256];
    uint8_t hash[PWID_HASH_LEN];
    int pos = 0;
    
    int pwd_len = strlen(password);
    int note_len = strlen(note);
    
    for (int i = 0; i < pwd_len && pos < 128; i++) {
        input[pos++] = password[i];
    }
    input[pos++] = ':';
    for (int i = 0; i < note_len && pos < 255; i++) {
        input[pos++] = note[i];
    }
    
    sha256(input, pos, hash);
    
    uint64_t pwid = 0;
    pwid |= ((uint64_t)level << 60);
    
    for (int i = 0; i < 7; i++) {
        pwid |= ((uint64_t)hash[i] << (i * 8));
    }
    pwid |= ((uint64_t)(hash[7] & 0x0F) << 56);
    
    return pwid;
}

int pwid_verify_password(uint64_t pwid, const char *password) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return 0;
    }
    
    uint8_t hash[PWID_HASH_LEN];
    sha256((const uint8_t *)password, strlen(password), hash);
    
    return memcmp(entry->password_hash, hash, PWID_HASH_LEN) == 0;
}

int pwid_create(const char *password, const char *note, uint8_t level) {
    if (pwid_count >= MAX_PWID_ENTRIES) {
        serial_puts(SERIAL_COM1, "PWID: table full\n");
        return -1;
    }
    
    if (level > PWID_LEVEL_UNTRUSTWORTHY) {
        serial_puts(SERIAL_COM1, "PWID: invalid level\n");
        return -1;
    }
    
    struct pwid_entry *entry = NULL;
    for (int i = 0; i < MAX_PWID_ENTRIES; i++) {
        if (pwid_table[i].pwid == 0) {
            entry = &pwid_table[i];
            break;
        }
    }
    
    if (entry == NULL) {
        return -1;
    }
    
    entry->pwid = pwid_generate(password, note, level);
    entry->level = level;
    entry->flags = 0;
    
    strcpy(entry->note, note);
    sha256((const uint8_t *)password, strlen(password), entry->password_hash);
    
    pwid_count++;
    pwid_set_modified();
    
    serial_puts(SERIAL_COM1, "PWID created: 0x");
    serial_put_hex(SERIAL_COM1, entry->pwid);
    serial_puts(SERIAL_COM1, " note=");
    serial_puts(SERIAL_COM1, note);
    serial_puts(SERIAL_COM1, "\n");
    
    return 0;
}

int pwid_delete(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return -1;
    }
    
    if (entry->flags & PWID_FLAG_ORIGINAL_ROOT) {
        serial_puts(SERIAL_COM1, "PWID: cannot delete original root\n");
        return -1;
    }
    
    entry->pwid = 0;
    entry->level = 0;
    entry->flags = 0;
    pwid_count--;
    pwid_set_modified();
    
    serial_puts(SERIAL_COM1, "PWID deleted\n");
    return 0;
}

int pwid_disable(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return -1;
    }
    
    if (entry->flags & PWID_FLAG_ORIGINAL_ROOT) {
        serial_puts(SERIAL_COM1, "PWID: cannot disable original root\n");
        return -1;
    }
    
    entry->flags |= PWID_FLAG_DISABLED;
    return 0;
}

int pwid_enable(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return -1;
    }
    
    entry->flags &= ~PWID_FLAG_DISABLED;
    return 0;
}

int pwid_change_password(uint64_t pwid, const char *old_password, const char *new_password) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return -1;
    }
    
    if (!pwid_verify_password(pwid, old_password)) {
        serial_puts(SERIAL_COM1, "PWID: old password incorrect\n");
        return -1;
    }
    
    sha256((const uint8_t *)new_password, strlen(new_password), entry->password_hash);
    entry->flags |= PWID_FLAG_MODIFIED;
    entry->flags &= ~PWID_FLAG_DEFAULT_PW;
    pwid_set_modified();
    
    serial_puts(SERIAL_COM1, "PWID: password changed\n");
    return 0;
}

int pwid_change_note(uint64_t pwid, const char *new_note) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return -1;
    }
    
    if (entry->flags & PWID_FLAG_ORIGINAL_ROOT) {
        serial_puts(SERIAL_COM1, "PWID: cannot change original root note\n");
        return -1;
    }
    
    strcpy(entry->note, new_note);
    return 0;
}

struct pwid_entry* pwid_find(uint64_t pwid) {
    for (int i = 0; i < MAX_PWID_ENTRIES; i++) {
        if (pwid_table[i].pwid == pwid) {
            return &pwid_table[i];
        }
    }
    return NULL;
}

struct pwid_entry* pwid_find_by_note(const char *note) {
    for (int i = 0; i < MAX_PWID_ENTRIES; i++) {
        if (pwid_table[i].pwid != 0 && strcmp(pwid_table[i].note, note) == 0) {
            return &pwid_table[i];
        }
    }
    return NULL;
}

uint8_t pwid_get_level(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return 0xFF;
    }
    return entry->level;
}

int pwid_is_original_root(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return 0;
    }
    return (entry->flags & PWID_FLAG_ORIGINAL_ROOT) != 0;
}

int pwid_is_root(uint64_t pwid) {
    return pwid_get_level(pwid) == PWID_LEVEL_ROOT;
}

int pwid_has_default_password(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return 0;
    }
    return (entry->flags & PWID_FLAG_DEFAULT_PW) != 0;
}

void pwid_clear_default_password_flag(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry != NULL) {
        entry->flags &= ~PWID_FLAG_DEFAULT_PW;
    }
}

int pwid_check_permission(uint64_t pwid, uint8_t required_level) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return 0;
    }
    
    if (entry->flags & PWID_FLAG_DISABLED) {
        return 0;
    }
    
    return entry->level <= required_level;
}

int pwid_create_derived_root(const char *password, const char *note) {
    if (pwid_count >= MAX_PWID_ENTRIES) {
        return -1;
    }
    
    int result = pwid_create(password, note, PWID_LEVEL_ROOT);
    if (result == 0) {
        struct pwid_entry *entry = pwid_find_by_note(note);
        if (entry) {
            entry->flags |= PWID_FLAG_MODIFIED;
        }
    }
    return result;
}

int pwid_delete_derived_root(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return -1;
    }
    
    if (entry->flags & PWID_FLAG_ORIGINAL_ROOT) {
        serial_puts(SERIAL_COM1, "PWID: cannot delete original root\n");
        return -1;
    }
    
    if (entry->level != PWID_LEVEL_ROOT) {
        serial_puts(SERIAL_COM1, "PWID: not a derived root\n");
        return -1;
    }
    
    return pwid_delete(pwid);
}

int pwid_create_original_root(const char *password) {
    if (original_root_created) {
        serial_puts(SERIAL_COM1, "PWID: original root already exists\n");
        return -1;
    }
    
    int result = pwid_create(password, "root", PWID_LEVEL_ROOT);
    if (result == 0) {
        struct pwid_entry *entry = pwid_find_by_note("root");
        if (entry) {
            entry->flags |= PWID_FLAG_ORIGINAL_ROOT | PWID_FLAG_DEFAULT_PW;
            original_root_created = 1;
            pwid_set_modified();
            serial_puts(SERIAL_COM1, "PWID: original root created\n");
        }
    }
    return result;
}

int pwid_has_original_root(void) {
    return original_root_created;
}

void pwid_list_all(void) {
    serial_puts(SERIAL_COM1, "\n=== PWID List ===\n");
    for (int i = 0; i < MAX_PWID_ENTRIES; i++) {
        if (pwid_table[i].pwid != 0) {
            serial_puts(SERIAL_COM1, "  PWID: 0x");
            serial_put_hex(SERIAL_COM1, pwid_table[i].pwid);
            serial_puts(SERIAL_COM1, " Level: ");
            serial_put_dec(SERIAL_COM1, pwid_table[i].level);
            serial_puts(SERIAL_COM1, " Note: ");
            serial_puts(SERIAL_COM1, pwid_table[i].note);
            if (pwid_table[i].flags & PWID_FLAG_ORIGINAL_ROOT) {
                serial_puts(SERIAL_COM1, " [ORIG]");
            }
            if (pwid_table[i].flags & PWID_FLAG_DISABLED) {
                serial_puts(SERIAL_COM1, " [DISABLED]");
            }
            serial_puts(SERIAL_COM1, "\n");
        }
    }
    serial_puts(SERIAL_COM1, "=================\n");
}

static struct pwid_context current_context = {NULL, 0};

void pwid_set_context(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry != NULL && !(entry->flags & PWID_FLAG_DISABLED)) {
        current_context.current = entry;
        current_context.session_pwid = pwid;
    } else {
        current_context.current = NULL;
        current_context.session_pwid = 0;
    }
}

uint64_t pwid_get_current(void) {
    if (current_context.session_pwid == 0) {
        return 0x0020F45A8B978417;
    }
    return current_context.session_pwid;
}

struct pwid_entry* pwid_get_current_entry(void) {
    return current_context.current;
}

int pwid_login(const char *note, const char *password) {
    struct pwid_entry *entry = pwid_find_by_note(note);
    if (entry == NULL) {
        serial_puts(SERIAL_COM1, "PWID: user not found\n");
        return PWID_ERR_NOT_FOUND;
    }
    
    if (entry->flags & PWID_FLAG_DISABLED) {
        serial_puts(SERIAL_COM1, "PWID: account disabled\n");
        return PWID_ERR_DISABLED;
    }
    
    uint8_t hash[PWID_HASH_LEN];
    sha256((const uint8_t *)password, strlen(password), hash);
    
    if (memcmp(entry->password_hash, hash, PWID_HASH_LEN) != 0) {
        serial_puts(SERIAL_COM1, "PWID: incorrect password\n");
        return PWID_ERR_PASSWORD;
    }
    
    current_context.current = entry;
    current_context.session_pwid = entry->pwid;
    
    hvfs_set_current_pwid_internal(entry->pwid);
    
    serial_puts(SERIAL_COM1, "PWID: logged in as '");
    serial_puts(SERIAL_COM1, note);
    serial_puts(SERIAL_COM1, "'\n");
    
    return PWID_OK;
}

void pwid_logout(void) {
    if (current_context.current != NULL) {
        serial_puts(SERIAL_COM1, "PWID: logged out from '");
        serial_puts(SERIAL_COM1, current_context.current->note);
        serial_puts(SERIAL_COM1, "'\n");
    }
    current_context.current = NULL;
    current_context.session_pwid = 0;
}

int pwid_can_create_level(uint8_t creator_level, uint8_t target_level) {
    if (creator_level == PWID_LEVEL_ROOT) {
        return 1;
    }
    if (creator_level == PWID_LEVEL_TRUSTWORTHY) {
        return target_level == PWID_LEVEL_UNTRUSTWORTHY;
    }
    return 0;
}

int pwid_can_modify(uint64_t modifier_pwid, uint64_t target_pwid) {
    struct pwid_entry *modifier = pwid_find(modifier_pwid);
    struct pwid_entry *target = pwid_find(target_pwid);
    
    if (modifier == NULL || target == NULL) {
        return 0;
    }
    
    if (modifier->flags & PWID_FLAG_DISABLED) {
        return 0;
    }
    
    if (target->flags & PWID_FLAG_ORIGINAL_ROOT) {
        return 0;
    }
    
    if (modifier->level == PWID_LEVEL_ROOT) {
        return 1;
    }
    
    if (modifier->level < target->level) {
        return 1;
    }
    
    return 0;
}

int pwid_create_user(const char *password, const char *note, uint8_t level) {
    if (current_context.current == NULL) {
        serial_puts(SERIAL_COM1, "PWID: no active session\n");
        return PWID_ERR_DENIED;
    }
    
    if (current_context.current->flags & PWID_FLAG_DISABLED) {
        serial_puts(SERIAL_COM1, "PWID: current account disabled\n");
        return PWID_ERR_DISABLED;
    }
    
    if (!pwid_can_create_level(current_context.current->level, level)) {
        serial_puts(SERIAL_COM1, "PWID: permission denied - cannot create level ");
        serial_put_dec(SERIAL_COM1, level);
        serial_puts(SERIAL_COM1, "\n");
        return PWID_ERR_DENIED;
    }
    
    if (pwid_find_by_note(note) != NULL) {
        serial_puts(SERIAL_COM1, "PWID: note already exists\n");
        return PWID_ERR_EXISTS;
    }
    
    if (pwid_count >= MAX_PWID_ENTRIES) {
        serial_puts(SERIAL_COM1, "PWID: table full\n");
        return PWID_ERR_FULL;
    }
    
    if (level > PWID_LEVEL_UNTRUSTWORTHY) {
        serial_puts(SERIAL_COM1, "PWID: invalid level\n");
        return PWID_ERR_INVALID;
    }
    
    int result = pwid_create(password, note, level);
    if (result == 0) {
        serial_puts(SERIAL_COM1, "PWID: user '");
        serial_puts(SERIAL_COM1, note);
        serial_puts(SERIAL_COM1, "' created by '");
        serial_puts(SERIAL_COM1, current_context.current->note);
        serial_puts(SERIAL_COM1, "'\n");
    }
    
    return result;
}

int pwid_enhanced_check(uint64_t pwid, uint64_t owner_pwid, 
                        uint64_t access_type, uint16_t domain) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return 0;
    }
    
    return pwid_check_permission_enhanced(
        pwid,
        owner_pwid,
        entry->level,
        entry->flags,
        access_type,
        domain,
        0
    );
}

int64_t pwid_create_token(uint64_t holder, uint16_t domain, uint64_t caps,
                          uint64_t duration_secs, uint32_t max_uses) {
    if (current_context.current == NULL) {
        return -1;
    }
    
    uint64_t issuer = current_context.session_pwid;
    uint16_t domains[1] = { domain };
    uint64_t capabilities[1] = { caps };
    
    return pwid_create_elevation_token_internal(
        issuer,
        holder,
        domains,
        capabilities,
        1,
        duration_secs,
        max_uses
    );
}

int pwid_add_trust_relation(uint64_t truster, uint64_t trusted,
                            uint8_t trust_level, uint16_t domain, 
                            uint64_t cap_mask) {
    return pwid_add_trust_internal(
        truster,
        trusted,
        trust_level,
        domain,
        cap_mask,
        0
    );
}

void pwid_set_modified(void) {
    pwid_modified = 1;
}

int pwid_is_modified(void) {
    return pwid_modified;
}

#define PWID_DB_PATH "/cfg/system/pwid.db"
#define PWID_DB_MAGIC 0x50574944
#define PWID_DB_VERSION 1

struct pwid_db_header {
    uint32_t magic;
    uint32_t version;
    uint32_t count;
    uint32_t original_root_created;
    uint8_t reserved[48];
} __attribute__((packed));

struct pwid_db_entry {
    uint64_t pwid;
    uint8_t level;
    uint8_t flags;
    char note[PWID_NOTE_LEN];
    uint8_t password_hash[PWID_HASH_LEN];
    uint64_t created_time;
    uint64_t expires_at;
    uint8_t reserved[8];
} __attribute__((packed));

int pwid_save_to_disk(void) {
    int fd = hvfs_open(PWID_DB_PATH, HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, 0);
    if (fd < 0) {
        serial_puts(SERIAL_COM1, "PWID: Failed to open database file for writing\n");
        return -1;
    }
    
    struct pwid_db_header header;
    memset(&header, 0, sizeof(header));
    header.magic = PWID_DB_MAGIC;
    header.version = PWID_DB_VERSION;
    header.count = pwid_count;
    header.original_root_created = original_root_created;
    
    if (hvfs_write(fd, &header, sizeof(header)) != sizeof(header)) {
        serial_puts(SERIAL_COM1, "PWID: Failed to write database header\n");
        hvfs_close(fd);
        return -1;
    }
    
    int saved_count = 0;
    for (int i = 0; i < MAX_PWID_ENTRIES; i++) {
        if (pwid_table[i].pwid != 0) {
            struct pwid_db_entry entry;
            memset(&entry, 0, sizeof(entry));
            entry.pwid = pwid_table[i].pwid;
            entry.level = pwid_table[i].level;
            entry.flags = pwid_table[i].flags;
            strcpy(entry.note, pwid_table[i].note);
            memcpy(entry.password_hash, pwid_table[i].password_hash, PWID_HASH_LEN);
            entry.created_time = pwid_table[i].created_time;
            entry.expires_at = pwid_table[i].expires_at;
            
            if (hvfs_write(fd, &entry, sizeof(entry)) != sizeof(entry)) {
                serial_puts(SERIAL_COM1, "PWID: Failed to write database entry\n");
                hvfs_close(fd);
                return -1;
            }
            saved_count++;
        }
    }
    
    hvfs_close(fd);
    
    pwid_modified = 0;
    
    serial_puts(SERIAL_COM1, "PWID: Saved ");
    serial_put_dec(SERIAL_COM1, saved_count);
    serial_puts(SERIAL_COM1, " entries to disk\n");
    
    return 0;
}

int pwid_load_from_disk(void) {
    int fd = hvfs_open(PWID_DB_PATH, HVFS_O_RDONLY, 0);
    if (fd < 0) {
        serial_puts(SERIAL_COM1, "PWID: No database file found, starting fresh\n");
        return -1;
    }
    
    struct pwid_db_header header;
    int bytes_read = hvfs_read(fd, &header, sizeof(header));
    if (bytes_read != sizeof(header)) {
        serial_puts(SERIAL_COM1, "PWID: Failed to read database header\n");
        hvfs_close(fd);
        return -1;
    }
    
    if (header.magic != PWID_DB_MAGIC) {
        serial_puts(SERIAL_COM1, "PWID: Invalid database magic\n");
        hvfs_close(fd);
        return -1;
    }
    
    if (header.version > PWID_DB_VERSION) {
        serial_puts(SERIAL_COM1, "PWID: Database version too new\n");
        hvfs_close(fd);
        return -1;
    }
    
    pwid_count = 0;
    original_root_created = header.original_root_created;
    
    for (uint32_t i = 0; i < header.count; i++) {
        struct pwid_db_entry entry;
        bytes_read = hvfs_read(fd, &entry, sizeof(entry));
        if (bytes_read != sizeof(entry)) {
            serial_puts(SERIAL_COM1, "PWID: Failed to read database entry\n");
            break;
        }
        
        int slot = -1;
        for (int j = 0; j < MAX_PWID_ENTRIES; j++) {
            if (pwid_table[j].pwid == 0) {
                slot = j;
                break;
            }
        }
        
        if (slot >= 0) {
            pwid_table[slot].pwid = entry.pwid;
            pwid_table[slot].level = entry.level;
            pwid_table[slot].flags = entry.flags;
            strcpy(pwid_table[slot].note, entry.note);
            memcpy(pwid_table[slot].password_hash, entry.password_hash, PWID_HASH_LEN);
            pwid_table[slot].created_time = entry.created_time;
            pwid_table[slot].expires_at = entry.expires_at;
            pwid_count++;
        }
    }
    
    hvfs_close(fd);
    
    pwid_modified = 0;
    
    serial_puts(SERIAL_COM1, "PWID: Loaded ");
    serial_put_dec(SERIAL_COM1, pwid_count);
    serial_puts(SERIAL_COM1, " entries from disk\n");
    
    return 0;
}

static uint64_t get_current_time(void) {
    uint64_t tsc;
    __asm__ volatile("rdtsc" : "=A"(tsc));
    return tsc / 3000000000ULL;
}

int pwid_is_expired(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return 1;
    }
    
    if (entry->flags & PWID_FLAG_ORIGINAL_ROOT) {
        return 0;
    }
    
    if (entry->expires_at == 0) {
        return 0;
    }
    
    uint64_t now = get_current_time();
    return now >= entry->expires_at;
}

int pwid_is_locked(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return 1;
    }
    
    if (entry->flags & PWID_FLAG_LOCKED) {
        uint64_t now = get_current_time();
        if (entry->lockout_until > 0 && now < entry->lockout_until) {
            return 1;
        }
        entry->flags &= ~PWID_FLAG_LOCKED;
        entry->lockout_until = 0;
    }
    
    return 0;
}

int pwid_check_expiry(uint64_t pwid) {
    if (pwid_is_expired(pwid)) {
        struct pwid_entry *entry = pwid_find(pwid);
        if (entry != NULL) {
            entry->flags |= PWID_FLAG_EXPIRED;
        }
        return 1;
    }
    return 0;
}

void pwid_set_expiry(uint64_t pwid, uint64_t expires_at) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return;
    }
    
    if (entry->flags & PWID_FLAG_ORIGINAL_ROOT) {
        return;
    }
    
    entry->expires_at = expires_at;
    entry->flags &= ~PWID_FLAG_EXPIRED;
    pwid_set_modified();
}

void pwid_extend_expiry(uint64_t pwid, uint64_t days) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return;
    }
    
    if (entry->flags & PWID_FLAG_ORIGINAL_ROOT) {
        return;
    }
    
    uint64_t now = get_current_time();
    uint64_t extension = days * 86400;
    
    if (entry->expires_at > now) {
        entry->expires_at += extension;
    } else {
        entry->expires_at = now + extension;
    }
    
    entry->flags &= ~PWID_FLAG_EXPIRED;
    pwid_set_modified();
}

void pwid_clear_lockout(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry != NULL) {
        entry->flags &= ~PWID_FLAG_LOCKED;
        entry->lockout_until = 0;
        entry->failed_attempts = 0;
    }
}

void pwid_record_failed_login(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry == NULL) {
        return;
    }
    
    entry->failed_attempts++;
    
    if (entry->failed_attempts >= PWID_MAX_LOGIN_ATTEMPTS) {
        entry->flags |= PWID_FLAG_LOCKED;
        entry->lockout_until = get_current_time() + PWID_LOCKOUT_DURATION;
        serial_puts(SERIAL_COM1, "PWID: Account locked due to too many failed attempts\n");
        pwid_audit_log(pwid, AUDIT_ACTION_LOGIN, AUDIT_RESULT_DENIED, 0, entry->failed_attempts);
    }
}

void pwid_clear_failed_attempts(uint64_t pwid) {
    struct pwid_entry *entry = pwid_find(pwid);
    if (entry != NULL) {
        entry->failed_attempts = 0;
    }
}

int pwid_login_with_bruteforce_protection(const char *note, const char *password) {
    struct pwid_entry *entry = pwid_find_by_note(note);
    if (entry == NULL) {
        serial_puts(SERIAL_COM1, "PWID: user not found\n");
        return PWID_ERR_NOT_FOUND;
    }
    
    if (pwid_is_locked(entry->pwid)) {
        serial_puts(SERIAL_COM1, "PWID: account locked\n");
        return PWID_ERR_DISABLED;
    }
    
    if (pwid_is_expired(entry->pwid)) {
        serial_puts(SERIAL_COM1, "PWID: account expired\n");
        return PWID_ERR_DISABLED;
    }
    
    if (entry->flags & PWID_FLAG_DISABLED) {
        serial_puts(SERIAL_COM1, "PWID: account disabled\n");
        return PWID_ERR_DISABLED;
    }
    
    uint8_t hash[PWID_HASH_LEN];
    sha256((const uint8_t *)password, strlen(password), hash);
    
    if (memcmp(entry->password_hash, hash, PWID_HASH_LEN) != 0) {
        pwid_record_failed_login(entry->pwid);
        serial_puts(SERIAL_COM1, "PWID: incorrect password\n");
        pwid_audit_log(entry->pwid, AUDIT_ACTION_LOGIN, AUDIT_RESULT_FAILURE, 0, 0);
        return PWID_ERR_PASSWORD;
    }
    
    pwid_clear_failed_attempts(entry->pwid);
    entry->last_login_time = get_current_time();
    
    current_context.current = entry;
    current_context.session_pwid = entry->pwid;
    
    hvfs_set_current_pwid_internal(entry->pwid);
    
    pwid_audit_log(entry->pwid, AUDIT_ACTION_LOGIN, AUDIT_RESULT_SUCCESS, 0, 0);
    
    serial_puts(SERIAL_COM1, "PWID: logged in as '");
    serial_puts(SERIAL_COM1, note);
    serial_puts(SERIAL_COM1, "'\n");
    
    return PWID_OK;
}

#define MAX_ELEVATION_DEPTH 8

static struct {
    struct pwid_context stack[MAX_ELEVATION_DEPTH];
    int depth;
} elevation_state = {{0}, 0};

static uint64_t elevation_token_id = 0;

int pwid_elevate(uint64_t target_pwid, const char *password, uint64_t duration_secs) {
    if (current_context.current == NULL) {
        return PWID_ERR_DENIED;
    }
    
    if (elevation_state.depth >= MAX_ELEVATION_DEPTH) {
        pwid_audit_log(current_context.session_pwid, AUDIT_ACTION_ELEVATE,
                       AUDIT_RESULT_FAILURE, target_pwid, 0);
        return PWID_ERR_DENIED;
    }
    
    struct pwid_entry *target = pwid_find(target_pwid);
    if (target == NULL) {
        return PWID_ERR_NOT_FOUND;
    }
    
    if (target->level != PWID_LEVEL_ROOT) {
        return PWID_ERR_DENIED;
    }
    
    if (!pwid_verify_password(target_pwid, password)) {
        pwid_audit_log(current_context.session_pwid, AUDIT_ACTION_ELEVATE, 
                       AUDIT_RESULT_FAILURE, target_pwid, 0);
        return PWID_ERR_PASSWORD;
    }

    elevation_state.stack[elevation_state.depth].current = current_context.current;
    elevation_state.stack[elevation_state.depth].session_pwid = current_context.session_pwid;
    elevation_state.depth++;
    
    uint16_t domains[] = {CAP_DOMAIN_SYSTEM, CAP_DOMAIN_FS, CAP_DOMAIN_PROC};
    uint64_t caps[] = {0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFF};
    
    int64_t token = pwid_create_elevation_token_internal(
        target_pwid,
        current_context.session_pwid,
        domains,
        caps,
        3,
        duration_secs,
        1
    );
    
    if (token < 0) {
        elevation_state.depth--;
        return PWID_ERR_DENIED;
    }
    
    elevation_token_id = (uint64_t)token;
    
    current_context.current = target;
    current_context.session_pwid = target_pwid;
    hvfs_set_current_pwid_internal(target_pwid);
    
    pwid_audit_log(elevation_state.stack[elevation_state.depth - 1].session_pwid, AUDIT_ACTION_ELEVATE, 
                   AUDIT_RESULT_SUCCESS, target_pwid, token);
    
    serial_puts(SERIAL_COM1, "PWID: Elevated to root for ");
    serial_put_dec(SERIAL_COM1, duration_secs);
    serial_puts(SERIAL_COM1, " seconds\n");
    
    return PWID_OK;
}

int pwid_elevate_with_token(uint64_t token_id) {
    if (pwid_use_token_internal(token_id) != 0) {
        return PWID_ERR_DENIED;
    }
    
    pwid_audit_log(current_context.session_pwid, AUDIT_ACTION_TOKEN_USE,
                   AUDIT_RESULT_SUCCESS, 0, token_id);
    
    return PWID_OK;
}

void pwid_end_elevation(void) {
    if (elevation_state.depth > 0) {
        elevation_state.depth--;
        
        if (elevation_token_id != 0) {
            pwid_revoke_token_internal(elevation_token_id, current_context.session_pwid);
            elevation_token_id = 0;
        }
        
        struct pwid_context prev = elevation_state.stack[elevation_state.depth];
        
        current_context.current = prev.current;
        current_context.session_pwid = prev.session_pwid;
        hvfs_set_current_pwid_internal(prev.session_pwid);
        
        pwid_audit_log(prev.session_pwid, AUDIT_ACTION_LOGOUT,
                       AUDIT_RESULT_SUCCESS, 0, 0);
        
        serial_puts(SERIAL_COM1, "PWID: Elevation ended (depth=");
        serial_put_dec(SERIAL_COM1, elevation_state.depth);
        serial_puts(SERIAL_COM1, ")\n");
    }
}

int pwid_is_elevated(void) {
    return elevation_state.depth > 0;
}

#define MAX_AUDIT_ENTRIES 256
static struct pwid_audit_entry audit_log_entries[MAX_AUDIT_ENTRIES];
static int audit_log_count = 0;

void pwid_audit_log(uint64_t pwid, uint32_t action, uint32_t result,
                    uint64_t target_pwid, uint64_t details) {
    if (audit_log_count >= MAX_AUDIT_ENTRIES) {
        for (int i = 0; i < MAX_AUDIT_ENTRIES - 1; i++) {
            audit_log_entries[i] = audit_log_entries[i + 1];
        }
        audit_log_count = MAX_AUDIT_ENTRIES - 1;
    }
    
    struct pwid_audit_entry *entry = &audit_log_entries[audit_log_count++];
    entry->timestamp = get_current_time();
    entry->pwid = pwid;
    entry->action = action;
    entry->result = result;
    entry->target_pwid = target_pwid;
    entry->details = details;
}

void pwid_audit_dump(void) {
    serial_puts(SERIAL_COM1, "\n=== PWID Audit Log ===\n");
    for (int i = 0; i < audit_log_count; i++) {
        struct pwid_audit_entry *e = &audit_log_entries[i];
        serial_puts(SERIAL_COM1, "  [");
        serial_put_dec(SERIAL_COM1, e->timestamp);
        serial_puts(SERIAL_COM1, "] PWID:0x");
        serial_put_hex(SERIAL_COM1, e->pwid);
        serial_puts(SERIAL_COM1, " Action:");
        serial_put_dec(SERIAL_COM1, e->action);
        serial_puts(SERIAL_COM1, " Result:");
        serial_put_dec(SERIAL_COM1, e->result);
        serial_puts(SERIAL_COM1, "\n");
    }
    serial_puts(SERIAL_COM1, "=====================\n");
}

#define AUDIT_DB_PATH "/cfg/system/pwid_audit.db"
#define AUDIT_DB_MAGIC 0x41554449

int pwid_audit_save_to_disk(void) {
    int fd = hvfs_open(AUDIT_DB_PATH, HVFS_O_CREAT | HVFS_O_WRONLY | HVFS_O_TRUNC, 0);
    if (fd < 0) {
        return -1;
    }
    
    uint32_t header[2] = {AUDIT_DB_MAGIC, (uint32_t)audit_log_count};
    hvfs_write(fd, header, sizeof(header));
    hvfs_write(fd, audit_log_entries, sizeof(struct pwid_audit_entry) * audit_log_count);
    hvfs_close(fd);
    
    return 0;
}

int pwid_audit_load_from_disk(void) {
    int fd = hvfs_open(AUDIT_DB_PATH, HVFS_O_RDONLY, 0);
    if (fd < 0) {
        return -1;
    }
    
    uint32_t header[2];
    if (hvfs_read(fd, header, sizeof(header)) != sizeof(header)) {
        hvfs_close(fd);
        return -1;
    }
    
    if (header[0] != AUDIT_DB_MAGIC) {
        hvfs_close(fd);
        return -1;
    }
    
    int count = header[1];
    if (count > MAX_AUDIT_ENTRIES) {
        count = MAX_AUDIT_ENTRIES;
    }
    
    hvfs_read(fd, audit_log_entries, sizeof(struct pwid_audit_entry) * count);
    audit_log_count = count;
    hvfs_close(fd);
    
    return 0;
}

void pwid_periodic_cleanup(void) {
    pwid_cleanup_internal();
    
    for (int i = 0; i < MAX_PWID_ENTRIES; i++) {
        if (pwid_table[i].pwid != 0) {
            pwid_check_expiry(pwid_table[i].pwid);
            
            if (pwid_table[i].flags & PWID_FLAG_LOCKED) {
                if (pwid_table[i].lockout_until > 0 && 
                    get_current_time() >= pwid_table[i].lockout_until) {
                    pwid_table[i].flags &= ~PWID_FLAG_LOCKED;
                    pwid_table[i].lockout_until = 0;
                    pwid_table[i].failed_attempts = 0;
                }
            }
        }
    }
}
