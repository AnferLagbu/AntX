#include "keyboard.h"
#include "io.h"
#include "klog.h"
#include "idt.h"
#include "proc_ffi.h"

#define KBD_DATA_PORT    0x60
#define KBD_STATUS_PORT  0x64
#define KBD_CMD_PORT     0x64

struct keyboard_buffer kbd_buffer;
struct keyboard_state kbd_state;
static uint32_t waiting_pid = 0;

void keyboard_set_waiting_pid(uint32_t pid) {
    waiting_pid = pid;
}

static const char scancode_to_ascii[128] = {
    0,    0,   '1', '2', '3', '4', '5', '6',
    '7', '8', '9', '0', '-', '=',   '\b', 0,
    'q', 'w', 'e', 'r', 't', 'y', 'u', 'i',
    'o', 'p', '[', ']', '\n', 0,   'a', 's',
    'd', 'f', 'g', 'h', 'j', 'k', 'l', ';',
    '\'', '`',   0, '\\', 'z', 'x', 'c', 'v',
    'b', 'n', 'm', ',', '.', '/',   0,   '*',
    0,   ' ',   '\t',  0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   '7',
    '8', '9', '-', '4', '5', '6', '+', '1',
    '2', '3', '0', '.',   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0
};

static const char scancode_to_ascii_shift[128] = {
    0,    0,   '!', '@', '#', '$', '%', '^',
    '&', '*', '(', ')', '_', '+',   '\b', 0,
    'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I',
    'O', 'P', '{', '}', '\n', 0,   'A', 'S',
    'D', 'F', 'G', 'H', 'J', 'K', 'L', ':',
    '"', '~',   0, '|', 'Z', 'X', 'C', 'V',
    'B', 'N', 'M', '<', '>', '?',   0,   '*',
    0,   ' ',   '\t',  0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   '7',
    '8', '9', '-', '4', '5', '6', '+', '1',
    '2', '3', '0', '.',   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0
};

static void kbd_buffer_put(char c) {
    if (kbd_buffer.count < KEYBOARD_BUFFER_SIZE) {
        kbd_buffer.buffer[kbd_buffer.head] = c;
        kbd_buffer.head = (kbd_buffer.head + 1) % KEYBOARD_BUFFER_SIZE;
        kbd_buffer.count++;
    }
}

static char kbd_buffer_get(void) {
    if (kbd_buffer.count > 0) {
        char c = kbd_buffer.buffer[kbd_buffer.tail];
        kbd_buffer.tail = (kbd_buffer.tail + 1) % KEYBOARD_BUFFER_SIZE;
        kbd_buffer.count--;
        return c;
    }
    return 0;
}

static void handle_modifier_press(uint8_t scancode) {
    if (kbd_state.ext_prefix) {
        switch (scancode) {
            case KEY_RCTRL:
                kbd_state.modifiers |= MODIFIER_RCTRL;
                break;
            case KEY_RALT:
                kbd_state.modifiers |= MODIFIER_RALT;
                break;
        }
    } else {
        switch (scancode) {
            case KEY_LSHIFT:
                kbd_state.modifiers |= MODIFIER_LSHIFT;
                break;
            case KEY_RSHIFT:
                kbd_state.modifiers |= MODIFIER_RSHIFT;
                break;
            case KEY_LCTRL:
                kbd_state.modifiers |= MODIFIER_LCTRL;
                break;
            case KEY_LALT:
                kbd_state.modifiers |= MODIFIER_LALT;
                break;
            case KEY_CAPS:
                kbd_state.modifiers ^= MODIFIER_CAPS;
                break;
            case KEY_NUMLOCK:
                kbd_state.modifiers ^= MODIFIER_NUM;
                break;
            case KEY_SCROLLLOCK:
                kbd_state.modifiers ^= MODIFIER_SCROLL;
                break;
        }
    }
}

static void handle_modifier_release(uint8_t scancode) {
    if (kbd_state.ext_prefix) {
        switch (scancode) {
            case KEY_RCTRL:
                kbd_state.modifiers &= ~MODIFIER_RCTRL;
                break;
            case KEY_RALT:
                kbd_state.modifiers &= ~MODIFIER_RALT;
                break;
        }
    } else {
        switch (scancode) {
            case KEY_LSHIFT:
                kbd_state.modifiers &= ~MODIFIER_LSHIFT;
                break;
            case KEY_RSHIFT:
                kbd_state.modifiers &= ~MODIFIER_RSHIFT;
                break;
            case KEY_LCTRL:
                kbd_state.modifiers &= ~MODIFIER_LCTRL;
                break;
            case KEY_LALT:
                kbd_state.modifiers &= ~MODIFIER_LALT;
                break;
        }
    }
}

