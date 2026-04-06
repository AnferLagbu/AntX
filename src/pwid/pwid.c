#include "pwid.h"
#include "serial.h"
#include "kernel.h"

struct pwid_entry pwid_table[MAX_PWID_ENTRIES];
int pwid_count = 0;
int original_root_created = 0;

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

static int str_len(const char *s) {
    int len = 0;
    while (s[len]) len++;
    return len;
}

static int str_cmp(const char *s1, const char *s2) {
    while (*s1 && *s2 && *s1 == *s2) {
        s1++; s2++;
    }
    return *s1 - *s2;
}

static void str_cpy(char *dest, const char *src) {
    while (*src) {
        *dest++ = *src++;
    }
    *dest = '\0';
}

static void mem_cpy(uint8_t *dest, const uint8_t *src, size_t n) {
    for (size_t i = 0; i < n; i++) {
        dest[i] = src[i];
    }
}

static int mem_cmp(const uint8_t *a, const uint8_t *b, size_t n) {
    for (size_t i = 0; i < n; i++) {
        if (a[i] != b[i]) return a[i] - b[i];
    }
    return 0;
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
    
    serial_puts(SERIAL_COM1, "PWID manager initialized\n");
}

uint64_t pwid_generate(const char *password, const char *note, uint8_t level) {
    uint8_t input[256];
    uint8_t hash[PWID_HASH_LEN];
    int pos = 0;
    
    int pwd_len = str_len(password);
    int note_len = str_len(note);
    
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
    sha256((const uint8_t *)password, str_len(password), hash);
    
    return mem_cmp(entry->password_hash, hash, PWID_HASH_LEN) == 0;
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
    
    str_cpy(entry->note, note);
    sha256((const uint8_t *)password, str_len(password), entry->password_hash);
    
    pwid_count++;
    
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
    
    sha256((const uint8_t *)new_password, str_len(new_password), entry->password_hash);
    entry->flags |= PWID_FLAG_MODIFIED;
    entry->flags &= ~PWID_FLAG_DEFAULT_PW;
    
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
    
    str_cpy(entry->note, new_note);
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
        if (pwid_table[i].pwid != 0 && str_cmp(pwid_table[i].note, note) == 0) {
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
    sha256((const uint8_t *)password, str_len(password), hash);
    
    if (mem_cmp(entry->password_hash, hash, PWID_HASH_LEN) != 0) {
        serial_puts(SERIAL_COM1, "PWID: incorrect password\n");
        return PWID_ERR_PASSWORD;
    }
    
    current_context.current = entry;
    current_context.session_pwid = entry->pwid;
    
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
