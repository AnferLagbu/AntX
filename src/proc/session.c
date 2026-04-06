#include "proc.h"
#include "serial.h"

static struct session session_table[MAX_SESSIONS];
static uint64_t next_session_id = 1;

void session_init(void) {
    for (int i = 0; i < MAX_SESSIONS; i++) {
        session_table[i].session_id = 0;
        session_table[i].state = SESSION_ZOMBIE;
        session_table[i].process_list = NULL;
        session_table[i].process_count = 0;
        session_table[i].next = NULL;
    }
    
    serial_puts(SERIAL_COM1, "Session manager initialized\n");
}

static struct session* session_alloc(void) {
    for (int i = 0; i < MAX_SESSIONS; i++) {
        if (session_table[i].session_id == 0) {
            return &session_table[i];
        }
    }
    return NULL;
}

struct session* session_create(uint64_t pwid) {
    struct session *sess = session_alloc();
    if (sess == NULL) {
        serial_puts(SERIAL_COM1, "Failed to allocate session\n");
        return NULL;
    }
    
    sess->session_id = next_session_id++;
    sess->pwid = pwid;
    sess->parent_sid = 0;
    sess->terminal = 0;
    sess->create_time = 0;
    sess->state = SESSION_ACTIVE;
    sess->process_list = NULL;
    sess->process_count = 0;
    sess->next = NULL;
    
    serial_puts(SERIAL_COM1, "Session created: SID=");
    serial_put_dec(SERIAL_COM1, sess->session_id);
    serial_puts(SERIAL_COM1, ", PWID=0x");
    serial_put_hex(SERIAL_COM1, pwid);
    serial_puts(SERIAL_COM1, "\n");
    
    return sess;
}

void session_destroy(uint64_t session_id) {
    struct session *sess = session_find_by_id(session_id);
    if (sess == NULL) return;
    
    struct process *proc = sess->process_list;
    while (proc != NULL) {
        struct process *next = proc->next;
        process_exit(proc, 0);
        proc = next;
    }
    
    sess->session_id = 0;
    sess->state = SESSION_ZOMBIE;
    sess->process_list = NULL;
    sess->process_count = 0;
    
    serial_puts(SERIAL_COM1, "Session destroyed: SID=");
    serial_put_dec(SERIAL_COM1, session_id);
    serial_puts(SERIAL_COM1, "\n");
}

struct session* session_find_by_id(uint64_t session_id) {
    for (int i = 0; i < MAX_SESSIONS; i++) {
        if (session_table[i].session_id == session_id) {
            return &session_table[i];
        }
    }
    return NULL;
}
