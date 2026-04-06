#include "keyboard.h"
#include "io.h"
#include "serial.h"
#include "idt.h"

#define KBD_DATA_PORT    0x60
#define KBD_STATUS_PORT  0x64

struct keyboard_buffer kbd_buffer;

static int shift_pressed = 0;
static int ctrl_pressed = 0;
static int caps_lock = 0;

static const char scancode_to_ascii[128] = {
    0,    0,   '1', '2', '3', '4', '5', '6',
    '7', '8', '9', '0', '-', '=',   0,   0,
    'q', 'w', 'e', 'r', 't', 'y', 'u', 'i',
    'o', 'p', '[', ']', '\n', 0,   'a', 's',
    'd', 'f', 'g', 'h', 'j', 'k', 'l', ';',
    '\'', '`',   0, '\\', 'z', 'x', 'c', 'v',
    'b', 'n', 'm', ',', '.', '/',   0,   '*',
    0,   ' ',   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   '7',
    '8', '9', '-', '4', '5', '6', '+', '1',
    '2', '3', '0', '.',   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0
};

static const char scancode_to_ascii_shift[128] = {
    0,    0,   '!', '@', '#', '$', '%', '^',
    '&', '*', '(', ')', '_', '+',   0,   0,
    'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I',
    'O', 'P', '{', '}', '\n', 0,   'A', 'S',
    'D', 'F', 'G', 'H', 'J', 'K', 'L', ':',
    '"', '~',   0, '|', 'Z', 'X', 'C', 'V',
    'B', 'N', 'M', '<', '>', '?',   0,   '*',
    0,   ' ',   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   '7',
    '8', '9', '-', '4', '5', '6', '+', '1',
    '2', '3', '0', '.',   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,
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

static void keyboard_isr(struct interrupt_frame *frame) {
    (void)frame;
    
    uint8_t scancode = inb(KBD_DATA_PORT);
    
    if (scancode & KEY_RELEASED) {
        uint8_t released = scancode & ~KEY_RELEASED;
        if (released == KEY_LSHIFT || released == KEY_RSHIFT) {
            shift_pressed = 0;
        } else if (released == KEY_LCTRL) {
            ctrl_pressed = 0;
        }
        return;
    }
    
    switch (scancode) {
        case KEY_LSHIFT:
        case KEY_RSHIFT:
            shift_pressed = 1;
            return;
        case KEY_LCTRL:
            ctrl_pressed = 1;
            return;
        case KEY_CAPS:
            caps_lock = !caps_lock;
            return;
        case KEY_ESC:
            return;
    }
    
    char c;
    if (shift_pressed) {
        c = scancode_to_ascii_shift[scancode];
    } else {
        c = scancode_to_ascii[scancode];
    }
    
    if (c >= 'a' && c <= 'z') {
        if (caps_lock) {
            c = c - 'a' + 'A';
        }
    } else if (c >= 'A' && c <= 'Z') {
        if (caps_lock && !shift_pressed) {
            c = c - 'A' + 'a';
        }
    }
    
    if (c != 0) {
        kbd_buffer_put(c);
    }
}

void keyboard_init(void) {
    kbd_buffer.head = 0;
    kbd_buffer.tail = 0;
    kbd_buffer.count = 0;
    
    shift_pressed = 0;
    ctrl_pressed = 0;
    caps_lock = 0;
    
    while (inb(KBD_STATUS_PORT) & 0x01) {
        inb(KBD_DATA_PORT);
    }
    
    idt_set_handler(33, keyboard_isr);
    
    serial_puts(SERIAL_COM1, "  [OK] Keyboard\n");
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
        c = keyboard_get_char();
        
        if (c == '\n') {
            serial_puts(SERIAL_COM1, "\n");
            break;
        } else if (c == '\b' || c == 0x7F) {
            if (i > 0) {
                i--;
                serial_puts(SERIAL_COM1, "\b \b");
            }
        } else if (c >= ' ' && c <= '~') {
            buf[i++] = c;
            serial_putc(SERIAL_COM1, c);
        }
    }
    
    buf[i] = '\0';
    return i;
}
