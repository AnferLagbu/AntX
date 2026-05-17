CC = x86_64-linux-gnu-gcc
LD = x86_64-linux-gnu-ld
AS = nasm

CFLAGS = -std=c11 -m64 -Wall -Wextra -nostdinc -nostdlib -fPIC -fno-stack-protector \
         -fno-asynchronous-unwind-tables -fno-ident -mcmodel=medium \
         -Wno-builtin-declaration-mismatch \
         -Isrc/include -Isrc/include/tests \
         -Isrc/kernel/net -Isrc/kernel/net/lwip -Isrc/kernel/net/lwip/src/include -Isrc/kernel/net/arch -Isrc/kernel/net/driver

NET_CORE_C = $(wildcard src/kernel/net/lwip/src/core/*.c) \
             $(wildcard src/kernel/net/lwip/src/core/ipv4/*.c) \
             $(wildcard src/kernel/net/lwip/src/core/ipv6/*.c)
NET_NETIF_C = src/kernel/net/lwip/src/netif/ethernet.c
NET_APPS_C = src/kernel/net/lwip/src/apps/http/httpd.c \
             src/kernel/net/lwip/src/apps/http/fs.c \
             src/kernel/net/lwip/src/apps/http/http_client.c \
             $(wildcard src/kernel/net/lwip/src/apps/mdns/*.c) \
             $(wildcard src/kernel/net/lwip/src/apps/mqtt/*.c) \
             $(wildcard src/kernel/net/lwip/src/apps/netbiosns/*.c) \
             $(wildcard src/kernel/net/lwip/src/apps/smtp/*.c) \
             $(wildcard src/kernel/net/lwip/src/apps/sntp/*.c) \
             $(wildcard src/kernel/net/lwip/src/apps/tftp/*.c) \
             $(wildcard src/kernel/net/lwip/src/apps/lwiperf/*.c) \
             $(wildcard src/kernel/net/lwip/src/apps/snmp/*.c)
# C 桥接文件已被 Rust 重写 (sys_arch.rs / init.rs / netif.rs / apps.rs / e1000.rs)
NET_QX_C   = src/kernel/net/arch/net_glue.c
# 注意: 以下文件已用 Rust 重写:
#   - src/kernel/net/arch/sys_arch.c → sys_arch.rs
#   - src/kernel/net/qx_net_init.c → init.rs
#   - src/kernel/net/qx_netif.c    → netif.rs
#   - src/kernel/net/qx_net_apps.c → apps.rs
#   - src/kernel/net/qx_fsdata.c   → fsdata.rs

NET_ALL_C  = $(NET_CORE_C) $(NET_NETIF_C) $(NET_APPS_C) $(NET_QX_C)
NET_OBJS   = $(patsubst src/kernel/net/%.c,build/net/%.o,$(NET_ALL_C))

USER_CFLAGS = -std=c11 -m64 -Wall -Wextra -nostdinc -nostdlib -fPIC \
              -fno-asynchronous-unwind-tables -fno-ident -fno-builtin \
              -fno-stack-protector \
              -Isrc/include -Isrc/user/install

LDFLAGS = -T src/link.ld -nostdlib -Map=build/kernel.map

RUST_LIB = src/rust/target/x86_64-unknown-none/release/libqueenx.a
RUST_LIB_TEST = src/rust/target/test-release/x86_64-unknown-none/release/libqueenx.a
RUST_LIB_CHAOS = src/rust/target/chaos-release/x86_64-unknown-none/release/libqueenx.a

USER_LDFLAGS = -T src/user/link.ld -nostdlib -Map=build/user.map

ASFLAGS = -f elf64

KERNEL_OBJS = build/boot.o build/entry.o build/isr.o build/switch.o \
              build/user/embedded/user_init_bin.o \
              build/lib/string.o \
              $(NET_OBJS)

KERNEL_TEST_OBJS = build/boot.o build/entry.o build/isr.o build/switch.o \
              build/kernel_test.o build/test_main.o build/test_hvfs.o \
              build/test_hw_stubs.o

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
         test-all test-chaos test-smp

all: build/kernel.bin user

user: build/user/init.bin build/user/axsh.bin build/user/install.bin
	@echo "User programs built successfully"

build/kernel.bin: $(KERNEL_OBJS) $(RUST_LIB)
	@echo "[LINK] Linking kernel..."
	$(LD) $(LDFLAGS) --allow-multiple-definition -o $@ --whole-archive $(RUST_LIB) --no-whole-archive $(KERNEL_OBJS)

build/kernel.flat: build/kernel.bin
	objcopy -O binary $< $@

$(RUST_LIB):
	@echo "Building Rust kernel module..."
	cd src/rust && cargo build --release

$(RUST_LIB_TEST):
	@echo "Building Rust test kernel..."
	cd src/rust && cargo build --release --features kernel_test --target-dir target/test-release

$(RUST_LIB_CHAOS):
	@echo "Building Rust chaos kernel (fault_injection enabled)..."
	cd src/rust && cargo build --release --features "kernel_test fault_injection" --target-dir target/chaos-release

build/lib/string.o: src/kernel/lib/string.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -c $< -o $@

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

build/%.o: src/kernel/boot/%.asm
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@

build/gdt_asm.o: src/kernel/gdt.asm
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@

build/switch.o: src/kernel/proc/switch.asm
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

	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/cpu.o: src/kernel/cpu.c
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
build/net/%.o: src/kernel/net/%.c
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

run: all user build/kernel.flat
	@mkdir -p $(LOG_DIR)
	$(QEMU) $(QEMU_FLAGS) -kernel build/kernel.flat $(QEMU_DISPLAY)

run-net: all user build/kernel.flat
	@mkdir -p $(LOG_DIR)
	$(QEMU) $(QEMU_FLAGS) -kernel build/kernel.flat $(QEMU_NET) $(QEMU_DISPLAY)

run-headless: all user
	@$(MAKE) QEMU_MODE=headless run
	@echo "✓ Kernel output saved to $(LOG_DIR)/serial.log"
	@cat $(LOG_DIR)/serial.log | head -100

run-iso: iso
	@mkdir -p $(LOG_DIR)
	$(QEMU) $(QEMU_FLAGS) -cdrom build/antx.iso $(QEMU_DISPLAY)

debug: all user build/kernel.flat
	$(QEMU) $(QEMU_FLAGS) -kernel build/kernel.flat -serial stdio -s -S

log: all user build/kernel.flat
	@mkdir -p $(LOG_DIR)
	timeout 30 $(QEMU) $(QEMU_FLAGS) -kernel build/kernel.flat \
		-serial file:$(LOG_DIR)/serial.log \
		-display none \
		-d cpu_reset,guest_errors 2>&1 | tee $(LOG_DIR)/qemu_stderr.log || true

log-net: all user build/kernel.flat
	@mkdir -p $(LOG_DIR)
	timeout 60 $(QEMU) $(QEMU_FLAGS) -kernel build/kernel.flat \
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

build/test_hvfs.o: src/kernel/tests/test_hvfs.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

build/test_hw_stubs.o: src/kernel/tests/test_hw_stubs.c
	@mkdir -p build
	$(CC) $(CFLAGS) -DKERNEL_TEST -c $< -o $@

test: test-unit

build/kernel_test.bin: $(KERNEL_TEST_OBJS) $(RUST_LIB_TEST)
	x86_64-linux-gnu-ld -T src/link.ld -nostdlib -Map=build/kernel_test.map --allow-multiple-definition -o build/kernel_test.bin $(KERNEL_TEST_OBJS) $(RUST_LIB_TEST)

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
		-d cpu_reset 2>tests/reports/qemu_stderr_$${timestamp}.log; \
	exit_code=$$?; \
	if [ $$exit_code -eq 0 ]; then \
		echo ""; \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  ✅ ALL TESTS PASSED (QEMU exit: $$exit_code)   ║"; \
		echo "╚══════════════════════════════════════════════╝"; \
	elif [ $$exit_code -eq 33 ]; then \
		echo ""; \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  ❌ TESTS FAILED (QEMU exit: $$exit_code)        ║"; \
		echo "╚══════════════════════════════════════════════╝"; \
	else \
		echo ""; \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  ⚠️  QEMU exited with code $$exit_code (timeout/crash) ║"; \
		echo "╚══════════════════════════════════════════════╝"; \
	fi
	@echo "  Report: tests/reports/unit_test_$${timestamp}.log"
	@if [ -f tests/reports/unit_test_$${timestamp}.log ]; then \
		echo "--- Serial Output (last 80 lines) ---"; \
		tail -80 tests/reports/unit_test_$${timestamp}.log; \
	fi

test-all: test-unit
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

FAULT_RATE ?= 50

build/kernel_chaos.bin: $(KERNEL_TEST_OBJS) $(RUST_LIB_CHAOS)
	x86_64-linux-gnu-ld -T src/link.ld -nostdlib -Map=build/kernel_chaos.map --allow-multiple-definition -o build/kernel_chaos.bin $(KERNEL_TEST_OBJS) $(RUST_LIB_CHAOS)

test-chaos: build/kernel_chaos.bin user
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     Chaos/Fault Injection Tests (fault_injection=on)   ║"
	@echo "║     FAULT_RATE=$(FAULT_RATE)/1000                        ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@mkdir -p isodir/boot/grub
	@cp build/kernel_chaos.bin isodir/boot/kernel.bin
	@mkdir -p isodir/bin
	@cp build/user/init.bin isodir/bin/init
	@cp build/user/axsh.bin isodir/bin/axsh
	@cp build/user/install.bin isodir/bin/install
	@echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	@echo 'set default=0' >> isodir/boot/grub/grub.cfg
	@echo '' >> isodir/boot/grub/grub.cfg
	@echo 'menuentry "AntX Chaos Test" {' >> isodir/boot/grub/grub.cfg
	@echo '    multiboot2 /boot/kernel.bin' >> isodir/boot/grub/grub.cfg
	@echo '}' >> isodir/boot/grub/grub.cfg
	@grub2-mkrescue -o build/antx_chaos.iso isodir 2>/dev/null
	@mkdir -p tests/reports
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	echo "▶ Starting QEMU with fault injection (rate=$(FAULT_RATE)/1000, timeout: 120s)..."; \
	timeout 120 $(QEMU) $(QEMU_FLAGS) \
		-m 512 \
		-cdrom build/antx_chaos.iso \
		-serial file:tests/reports/chaos_test_$${timestamp}.log \
		-display none \
		-d cpu_reset 2>tests/reports/qemu_chaos_stderr_$${timestamp}.log || true
	@echo ""
	@timestamp=$$(ls -t tests/reports/chaos_test_*.log 2>/dev/null | head -1 | sed 's/.*chaos_test_//;s/\.log//'); \
	if [ -n "$$timestamp" ]; then \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  Chaos Test Report                           ║"; \
		echo "╚══════════════════════════════════════════════╝"; \
		python3 tests/chaos/analyze_chaos.py tests/reports/chaos_test_$${timestamp}.log 2>/dev/null || \
		echo "  (Run 'python3 tests/chaos/analyze_chaos.py tests/reports/chaos_test_$${timestamp}.log' for analysis)"; \
		echo ""; \
		echo "--- Last 80 lines of serial output ---"; \
		tail -80 tests/reports/chaos_test_$${timestamp}.log; \
	fi

test-integration: iso
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     Integration Tests                                   ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@python3 tests/integration/run_integration_tests.py

test-stress: iso
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     Stress Tests                                        ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@python3 tests/stress/run_stress_tests.py

test-smp: all user build/kernel.flat
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     SMP Tests (2 cores)                                 ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@mkdir -p tests/reports
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	timeout 60 $(QEMU) $(QEMU_FLAGS) \
		-m 512 -smp 2 \
		-kernel build/kernel.flat \
		-serial file:tests/reports/smp_test_$${timestamp}.log \
		-display none \
		-d cpu_reset 2>tests/reports/qemu_smp_stderr_$${timestamp}.log || true
	@echo ""
	@smp_log=$$(ls -t tests/reports/smp_test_*.log 2>/dev/null | head -1); \
	if [ -n "$$smp_log" ]; then \
		echo "--- SMP Test Output (last 60 lines) ---"; \
		tail -60 "$$smp_log"; \
	fi
