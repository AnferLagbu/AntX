CC = x86_64-linux-gnu-gcc
LD = x86_64-linux-gnu-ld
AS = nasm

CFLAGS = -std=c11 -m64 -Wall -Wextra -nostdinc -nostdlib -fPIC -fno-stack-protector \
         -fno-asynchronous-unwind-tables -fno-ident -mcmodel=medium \
         -Wno-builtin-declaration-mismatch \
         -Isrc/include -Isrc/include/tests \
         -Isrc/net -Isrc/net/lwip -Isrc/net/lwip/src/include -Isrc/net/arch -Isrc/net/driver

NET_CORE_C = $(wildcard src/net/lwip/src/core/*.c) \
             $(wildcard src/net/lwip/src/core/ipv4/*.c) \
             $(wildcard src/net/lwip/src/core/ipv6/*.c)
NET_NETIF_C = src/net/lwip/src/netif/ethernet.c
NET_APPS_C = src/net/lwip/src/apps/http/httpd.c \
             src/net/lwip/src/apps/http/fs.c \
             src/net/lwip/src/apps/http/http_client.c \
             $(wildcard src/net/lwip/src/apps/mdns/*.c) \
             $(wildcard src/net/lwip/src/apps/mqtt/*.c) \
             $(wildcard src/net/lwip/src/apps/netbiosns/*.c) \
             $(wildcard src/net/lwip/src/apps/smtp/*.c) \
             $(wildcard src/net/lwip/src/apps/sntp/*.c) \
             $(wildcard src/net/lwip/src/apps/tftp/*.c) \
             $(wildcard src/net/lwip/src/apps/lwiperf/*.c) \
             $(wildcard src/net/lwip/src/apps/snmp/*.c)
NET_QX_C   = src/net/arch/sys_arch.c \
             src/net/qx_net_init.c \
             src/net/qx_netif.c \
             src/net/driver/e1000.c \
             src/net/qx_net_apps.c

NET_ALL_C  = $(NET_CORE_C) $(NET_NETIF_C) $(NET_APPS_C) $(NET_QX_C)
NET_OBJS   = $(patsubst src/net/%.c,build/net/%.o,$(NET_ALL_C))

USER_CFLAGS = -std=c11 -m64 -Wall -Wextra -nostdinc -nostdlib -fPIC \
              -fno-asynchronous-unwind-tables -fno-ident -fno-builtin \
              -fno-stack-protector \
              -Isrc/include -Isrc/user/install

LDFLAGS = -T src/link.ld -nostdlib -Map=build/kernel.map

RUST_LIB = src/rust/target/x86_64-unknown-none/release/libqueenx.a

USER_LDFLAGS = -T src/user/link.ld -nostdlib -Map=build/user.map

ASFLAGS = -f elf64

KERNEL_OBJS = build/boot.o build/entry.o build/main.o build/serial.o build/gdt.o build/gdt_asm.o build/idt.o build/isr.o \
              build/switch.o \
              build/syscall.o build/keyboard.o build/string.o build/ata.o \
              build/timer.o build/user/embedded/user_init_bin.o build/stack_canary.o \
              build/ipc.o build/klog.o build/grub_install.o \
              build/version_registry.o \
              build/cpu.o \
              build/spinlock.o build/atomic.o build/rwlock.o build/mutex.o build/slab.o \
              build/pci.o \
              $(NET_OBJS)

KERNEL_TEST_OBJS = build/boot.o build/entry.o build/main_test.o build/serial.o build/gdt.o build/gdt_asm.o build/idt.o build/isr.o \
              build/switch.o \
              build/syscall.o build/keyboard.o build/string.o build/ata.o \
              build/timer.o build/user/embedded/user_init_bin.o build/user/embedded/test_minimal_bin.o build/stack_canary.o \
              build/ipc.o build/klog.o build/grub_install.o \
              build/version_registry.o \
              build/cpu.o \
              build/spinlock.o build/atomic.o build/rwlock.o build/mutex.o build/slab.o \
              build/pci.o \
              build/kernel_test.o build/test_main.o \
              build/test_process.o build/test_scheduler.o build/test_vfs.o build/test_syscall.o build/test_ipc.o build/test_hvfs.o \
              build/test_pwid_enhanced.o build/test_persistence.o build/test_filesystem_full.o \
              build/test_memory_safety.o build/test_edge_cases.o build/test_error_handling.o build/test_performance.o \
              build/test_process_enhanced.o build/test_scheduler_enhanced.o build/test_interrupt.o build/test_ipc_enhanced.o \
              build/test_vfs_enhanced.o build/test_syscall_enhanced.o \
              build/test_qemu_hardware.o \
              build/process_stub.o \
              build/version_registry.o \
              build/test_spinlock.o build/test_atomic.o build/test_rwlock.o build/test_mutex.o build/test_slab.o \
              build/test_pci.o build/test_dma.o \
              build/test_pmm.o build/test_vmm.o build/test_kmalloc.o \
              build/test_network.o \
              build/test_scheduler_rt.o \
              build/test_smp.o \
              build/smp.o \
              $(NET_OBJS)

USER_LIB_OBJS = build/user/lib/user.o build/user/lib/stack_canary.o

USER_INIT_OBJS = build/user/init/main.o build/user/axsh/builtins.o

USER_AXSH_OBJS = build/user/axsh/main.o build/user/axsh/builtins.o

USER_INSTALL_OBJS = build/user/install/user_install.o

DISK_IMAGE = build/antx.img

LOG_DIR = logs

# ============================================================================
# 动态版本生成 (Git-based Versioning)
# ============================================================================
# 每次构建时自动从 Git 仓库获取版本信息
# 生成文件: src/include/version_auto.h, src/include/version_registry.h
#
# 手动触发: make generate-version
# 强制重新生成: make generate-version-force
# ============================================================================

.PHONY: generate-version generate-version-force

VERSION_SCRIPT = scripts/generate_version.sh
VERSION_AUTO_H = src/include/version_auto.h
VERSION_REGISTRY_H = src/include/version_registry.h

generate-version:
	@echo "[GEN] Generating dynamic version info from Git..."
	@bash $(VERSION_SCRIPT) --verbose
	@echo "[GEN] Version files generated successfully"

generate-version-force:
	@echo "[GEN] Force regenerating version info..."
	@bash $(VERSION_SCRIPT) --verbose --force
	@echo "[GEN] Version files regenerated"

# 确保版本头文件存在 (如果不存在则生成)
$(VERSION_AUTO_H):
	@$(MAKE) generate-version

$(VERSION_REGISTRY_H):
	@$(MAKE) generate-version

.PHONY: all clean run run-net debug log log-net iso run-iso disk run-disk user test test-unit test-integration test-stress \
         test-quick test-comprehensive test-verbose test-enhanced \
         test-qemu-hw test-qemu-full test-qemu-perf test-all \
         test-cpu-host test-cpu-host-quick

all: build/kernel.bin user

user: build/user/init.bin build/user/axsh.bin build/user/install.bin
	@echo "User programs built successfully"

build/kernel.bin: $(KERNEL_OBJS) $(RUST_LIB)
	$(LD) $(LDFLAGS) -o $@ --whole-archive $(RUST_LIB) --no-whole-archive $(KERNEL_OBJS)

$(RUST_LIB):
	@echo "Building Rust kernel module..."
	cd src/rust && cargo build --release

build/pci.o: src/driver/pci.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/keyboard.o: src/driver/keyboard.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/serial.o: src/driver/serial.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/ata.o: src/driver/ata.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

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

build/switch.o: src/proc/switch.asm
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@

build/ipc.o: src/ipc/ipc.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/user/embedded/user_init_bin.o: src/user/embedded/user_init_bin.c build/user/init.bin
	@mkdir -p build/user/embedded
	$(CC) $(CFLAGS) -c $< -o $@

build/ipc.o: src/ipc/ipc.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/syscall.o: src/kernel/syscall.c
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

build/stack_canary.o: src/kernel/stack_canary.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/ipc.o: src/ipc/ipc.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/grub_install.o: src/kernel/grub_install.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/version_registry.o: src/kernel/version_registry.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/cpu.o: src/kernel/cpu.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/spinlock.o: src/kernel/spinlock.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/atomic.o: src/kernel/atomic.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/rwlock.o: src/kernel/rwlock.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/mutex.o: src/kernel/mutex.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/slab.o: src/kernel/slab.c
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

build/user/axsh/main.o: src/user/axsh/main.c
	@mkdir -p build/user/axsh
	$(CC) $(USER_CFLAGS) -c $< -o $@

build/user/axsh/builtins.o: src/user/axsh/builtins.c
	@mkdir -p build/user/axsh
	$(CC) $(USER_CFLAGS) -c $< -o $@

build/user/install/user_install.o: src/user/install/user_install.c
	@mkdir -p build/user/install
	$(CC) $(USER_CFLAGS) -c $< -o $@

build/user/init.bin: $(USER_LIB_OBJS) $(USER_INIT_OBJS) $(USER_INSTALL_OBJS)
	@mkdir -p build/user
	$(LD) $(USER_LDFLAGS) -o $@ $(USER_LIB_OBJS) $(USER_INIT_OBJS) $(USER_INSTALL_OBJS)
	@echo "Generating embedded binary data..."
	@python3 scripts/gen_embed.py $@ src/user/embedded/user_init_bin.c build_user_init_bin

build/user/axsh.bin: $(USER_LIB_OBJS) $(USER_AXSH_OBJS)
	@mkdir -p build/user
	$(LD) $(USER_LDFLAGS) -o $@ $(USER_LIB_OBJS) $(USER_AXSH_OBJS)

build/user/install.bin: $(USER_LIB_OBJS) $(USER_INSTALL_OBJS)
	@mkdir -p build/user
	$(LD) $(USER_LDFLAGS) -o $@ $(USER_LIB_OBJS) $(USER_INSTALL_OBJS)

src/user/embedded/user_init_bin.c: build/user/init.bin
	@python3 scripts/gen_embed.py $< $@ build_user_init_bin

# 最小化用户态测试二进制 (14B asm → int 0x80)
build/user/test/minimal.o: src/user/test/minimal.asm
	@mkdir -p build/user/test
	$(AS) $(ASFLAGS) $< -o $@

build/user/test_minimal.bin: build/user/test/minimal.o
	@mkdir -p build/user
	$(LD) -T src/user/link.ld -nostdlib -o $@ $<

src/user/embedded/test_minimal_bin.c: build/user/test_minimal.bin
	@mkdir -p src/user/embedded
	@python3 scripts/gen_embed.py $< $@ build_user_test_minimal_bin

build/user/embedded/test_minimal_bin.o: src/user/embedded/test_minimal_bin.c
	@mkdir -p build/user/embedded
	$(CC) -m64 -nostdinc -Isrc/include -c $< -o $@

# ============================================================
# 网络子系统 (lwIP 2.2.1)
# ============================================================
build/net/%.o: src/net/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -c $< -o $@

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
	cp build/user/axsh.bin isodir/bin/axsh
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

# QEMU 基础配置
QEMU := qemu-system-x86_64
QEMU_FLAGS := -m 512 -no-reboot -device isa-debug-exit,iobase=0xf4,iosize=0x04

QEMU_NET := -device e1000,netdev=n0 \
            -netdev user,id=n0,hostfwd=tcp::8080-:80,hostname=antx

# QEMU CPU 模型配置 (用于硬件仿真测试)
# 可选值: qemu64 (默认), host (使用宿主机CPU特性), Haswell-noTSX, Skylake-Client
QEMU_CPU ?= qemu64

# 运行模式配置
# mode-interactive: 需要图形界面，适合开发调试
# mode-headless: 无头模式，适合 CI/CD 和服务器环境
ifeq ($(QEMU_MODE),headless)
	QEMU_DISPLAY := -display none -nographic -serial file:$(LOG_DIR)/serial.log
else
	QEMU_DISPLAY := -serial stdio -display gtk
endif

run: all user
	@mkdir -p $(LOG_DIR)
	$(QEMU) $(QEMU_FLAGS) -kernel build/kernel.bin $(QEMU_DISPLAY)

run-net: all user
	@mkdir -p $(LOG_DIR)
	$(QEMU) $(QEMU_FLAGS) -kernel build/kernel.bin $(QEMU_NET) $(QEMU_DISPLAY)

run-headless: all user
	@$(MAKE) QEMU_MODE=headless run
	@echo "✓ Kernel output saved to $(LOG_DIR)/serial.log"
	@cat $(LOG_DIR)/serial.log | head -100

run-iso: iso
	@mkdir -p $(LOG_DIR)
	$(QEMU) $(QEMU_FLAGS) -cdrom build/antx.iso $(QEMU_DISPLAY)

debug: all user
	$(QEMU) $(QEMU_FLAGS) -kernel build/kernel.bin -serial stdio -s -S

log: all user
	@mkdir -p $(LOG_DIR)
	timeout 30 $(QEMU) $(QEMU_FLAGS) -kernel build/kernel.bin \
		-serial file:$(LOG_DIR)/serial.log \
		-display none \
		-d cpu_reset,guest_errors 2>&1 | tee $(LOG_DIR)/qemu_stderr.log || true

log-net: all user
	@mkdir -p $(LOG_DIR)
	timeout 30 $(QEMU) $(QEMU_FLAGS) -kernel build/kernel.bin \
		$(QEMU_NET) \
		-serial file:$(LOG_DIR)/serial.log \
		-display none \
		-d cpu_reset,guest_errors 2>&1 | tee $(LOG_DIR)/qemu_stderr.log || true
	@echo ""
	@echo "=== Network Log ==="
	@grep -E "NETWORK|DRIVER.*E1000|Ping|HTTP|DNS|DHCP|ISR" $(LOG_DIR)/serial.log | head -30
	@echo ""
	@echo "=== HTTP Test ==="
	@curl -s --max-time 3 http://localhost:8080/ 2>/dev/null && echo "OK" || echo "FAIL (no response)"
	@echo ""
	@echo "╔══════════════════════════════════════════╗"
	@echo "║  Serial log: $(LOG_DIR)/serial.log       ║"
	@echo "║  QEMU stderr: $(LOG_DIR)/qemu_stderr.log ║"
	@echo "╚══════════════════════════════════════════╝"
	@if [ -f $(LOG_DIR)/serial.log ]; then \
		echo "--- Last 50 lines of serial output ---"; \
		tail -50 $(LOG_DIR)/serial.log; \
	fi

run-iso-debug: iso
	@mkdir -p $(LOG_DIR)
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	timeout 30 $(QEMU) $(QEMU_FLAGS) \
		-cdrom build/antx.iso \
		-serial file:$(LOG_DIR)/serial_$${timestamp}.log \
		-display none \
		-no-reboot \
		-d int,cpu_reset,unimp,guest_errors,in_asm \
		-D $(LOG_DIR)/qemu_debug_$${timestamp}.log || true
	@echo ""
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║  Debug logs saved:                          ║"
	@echo "║  Serial:  $(LOG_DIR)/serial_$${timestamp}.log    ║"
	@echo "║  QEMU:    $(LOG_DIR)/qemu_debug_$${timestamp}.log ║"
	@echo "╚══════════════════════════════════════════════╝"
	@if [ -f $(LOG_DIR)/qemu_debug_$${timestamp}.log ]; then \
		echo "--- QEMU Debug Output (last 50 lines) ---"; \
		tail -50 $(LOG_DIR)/qemu_debug_$${timestamp}.log; \
	fi

debug-iso: iso
	@mkdir -p $(LOG_DIR)
	$(QEMU) $(QEMU_FLAGS) \
		-cdrom build/antx.iso \
		-serial stdio \
		-no-reboot \
		-s -S &
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║  QEMU started in debug mode on port 1234     ║"
	@echo "╚══════════════════════════════════════════════╝"
	@echo "Connect with:"
	@echo "  gdb -ex 'target remote localhost:1234' \\"
	@echo "      -ex 'symbol-file build/kernel.bin'"

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

build/test_filesystem_full.o: src/kernel/tests/test_filesystem_full.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_memory_safety.o: src/kernel/tests/test_memory_safety.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_edge_cases.o: src/kernel/tests/test_edge_cases.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_error_handling.o: src/kernel/tests/test_error_handling.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_performance.o: src/kernel/tests/test_performance.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/process_stub.o: src/kernel/process_stub.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_process_enhanced.o: src/kernel/tests/test_process_enhanced.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_scheduler_enhanced.o: src/kernel/tests/test_scheduler_enhanced.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_interrupt.o: src/kernel/tests/test_interrupt.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_ipc_enhanced.o: src/kernel/tests/test_ipc_enhanced.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_vfs_enhanced.o: src/kernel/tests/test_vfs_enhanced.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_syscall_enhanced.o: src/kernel/tests/test_syscall_enhanced.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_qemu_hardware.o: src/kernel/tests/test_qemu_hardware.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_spinlock.o: src/kernel/tests/test_spinlock.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_atomic.o: src/kernel/tests/test_atomic.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_rwlock.o: src/kernel/tests/test_rwlock.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_mutex.o: src/kernel/tests/test_mutex.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_slab.o: src/kernel/tests/test_slab.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

# Rust MM 子系统测试 (PMM/VMM/Kmalloc)
build/test_pmm.o: src/kernel/tests/test_pmm.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_vmm.o: src/kernel/tests/test_vmm.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_kmalloc.o: src/kernel/tests/test_kmalloc.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_pci.o: src/kernel/tests/test_pci.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_dma.o: src/kernel/tests/test_dma.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_network.o: src/kernel/tests/test_network.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_scheduler_rt.o: src/kernel/tests/test_scheduler_rt.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_smp.o: src/kernel/tests/test_smp.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/smp.o: src/kernel/smp.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

test: test-unit

build/kernel_test.bin: $(KERNEL_TEST_OBJS) src/rust/target/x86_64-unknown-none/release/libqueenx.a
	x86_64-linux-gnu-ld -T src/link.ld -nostdlib -Map=build/kernel_test.map --allow-multiple-definition -o build/kernel_test.bin $(KERNEL_TEST_OBJS) src/rust/target/x86_64-unknown-none/release/libqueenx.a

# 单元测试（优化版）
test-unit: build/kernel_test.bin user
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║     Building & Running Unit Tests             ║"
	@echo "╚══════════════════════════════════════════════╝"
	@mkdir -p isodir/boot/grub
	@cp build/kernel_test.bin isodir/boot/kernel.bin
	@mkdir -p isodir/bin
	@cp build/user/init.bin isodir/bin/init
	@cp build/user/axsh.bin isodir/bin/axsh
	@cp build/user/install.bin isodir/bin/install
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'set default=0' >> isodir/boot/grub/grub.cfg
	@echo '' >> isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX Test" {' >> isodir/boot/grub/grub.cfg
	@echo '    multiboot2 /boot/kernel.bin' >> isodir/boot/grub/grub.cfg
	@echo '}' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_test.iso isodir 2>/dev/null
	@echo ""
	@echo "▶ Starting QEMU (timeout: 120s, memory: 512MB)..."
	@mkdir -p tests/reports
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	timeout 120 $(QEMU) $(QEMU_FLAGS) \
		-m 512 \
		-cdrom build/antx_test.iso \
		$(QEMU_NET) \
		-serial file:tests/reports/unit_test_$${timestamp}.log \
		-display none \
		-d cpu_reset 2>tests/reports/qemu_stderr_$${timestamp}.log || true
	@echo ""
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║  Test completed!                             ║"
	@echo "║  Report: tests/reports/unit_test_$${timestamp}.log ║"
	@echo "╚══════════════════════════════════════════════╝"
	@if [ -f tests/reports/unit_test_$${timestamp}.log ]; then \
		echo "--- Serial Output (last 80 lines) ---"; \
		tail -80 tests/reports/unit_test_$${timestamp}.log; \
	fi

# 快速测试（中等超时，适合频繁开发迭代）
test-quick: build/kernel_test.bin user
	@echo "▶ Quick test mode (timeout: 60s)..."
	@mkdir -p isodir/boot/grub tests/reports
	@cp build/kernel_test.bin isodir/boot/kernel.bin
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX" { multiboot2 /boot/kernel.bin }' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_quick.iso isodir 2>/dev/null
	@timeout 60 $(QEMU) $(QEMU_FLAGS) \
		-m 256 \
		-cdrom build/antx_quick.iso \
		-serial file:tests/reports/quick_test.log \
		-display none || true
	@echo "✓ Quick test log: tests/reports/quick_test.log"
	@tail -40 tests/reports/quick_test.log 2>/dev/null || echo "(no output)"

# 完整综合测试（长超时，包含所有新增测试）
test-comprehensive: build/kernel_test.bin user
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║     Comprehensive Test Suite (180s)          ║"
	@echo "╚══════════════════════════════════════════════╝"
	@mkdir -p isodir/boot/grub tests/reports
	@cp build/kernel_test.bin isodir/boot/kernel.bin
	@mkdir -p isodir/bin
	@cp build/user/init.bin isodir/bin/init
	@cp build/user/axsh.bin isodir/bin/axsh
	@cp build/user/install.bin isodir/bin/install
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'set default=0' >> isodir/boot/grub/grub.cfg
	@echo '' >> isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX Comprehensive Test" {' >> isodir/boot/grub/grub.cfg
	@echo '    multiboot2 /boot/kernel.bin' >> isodir/boot/grub/grub.cfg
	@echo '}' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_comprehensive.iso isodir 2>/dev/null
	@echo ""
	@echo "▶ Starting comprehensive QEMU test (timeout: 180s, memory: 512MB)..."
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	timeout 180 $(QEMU) $(QEMU_FLAGS) \
		-m 512 \
		-cdrom build/antx_comprehensive.iso \
		-serial file:tests/reports/comprehensive_$${timestamp}.log \
		-display none \
		-d cpu_reset 2>tests/reports/qemu_stderr_$${timestamp}.log || true
	@echo ""
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║  Comprehensive Test completed!               ║"
	@echo "║  Report: tests/reports/comprehensive_$${timestamp}.log ║"
	@echo "╚══════════════════════════════════════════════╝"
	@if [ -f tests/reports/comprehensive_$${timestamp}.log ]; then \
		echo "--- Summary ---"; \
		grep -E "(Summary:|Passed:|Failed:|Skipped:|TEST_RESULT)" tests/reports/comprehensive_$${timestamp}.log | tail -10; \
	fi

# 详细调试模式（包含完整 QEMU 调试信息）
test-verbose: build/kernel_test.bin user
	@echo "▶ Verbose test mode (with QEMU debug output)..."
	@mkdir -p isodir/boot/grub tests/reports
	@cp build/kernel_test.bin isodir/boot/kernel.bin
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX" { multiboot2 /boot/kernel.bin }' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_verbose.iso isodir 2>/dev/null
	@timeout 120 $(QEMU) $(QEMU_FLAGS) \
		-cdrom build/antx_verbose.iso \
		-serial stdio \
		-display none \
		-d int,cpu_reset,unimp,guest_errors,in_asm \
		2>&1 | tee tests/reports/verbose_test.log | head -200 || true
	@echo "✓ Full log saved to: tests/reports/verbose_test.log"

test-integration: iso
	@echo "Running integration tests..."
	@mkdir -p tests/reports
	@python3 tests/integration/run_integration_tests.py

test-stress: iso
	@echo "Running stress tests..."
	@mkdir -p tests/reports
	@python3 tests/stress/run_stress_tests.py

# ============================================================================
# QEMU AMD64 仿真平台测试 (v3.0 - 集成到测试框架)
# ============================================================================
# 这些目标专门用于在真实 AMD64 CPU 仿真环境中验证硬件交互

# QEMU 硬件级测试 (150秒超时，包含CPU/内存/中断/设备检测)
test-qemu-hw: build/kernel_test.bin user
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     🖥️  QEMU AMD64 Hardware Simulation Test           ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@echo ""
	@echo "  测试内容:"
	@echo "    • CPU 架构验证 (CPUID/长模式/SSE/MSR)"
	@echo "    • 内存管理硬件测试 (PMM/VMM/kmalloc)"
	@echo "    • 中断系统硬件测试 (IDT/IRQ/异常/定时器)"
	@echo "    • 设备驱动硬件测试 (串口/键盘/VGA)"
	@echo "    • QEMU 平台特性检测 (版本/SMP/性能)"
	@echo ""
	@mkdir -p isodir/boot/grub tests/reports tests/logs
	@cp build/kernel_test.bin isodir/boot/kernel.bin
	@mkdir -p isodir/bin
	@cp build/user/init.bin isodir/bin/init
	@cp build/user/axsh.bin isodir/bin/axsh
	@cp build/user/install.bin isodir/bin/install
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'set default=0' >> isodir/boot/grub/grub.cfg
	@echo '' >> isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX QEMU HW Test" {' >> isodir/boot/grub/grub.cfg
	@echo '    multiboot2 /boot/kernel.bin' >> isodir/boot/grub/grub.cfg
	@echo '}' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_qemu_hw.iso isodir 2>/dev/null
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	echo ""; \
	echo "▶ Starting QEMU hardware simulation test..."; \
	echo "  CPU Model: $(QEMU_CPU)"; \
	echo "  Memory: 512MB"; \
	echo "  Timeout: 150s"; \
	echo ""; \
	timeout 150 $(QEMU) $(QEMU_FLAGS) \
		-cpu $(QEMU_CPU) \
		-m 512 \
		-no-reboot \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
		-display none \
		-serial file:tests/reports/qemu_hw_$${timestamp}.log \
		-d cpu_reset,int,unimp,guest_errors \
		-D tests/logs/qemu_hw_debug_$${timestamp}.log \
		-cdrom build/antx_qemu_hw.iso \
		2>tests/reports/qemu_hw_stderr_$${timestamp}.log || true; \
	echo ""; \
	echo "╔══════════════════════════════════════════════╗"; \
	echo "║  QEMU HW Test completed!                    ║"; \
	echo "║  Report: tests/reports/qemu_hw_$${timestamp}.log ║"; \
	echo "╚══════════════════════════════════════════════╝"; \
	if [ -f tests/reports/qemu_hw_$${timestamp}.log ]; then \
		echo "--- QEMU Hardware Test Results ---"; \
		grep -E "(QEMU-HW|PASS|FAIL|SKIP|WARN)" tests/reports/qemu_hw_$${timestamp}.log | tail -30; \
	fi

# QEMU 完整仿真测试套件 (按顺序执行所有测试类型)
test-qemu-full: test-quick test-qemu-hw test-comprehensive
	@echo ""
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║     ✅ QEMU Full Simulation Suite Complete   ║"
	@echo "╚══════════════════════════════════════════════╝"
	@echo "  已执行:"
	@echo "    ✓ Quick Test (60s) - 基础功能验证"
	@echo "    ✓ QEMU HW Test (150s) - 硬件级深度检测"
	@echo "    ✓ Comprehensive Test (180s) - 全模块综合"
	@echo ""

# QEMU 性能基准测试 (120秒，采集性能指标)
test-qemu-perf: build/kernel_test.bin user
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║     ⚡ QEMU Performance Benchmark              ║"
	@echo "╚══════════════════════════════════════════════╝"
	@mkdir -p isodir/boot/grub tests/reports
	@cp build/kernel_test.bin isodir/boot/kernel.bin
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX Perf" { multiboot2 /boot/kernel.bin }' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_perf.iso isodir 2>/dev/null
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	timeout 120 $(QEMU) $(QEMU_FLAGS) \
		-cpu $(QEMU_CPU) \
		-m 512 \
		-no-reboot \
		-display none \
		-serial file:tests/reports/qemu_perf_$${timestamp}.log \
		-d cpu_reset,in_asm \
		-cdrom build/antx_perf.iso \
		2>tests/reports/qemu_perf_stderr_$${timestamp}.log || true; \
	echo "✓ Performance report: tests/reports/qemu_perf_$${timestamp}.log"

# 增强版综合测试 (包含 QEMU 硬件信息收集)
test-enhanced: build/kernel_test.bin user
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     Enhanced Comprehensive Test + QEMU Info          ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@mkdir -p isodir/boot/grub tests/reports tests/logs
	@cp build/kernel_test.bin isodir/boot/kernel.bin
	@mkdir -p isodir/bin
	@cp build/user/init.bin isodir/bin/init
	@cp build/user/axsh.bin isodir/bin/axsh
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX Enhanced" { multiboot2 /boot/kernel.bin }' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_enhanced.iso isodir 2>/dev/null
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	echo ""; \
	echo "▶ Running enhanced comprehensive test (200s)..."; \
	echo "  Collecting: Test results + QEMU platform info"; \
	echo ""; \
	timeout 200 $(QEMU) $(QEMU_FLAGS) \
		-cpu $(QEMU_CPU) \
		-m 512 \
		-no-reboot \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
		-display none \
		-serial file:tests/reports/enhanced_$${timestamp}.log \
		-d cpu_reset,int,unimp,guest_errors \
		-D tests/logs/enhanced_debug_$${timestamp}.log \
		-cdrom build/antx_enhanced.iso \
		2>tests/reports/enhanced_stderr_$${timestamp}.log || true; \
	echo ""; \
	echo "--- Enhanced Test Summary ---"; \
	if [ -f tests/reports/enhanced_$${timestamp}.log ]; then \
		echo "Test Results:"; \
		grep -E "(Summary:|passed:|failed:|skipped:)" tests/reports/enhanced_$${timestamp}.log | tail -5; \
		echo ""; \
		echo "QEMU Platform Info:"; \
		grep -E "(QEMU-HW|QEMU-Platform|QEMU-Perf)" tests/reports/enhanced_$${timestamp}.log | tail -20; \
	fi

# ============================================================================
# 宿主 CPU 模式测试 (暴露真实 CPU 特性)
# ============================================================================
# 使用 -cpu host -enable-kvm 暴露宿主机真实 CPU 特性
# 用于验证 CPU 驱动在真实硬件上的行为

test-cpu-host: build/kernel_test.bin user
	@echo "╔═════════════════════════════════════════════════════════════════╗"
	@echo "║     🖥️  QEMU Host CPU Mode Test (真实硬件特性)               ║"
	@echo "╚═════════════════════════════════════════════════════════════════╝"
	@echo ""
	@echo "  ⚠️  注意事项:"
	@echo "    • 使用宿主机 CPU 特性运行测试 (需要 KVM 支持)"
	@echo "    • 可能需要 root 权限或用户权限配置"
	@echo "    • 测试结果反映真实硬件特性，非仿真"
	@echo ""
	@echo "  测试内容:"
	@echo "    • CPU 驱动初始化和基本信息"
	@echo "    • 真实 CPUID 特性检测 (厂商/型号/扩展指令集)"
	@echo "    • MSR 寄存器读写操作"
	@echo "    • 缓存层次结构检测 (L1/L2/L3)"
	@echo "    • 多核拓扑信息 (物理核/逻辑核/SMT)"
	@echo "    • TSC 性能基准测试"
	@echo ""
	@mkdir -p isodir/boot/grub tests/reports tests/logs
	@cp build/kernel_test.bin isodir/boot/kernel.bin
	@mkdir -p isodir/bin
	@cp build/user/init.bin isodir/bin/init
	@cp build/user/axsh.bin isodir/bin/axsh
	@cp build/user/install.bin isodir/bin/install
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'set default=0' >> isodir/boot/grub/grub.cfg
	@echo '' >> isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX Host CPU Test" {' >> isodir/boot/grub/grub.cfg
	@echo '    multiboot2 /boot/kernel.bin' >> isodir/boot/grub/grub.cfg
	@echo '}' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_host_cpu.iso isodir 2>/dev/null
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	echo ""; \
	echo "▶ Starting Host CPU mode test..."; \
	echo "  Mode: -cpu host -enable-kvm (passthrough)"; \
	echo "  Memory: 1024MB (增加内存以支持完整特性检测)"; \
	echo "  Timeout: 180s"; \
	echo ""; \
	timeout 180 $(QEMU) \
		-cpu host \
		-enable-kvm \
		-m 1024 \
		-no-reboot \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
		-display none \
		-serial file:tests/reports/host_cpu_$${timestamp}.log \
		-d cpu_reset,int,unimp,guest_errors,in_asm \
		-D tests/logs/host_cpu_debug_$${timestamp}.log \
		-cdrom build/antx_host_cpu.iso \
		2>tests/reports/host_cpu_stderr_$${timestamp}.log || true; \
	echo ""; \
	echo "╔═════════════════════════════════════════════════════╗"; \
	echo "║  Host CPU Test completed!                         ║"; \
	echo "║  Report: tests/reports/host_cpu_$${timestamp}.log   ║"; \
	echo "╚═════════════════════════════════════════════════════╝"; \
	if [ -f tests/reports/host_cpu_$${timestamp}.log ]; then \
		echo ""; \
		echo "--- Host CPU Test Results ---"; \
		echo ""; \
		echo "[CPU Driver Initialization]"; \
		grep -E "(CPU-DRV.*Init|CPU driver initialized)" tests/reports/host_cpu_$${timestamp}.log | head -5; \
		echo ""; \
		echo "[CPU Information]"; \
		grep -E "(Vendor:|Brand:|Family:|Model:|Stepping:)" tests/reports/host_cpu_$${timestamp}.log | head -10; \
		echo ""; \
		echo "[Feature Detection]"; \
		grep -E "(CPU-DRV.*Feature|SSE|AVX|Virtualization)" tests/reports/host_cpu_$${timestamp}.log | head -15; \
		echo ""; \
		echo "[Cache & Topology]"; \
		grep -E "(Cache|Topology|Core|Thread)" tests/reports/host_cpu_$${timestamp}.log | head -10; \
		echo ""; \
		echo "[MSR & Performance]"; \
		grep -E "(MSR|TSC|Benchmark)" tests/reports/host_cpu_$${timestamp}.log | head -10; \
		echo ""; \
		echo "[Test Summary]"; \
		grep -E "(PASS|FAIL|SKIP|Summary:)" tests/reports/host_cpu_$${timestamp}.log | tail -20; \
	fi

# 快速宿主 CPU 测试 (60秒，仅运行 CPU 核心测试)
test-cpu-host-quick: build/kernel_test.bin user
	@echo "▶ Quick Host CPU test (60s)..."
	@mkdir -p isodir/boot/grub tests/reports
	@cp build/kernel_test.bin isodir/boot/kernel.bin
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX" { multiboot2 /boot/kernel.bin }' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_host_quick.iso isodir 2>/dev/null
	@timeout 60 $(QEMU) \
		-cpu host \
		-enable-kvm \
		-m 512 \
		-no-reboot \
		-display none \
		-serial file:tests/reports/host_cpu_quick.log \
		-cdrom build/antx_host_quick.iso \
		2>tests/reports/host_cpu_quick_stderr.log || true
	@echo "✓ Quick Host CPU log: tests/reports/host_cpu_quick.log"
	@grep -E "(CPU-DRV|Vendor:|Brand:|PASS|FAIL)" tests/reports/host_cpu_quick.log | tail -30 || echo "(no output)"

# 测试全部 (包含 QEMU 仿真测试)
test-all: test-quick test-qemu-hw test-unit test-comprehensive
	@echo ""
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     🎉 All Tests Complete!                            ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@echo "  执行的测试套件:"
	@echo "    1. Quick Test (60s)"
	@echo "    2. QEMU Hardware Simulation (150s)"
	@echo "    3. Unit Tests (120s)"
	@echo "    4. Comprehensive Tests (180s)"
	@echo "  总计: ~510 秒 (~8.5 分钟)"
	@echo ""
