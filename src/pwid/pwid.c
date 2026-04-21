#include "pwid.h"
#include "serial.h"
#include "kernel.h"
#include "string.h"
#include "hvfs_rust.h"
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
    
    rust_pwid_init();
    
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
    
    rust_hvfs_set_current_pwid(entry->pwid);
    
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
    
    return rust_pwid_check_permission(
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
    
    return rust_pwid_create_elevation_token(
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
    return rust_pwid_add_trust(
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

#define PWID_DB_PATH "/etc/pwid.db"
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
