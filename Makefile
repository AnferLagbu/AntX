CC = x86_64-linux-gnu-gcc
LD = x86_64-linux-gnu-ld
AS = nasm

CFLAGS = -std=c11 -m64 -Wall -Wextra -nostdinc -nostdlib -fPIC -fno-stack-protector \
         -fno-asynchronous-unwind-tables -fno-ident -mcmodel=medium \
         -Isrc/include

USER_CFLAGS = -std=c11 -m64 -Wall -Wextra -nostdinc -nostdlib -fPIC \
              -fno-asynchronous-unwind-tables -fno-ident -fno-builtin \
              -fno-stack-protector \
              -Isrc/include

LDFLAGS = -T src/link.ld -nostdlib -Map=build/kernel.map

USER_LDFLAGS = -T src/user/link.ld -nostdlib -Map=build/user.map

ASFLAGS = -f elf64

KERNEL_OBJS = build/boot.o build/entry.o build/main.o build/serial.o build/gdt.o build/gdt_asm.o build/idt.o build/isr.o \
              build/pmm.o build/vmm.o build/process.o build/scheduler.o build/session.o build/switch.o build/pwid.o \
              build/vfs.o build/ramfs.o build/diskfs.o build/devfs.o build/procfs.o build/hvfs.o \
              build/syscall.o build/keyboard.o build/shell.o build/string.o build/printk.o build/ata.o build/install_guide.o \
              build/timer.o build/user_proc.o build/user/embedded/user_init_bin.o build/stack_canary.o build/log_buffer.o

USER_LIB_OBJS = build/user/lib/user.o build/user/lib/stack_canary.o

USER_INIT_OBJS = build/user/init/main.o build/user/antxsh/builtins.o

USER_ANTXSH_OBJS = build/user/antxsh/main.o build/user/antxsh/builtins.o

USER_INSTALL_OBJS = build/user/install/user_install.o

DISK_IMAGE = build/antx.img

LOG_DIR = logs

.PHONY: all clean run debug log iso run-iso disk run-disk user

all: build/kernel.bin user

user: build/user/init.bin build/user/antxsh.bin build/user/install.bin
	@echo "User programs built successfully"

build/kernel.bin: $(KERNEL_OBJS)
	$(LD) $(LDFLAGS) -o $@ $(KERNEL_OBJS)

build/%.o: src/kernel/%.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/%.o: src/kernel/%.asm
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@

build/gdt_asm.o: src/kernel/gdt.asm
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@

build/entry.o: src/kernel/entry.asm
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@

build/pmm.o: src/mm/pmm.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/vmm.o: src/mm/vmm.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/process.o: src/proc/process.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/scheduler.o: src/proc/scheduler.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/session.o: src/proc/session.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/switch.o: src/proc/switch.asm
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@

build/user_proc.o: src/proc/user_proc.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/user/embedded/user_init_bin.o: src/user/embedded/user_init_bin.c build/user/init.bin
	@mkdir -p build/user/embedded
	$(CC) $(CFLAGS) -c $< -o $@

build/pwid.o: src/pwid/pwid.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/vfs.o: src/fs/vfs.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/ramfs.o: src/fs/ramfs.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/diskfs.o: src/fs/diskfs.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/devfs.o: src/fs/devfs.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/procfs.o: src/fs/procfs.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/hvfs.o: src/hvfs/hvfs.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/ata.o: src/disk/ata.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/syscall.o: src/kernel/syscall.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/keyboard.o: src/kernel/keyboard.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/shell.o: src/kernel/shell.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/install_guide.o: src/kernel/install_guide.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/string.o: src/lib/string.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/printk.o: src/lib/printk.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/stack_canary.o: src/kernel/stack_canary.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/log_buffer.o: src/kernel/log_buffer.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/user/lib/user.o: src/user/lib/user.c
	@mkdir -p build/user/lib
	$(CC) $(USER_CFLAGS) -c $< -o $@

build/user/lib/stack_canary.o: src/user/lib/stack_canary.c
	@mkdir -p build/user/lib
	$(CC) $(USER_CFLAGS) -c $< -o $@

build/user/init/main.o: src/user/init/main.c
	@mkdir -p build/user/init
	$(CC) $(USER_CFLAGS) -c $< -o $@

build/user/antxsh/main.o: src/user/antxsh/main.c
	@mkdir -p build/user/antxsh
	$(CC) $(USER_CFLAGS) -c $< -o $@