static char handle_numpad(uint8_t scancode) {
    if (kbd_state.modifiers & MODIFIER_NUM) {
        switch (scancode) {
            case KEY_KP_0: return '0';
            case KEY_KP_1: return '1';
            case KEY_KP_2: return '2';
            case KEY_KP_3: return '3';
            case KEY_KP_4: return '4';
            case KEY_KP_5: return '5';
            case KEY_KP_6: return '6';
            case KEY_KP_7: return '7';
            case KEY_KP_8: return '8';
            case KEY_KP_9: return '9';
            case KEY_KP_DOT: return '.';
            case KEY_KP_PLUS: return '+';
            case KEY_KP_MINUS: return '-';
            case KEY_KP_ASTERISK: return '*';
        }
    }
    return 0;
}

static void keyboard_isr(struct interrupt_frame *frame) {
    (void)frame;
    
    uint8_t scancode = inb(KBD_DATA_PORT);
    
    if (scancode == KEY_EXT_PREFIX) {
        kbd_state.ext_prefix = true;
        return;
    }
    
    bool is_released = (scancode & KEY_RELEASED) != 0;
    if (is_released) {
        scancode &= ~KEY_RELEASED;
        handle_modifier_release(scancode);
        kbd_state.ext_prefix = false;
        return;
    }
    
    if (scancode == KEY_LSHIFT || scancode == KEY_RSHIFT ||
        scancode == KEY_LCTRL || scancode == KEY_LALT ||
        scancode == KEY_CAPS || scancode == KEY_NUMLOCK ||
        scancode == KEY_SCROLLLOCK ||
        (kbd_state.ext_prefix && (scancode == KEY_RCTRL || scancode == KEY_RALT))) {
        handle_modifier_press(scancode);
        kbd_state.ext_prefix = false;
        return;
    }
    
    if (kbd_state.ext_prefix) {
        kbd_state.ext_prefix = false;
        return;
    }
    
    char c = handle_numpad(scancode);
    if (c == 0) {
        bool shift = (kbd_state.modifiers & MODIFIER_SHIFT) != 0;
        bool caps = (kbd_state.modifiers & MODIFIER_CAPS) != 0;
        
        if (shift) {
            c = scancode_to_ascii_shift[scancode];
        } else {
            c = scancode_to_ascii[scancode];
        }
        
        if (c >= 'a' && c <= 'z') {
            if (caps) {
                c = c - 'a' + 'A';
            }
        } else if (c >= 'A' && c <= 'Z') {
            if (caps && !shift) {
                c = c - 'A' + 'a';
            }
        }
    }
    
    if (c != 0) {
        kbd_buffer_put(c);
        if (waiting_pid != 0) {
            proc_unblock(waiting_pid);
            waiting_pid = 0;
        }
    }
}

void keyboard_init(void) {
    kbd_buffer.head = 0;
    kbd_buffer.tail = 0;
    kbd_buffer.count = 0;
    
    kbd_state.modifiers = 0;
    kbd_state.last_scancode = 0;
    kbd_state.ext_prefix = false;
    
    while (inb(KBD_STATUS_PORT) & 0x01) {
        inb(KBD_DATA_PORT);
    }
    
    outb(KBD_CMD_PORT, 0xAE);
    outb(KBD_CMD_PORT, 0x20);
    uint8_t config = inb(KBD_DATA_PORT);
    config |= 0x01;
    config &= ~0x10;
    outb(KBD_CMD_PORT, 0x60);
    outb(KBD_DATA_PORT, config);
    
    idt_set_handler(33, keyboard_isr, "keyboard");
}

bool keyboard_has_data(void) {
    return kbd_buffer.count > 0;
}

char keyboard_get_char(void) {
    return kbd_buffer_get();
}

int keyboard_read_char(void) {
    while (!keyboard_has_data()) {
        __asm__ volatile ("hlt");
    }
    return (int)keyboard_get_char();
}

int keyboard_read_line(char *buf, int max) {
    int i = 0;
    char c;
    
    while (i < max - 1) {
        while (!keyboard_has_data()) {
            __asm__ volatile ("hlt");
        }
        c = keyboard_get_char();
        
        if (c == '\n') {
            klog_kern("");
            break;
        } else if (c == '\b' || c == 0x7F) {
            if (i > 0) {
                i--;
            }
        } else if (c >= ' ' && c <= '~') {
            buf[i++] = c;
        }
    }
    
    buf[i] = '\0';
    return i;
}

uint16_t keyboard_get_modifiers(void) {
    return kbd_state.modifiers;
}

bool keyboard_is_shift_pressed(void) {
    return (kbd_state.modifiers & MODIFIER_SHIFT) != 0;
}

bool keyboard_is_ctrl_pressed(void) {
    return (kbd_state.modifiers & MODIFIER_CTRL) != 0;
}

bool keyboard_is_alt_pressed(void) {
    return (kbd_state.modifiers & MODIFIER_ALT) != 0;
}

bool keyboard_is_caps_lock(void) {
    return (kbd_state.modifiers & MODIFIER_CAPS) != 0;
}

bool keyboard_is_num_lock(void) {
    return (kbd_state.modifiers & MODIFIER_NUM) != 0;
}
