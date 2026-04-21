CC = x86_64-linux-gnu-gcc
LD = x86_64-linux-gnu-ld
AS = nasm

CFLAGS = -std=c11 -m64 -Wall -Wextra -nostdinc -nostdlib -fPIC -fno-stack-protector \
         -fno-asynchronous-unwind-tables -fno-ident -mcmodel=medium \
         -Isrc/include -Isrc/include/tests

USER_CFLAGS = -std=c11 -m64 -Wall -Wextra -nostdinc -nostdlib -fPIC \
              -fno-asynchronous-unwind-tables -fno-ident -fno-builtin \
              -fno-stack-protector \
              -Isrc/include

LDFLAGS = -T src/link.ld -nostdlib -Map=build/kernel.map

RUST_LIB = src/rust/target/x86_64-unknown-none/release/libqueenx.a

USER_LDFLAGS = -T src/user/link.ld -nostdlib -Map=build/user.map

ASFLAGS = -f elf64

KERNEL_OBJS = build/boot.o build/entry.o build/main.o build/serial.o build/gdt.o build/gdt_asm.o build/idt.o build/isr.o \
              build/pmm.o build/vmm.o build/kmalloc.o build/process.o build/scheduler.o build/session.o build/switch.o build/pwid.o \
              build/vfs.o build/ramfs.o build/devfs.o build/procfs.o build/hvfs.o \
              build/syscall.o build/keyboard.o build/string.o build/printk.o build/ata.o \
              build/timer.o build/user_proc.o build/user/embedded/user_init_bin.o build/stack_canary.o build/log_buffer.o \
              build/thread.o build/scheduler_ex.o build/ipc.o

KERNEL_TEST_OBJS = build/boot.o build/entry.o build/main_test.o build/serial.o build/gdt.o build/gdt_asm.o build/idt.o build/isr.o \
              build/pmm.o build/vmm.o build/kmalloc.o build/process.o build/scheduler.o build/session.o build/switch.o build/pwid.o \
              build/vfs.o build/ramfs.o build/devfs.o build/procfs.o build/hvfs.o \
              build/syscall.o build/keyboard.o build/string.o build/printk.o build/ata.o \
              build/timer.o build/user_proc.o build/user/embedded/user_init_bin.o build/stack_canary.o build/log_buffer.o \
              build/thread.o build/scheduler_ex.o build/ipc.o \
              build/kernel_test.o build/test_main.o build/test_pmm.o build/test_vmm.o build/test_kmalloc.o \
              build/test_process.o build/test_scheduler.o build/test_vfs.o build/test_syscall.o build/test_ipc.o build/test_hvfs.o \
              build/test_pwid_enhanced.o build/test_persistence.o

USER_LIB_OBJS = build/user/lib/user.o build/user/lib/stack_canary.o

USER_INIT_OBJS = build/user/init/main.o build/user/antxsh/builtins.o

USER_ANTXSH_OBJS = build/user/antxsh/main.o build/user/antxsh/builtins.o

USER_INSTALL_OBJS = build/user/install/user_install.o

DISK_IMAGE = build/antx.img

LOG_DIR = logs

.PHONY: all clean run debug log iso run-iso disk run-disk user test test-unit test-integration test-stress

all: build/kernel.bin user

user: build/user/init.bin build/user/antxsh.bin build/user/install.bin
	@echo "User programs built successfully"

build/kernel.bin: $(KERNEL_OBJS) $(RUST_LIB)
	$(LD) $(LDFLAGS) -o $@ $(KERNEL_OBJS) $(RUST_LIB)

$(RUST_LIB):
	@echo "Building Rust kernel module..."
	cd src/rust && cargo build --release

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

build/kmalloc.o: src/mm/kmalloc.c
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

build/thread.o: src/proc/thread.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/scheduler_ex.o: src/proc/scheduler_ex.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/ipc.o: src/ipc/ipc.c
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
	cd src/rust && cargo clean

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

build/main.o: src/kernel/main.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/main_test.o: src/kernel/main.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/kernel_test.o: src/kernel/tests/kernel_test.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_main.o: src/kernel/tests/test_main.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_pmm.o: src/kernel/tests/test_pmm.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_vmm.o: src/kernel/tests/test_vmm.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_kmalloc.o: src/kernel/tests/test_kmalloc.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_process.o: src/kernel/tests/test_process.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_scheduler.o: src/kernel/tests/test_scheduler.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_vfs.o: src/kernel/tests/test_vfs.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_syscall.o: src/kernel/tests/test_syscall.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_ipc.o: src/kernel/tests/test_ipc.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_hvfs.o: src/kernel/tests/test_hvfs.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_pwid_enhanced.o: src/kernel/tests/test_pwid_enhanced.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_persistence.o: src/kernel/tests/test_persistence.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

test: test-unit

build/kernel_test.bin: $(KERNEL_TEST_OBJS) src/rust/target/x86_64-unknown-none/release/libqueenx.a
	x86_64-linux-gnu-ld -T src/link.ld -nostdlib -Map=build/kernel_test.map -o build/kernel_test.bin $(KERNEL_TEST_OBJS) src/rust/target/x86_64-unknown-none/release/libqueenx.a

test-unit: build/kernel_test.bin user
	@echo "Building test ISO..."
	@mkdir -p isodir/boot/grub
	@cp build/kernel_test.bin isodir/boot/kernel.bin
	@mkdir -p isodir/bin
	@cp build/user/init.bin isodir/bin/init
	@cp build/user/antxsh.bin isodir/bin/antxsh
	@cp build/user/install.bin isodir/bin/install
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'set default=0' >> isodir/boot/grub/grub.cfg
	@echo '' >> isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX Test" {' >> isodir/boot/grub/grub.cfg
	@echo '    multiboot2 /boot/kernel.bin' >> isodir/boot/grub/grub.cfg
	@echo '}' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_test.iso isodir
	@echo "Running kernel unit tests..."
	@mkdir -p tests/reports
	@timeout 120 qemu-system-x86_64 -m 512M -cdrom build/antx_test.iso -serial stdio -display none 2>&1 | tee tests/reports/unit_test_$(shell date +%Y%m%d_%H%M%S).log
	@echo "Test completed. Check tests/reports/ for results."

test-integration: iso
	@echo "Running integration tests..."
	@mkdir -p tests/reports
	@python3 tests/integration/run_integration_tests.py

test-stress: iso
	@echo "Running stress tests..."
	@mkdir -p tests/reports
	@python3 tests/stress/run_stress_tests.py