build/user/antxsh/builtins.o: src/user/antxsh/builtins.c
	@mkdir -p build/user/antxsh
	$(CC) $(USER_CFLAGS) -c $< -o $@

build/user/install/user_install.o: src/user/install/user_install.c
	@mkdir -p build/user/install
	$(CC) $(USER_CFLAGS) -c $< -o $@

build/user/init.bin: $(USER_LIB_OBJS) $(USER_INIT_OBJS) $(USER_INSTALL_OBJS)
	@mkdir -p build/user
	$(LD) $(USER_LDFLAGS) -o $@ $(USER_LIB_OBJS) $(USER_INIT_OBJS) $(USER_INSTALL_OBJS)
	@echo "Generating embedded binary data..."
	@python3 scripts/gen_embed.py $@ src/user/embedded/user_init_bin.c build_user_init_bin

build/user/antxsh.bin: $(USER_LIB_OBJS) $(USER_ANTXSH_OBJS)
	@mkdir -p build/user
	$(LD) $(USER_LDFLAGS) -o $@ $(USER_LIB_OBJS) $(USER_ANTXSH_OBJS)

build/user/install.bin: $(USER_LIB_OBJS) $(USER_INSTALL_OBJS)
	@mkdir -p build/user
	$(LD) $(USER_LDFLAGS) -o $@ $(USER_LIB_OBJS) $(USER_INSTALL_OBJS)

src/user/embedded/user_init_bin.c: build/user/init.bin
	@python3 scripts/gen_embed.py $< $@ build_user_init_bin

$(DISK_IMAGE): build/kernel.bin user
	@echo "Creating disk image..."
	@dd if=/dev/zero of=$@ bs=1M count=4 2>/dev/null
	@dd if=build/kernel.bin of=$@ bs=512 seek=2 conv=notrunc 2>/dev/null
	@echo "Disk image created: $@ (4MB)"

disk: $(DISK_IMAGE)

run-disk: $(DISK_IMAGE)
	qemu-system-x86_64 -drive file=$(DISK_IMAGE),format=raw -serial stdio

iso: all user
	@mkdir -p isodir/boot/grub
	cp build/kernel.bin isodir/boot/kernel.bin
	mkdir -p isodir/bin
	cp build/user/init.bin isodir/bin/init
	cp build/user/antxsh.bin isodir/bin/antxsh
	cp build/user/install.bin isodir/bin/install
	echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	echo 'set default=0' >> isodir/boot/grub/grub.cfg
	echo '' >> isodir/boot/grub/grub.cfg
	echo 'menuentry "AntX" {' >> isodir/boot/grub/grub.cfg
	echo '    multiboot2 /boot/kernel.bin' >> isodir/boot/grub/grub.cfg
	echo '}' >> isodir/boot/grub/grub.cfg
	grub2-mkrescue -o build/antx.iso isodir

clean:
	rm -rf build/ isodir/

run: all user
	qemu-system-x86_64 -kernel build/kernel.bin -serial stdio

run-iso: iso
	qemu-system-x86_64 -cdrom build/antx.iso -serial stdio

debug: all user
	qemu-system-x86_64 -kernel build/kernel.bin -serial stdio -s -S

log: all user
	@mkdir -p $(LOG_DIR)
	qemu-system-x86_64 -kernel build/kernel.bin -serial file:$(LOG_DIR)/serial.log -display none
	@echo "Serial log saved to $(LOG_DIR)/serial.log"

run-iso-debug: iso
	@mkdir -p $(LOG_DIR)
	timeout 10 qemu-system-x86_64 -cdrom build/antx.iso -serial stdio -no-reboot \
		-d int,cpu_reset,unimp,guest_errors,in_asm \
		-D $(LOG_DIR)/qemu_debug.log || true
	@echo ""
	@echo "QEMU debug log saved to $(LOG_DIR)/qemu_debug.log"
	@echo "Run 'cat $(LOG_DIR)/qemu_debug.log' to view details"

debug-iso: iso
	qemu-system-x86_64 -cdrom build/antx.iso -serial stdio -no-reboot -s -S &
	@echo "QEMU started in debug mode on port 1234"
	@echo "Connect with: gdb -ex 'target remote localhost:1234' -ex 'symbol-file build/kernel.bin'"
