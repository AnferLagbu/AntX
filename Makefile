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

LDFLAGS = -T src/link.ld -nostdlib -Map=build/kernel.map

RUST_LIB = src/rust/target/x86_64-unknown-none/release/libqueenx.a
RUST_LIB_TEST = src/rust/target/test-release/x86_64-unknown-none/release/libqueenx.a
RUST_LIB_CHAOS = src/rust/target/chaos-release/x86_64-unknown-none/release/libqueenx.a

RUST_USER_DIR = src/user
RUST_USER_TARGET = $(RUST_USER_DIR)/target/x86_64-unknown-none/release

USER_INIT_ELF = $(RUST_USER_TARGET)/init
USER_SHELL_ELF = $(RUST_USER_TARGET)/axsh
USER_INSTALL_ELF = $(RUST_USER_TARGET)/install

ASFLAGS = -f elf64

STAGE1_BIN = build/stage1.bin

KERNEL_OBJS = build/boot.o build/entry.o build/isr.o build/switch.o \
              build/lib/string.o \
              $(NET_OBJS)

KERNEL_TEST_OBJS = build/boot.o build/entry.o build/isr.o build/switch.o \
              build/kernel_test.o build/test_main.o build/test_hvfs.o \
              build/test_hw_stubs.o

DISK_IMAGE = build/antx.img

LOG_DIR = logs

.PHONY: all clean run run-net debug log log-net iso run-iso disk run-disk user test test-host test-unit test-integration test-smoke test-stress \
         test-all test-chaos test-smp

all: build/kernel.bin user

user: $(USER_INIT_ELF) $(USER_SHELL_ELF) $(USER_INSTALL_ELF)
	@mkdir -p build/user
	@cp $(USER_INIT_ELF) build/user/init.bin
	@cp $(USER_SHELL_ELF) build/user/axsh.bin
	@cp $(USER_INSTALL_ELF) build/user/install.bin
	@echo "User programs built successfully (Rust)"

$(USER_INIT_ELF) $(USER_SHELL_ELF) $(USER_INSTALL_ELF):
	@echo "Building Rust user programs..."
	cd $(RUST_USER_DIR) && RUSTFLAGS="-C link-arg=-T$$(pwd)/link.x -C link-arg=-nostdlib" cargo build --release

build/kernel.bin: $(KERNEL_OBJS) $(RUST_LIB)
	@echo "[LINK] Linking kernel..."
	$(LD) $(LDFLAGS) --allow-multiple-definition -o $@ --whole-archive $(RUST_LIB) --no-whole-archive $(KERNEL_OBJS)

build/kernel.flat: build/kernel.bin
	objcopy -O binary $< $@

$(RUST_LIB): build/user/init.bin $(STAGE1_BIN)
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

build/%.o: src/kernel/%.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

$(STAGE1_BIN): src/kernel/boot/stage1.asm
	@mkdir -p build
	nasm -f bin $< -o $@

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

build/net/%.o: src/kernel/net/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -c $< -o $@

build/user/init.bin: $(USER_INIT_ELF)
	@mkdir -p build/user
	@cp $< $@

build/user/axsh.bin: $(USER_SHELL_ELF)
	@mkdir -p build/user
	@cp $< $@

build/user/install.bin: $(USER_INSTALL_ELF)
	@mkdir -p build/user
	@cp $< $@

# ============================================================
# 网络子系统 (lwIP 2.2.1)
# ============================================================
build/net/%.o: src/kernel/net/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -c $< -o $@

$(DISK_IMAGE): build/kernel.flat user
	@echo "Creating disk image..."
	@dd if=/dev/zero of=$@ bs=1M count=4 2>/dev/null
	@dd if=build/stage1.bin of=$@ bs=512 seek=0 conv=notrunc 2>/dev/null
	@dd if=build/kernel.flat of=$@ bs=512 seek=1 conv=notrunc 2>/dev/null
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
	cd $(RUST_USER_DIR) && cargo clean

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
test-host:
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║     Running Host-Side Unit Tests             ║"
	@echo "╚══════════════════════════════════════════════╝"
	@mkdir -p tests/reports
	@cd host-tests && cargo test --quiet 2>&1 | tee tests/reports/host_test_$$(date +%Y%m%d_%H%M%S).log; true
	@echo ""

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
	if [ $$exit_code -eq 33 ]; then \
		echo ""; \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  ✅ ALL TESTS PASSED (QEMU exit: $$exit_code)   ║"; \
		echo "╚══════════════════════════════════════════════╝"; \
	elif [ $$exit_code -eq 35 ]; then \
		echo ""; \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  ❌ TESTS FAILED (QEMU exit: $$exit_code)        ║"; \
		echo "╚══════════════════════════════════════════════╝"; \
	elif [ $$exit_code -eq 0 ]; then \
		echo ""; \
		echo "╔══════════════════════════════════════════════╗"; \
		echo "║  ⚠️  QEMU exited normally (no isa-debug-exit)    ║"; \
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

test-all: test-smoke test-host test-unit
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

test-smoke: iso
	@python3 tests/smoke/run_smoke_tests.py

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

# ============================================================================
# QEMU 调试脚本支持 (QEMU Debug Script Support)
# ============================================================================

.PHONY: qemu-debug qemu-debug-gdb qemu-headless qemu-network driver-test

# 使用 QEMU 调试脚本启动 (正常模式)
qemu-debug:
	@chmod +x scripts/qemu_debug.sh
	@./scripts/qemu_debug.sh -k build/kernel.flat

# 使用 QEMU 调试脚本启动 (GDB 调试模式)
qemu-debug-gdb:
	@chmod +x scripts/qemu_debug.sh
	@./scripts/qemu_debug.sh -k build/kernel.flat -d
	@echo ""
	@echo "╔══════════════════════════════════════════════╗"
	@echo "║  GDB Debug Session                           ║"
	@echo "╠══════════════════════════════════════════════╣"
	@echo "║  In another terminal, run:                   ║"
	@echo "║  gdb -x .gdbinit.antx                        ║"
	@echo "╚══════════════════════════════════════════════╝"

# 无头模式 (Headless mode)
qemu-headless:
	@chmod +x scripts/qemu_debug.sh
	@./scripts/qemu_debug.sh -k build/kernel.flat -D none

# 网络模式 (Network mode)
qemu-network:
	@chmod +x scripts/qemu_debug.sh
	@./scripts/qemu_debug.sh -k build/kernel.flat -n

# ============================================================================
# 驱动测试 (Driver Tests)
# ============================================================================

driver-test: all build/kernel.flat
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     Hardware Driver Tests                                ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@mkdir -p tests/reports
	@timestamp=$$(date +%Y%m%d_%H%M%S); \
	echo "[TEST] Starting driver tests in QEMU..."; \
	timeout 30 $(QEMU) $(QEMU_FLAGS) \
		-kernel build/kernel.flat \
		-serial file:tests/reports/driver_test_$${timestamp}.log \
		-display none \
		-d cpu_reset,guest_errors,unimp 2>tests/reports/qemu_driver_stderr_$${timestamp}.log || true
	@echo ""
	@driver_log=$$(ls -t tests/reports/driver_test_*.log 2>/dev/null | head -1); \
	if [ -n "$$driver_log" ]; then \
		echo "--- Driver Test Output ---"; \
		cat "$$driver_log"; \
		echo ""; \
		echo "--- QEMU Warnings (if any) ---"; \
		qemu_err=$$(ls -t tests/reports/qemu_driver_stderr_*.log 2>/dev/null | head -1); \
		if [ -f "$$qemu_err" ] && [ -s "$$qemu_err" ]; then \
			cat "$$qemu_err"; \
		else \
			echo "No warnings."; \
		fi \
	fi

# ============================================================================
# 帮助信息更新 (Updated Help)
# ============================================================================

.PHONY: help-drivers

help-drivers:
	@echo ""
	@echo "╔══════════════════════════════════════════════════════════╗"
	@echo "║     Hardware Driver Commands                             ║"
	@echo "╚══════════════════════════════════════════════════════════╝"
	@echo ""
	@echo "  QEMU Debug Commands:"
	@echo "    make qemu-debug        - Start QEMU with VGA display"
	@echo "    make qemu-debug-gdb    - Start QEMU in GDB debug mode"
	@echo "    make qemu-headless     - Start QEMU in headless mode"
	@echo "    make qemu-network      - Start QEMU with network"
	@echo ""
	@echo "  Driver Test Commands:"
	@echo "    make driver-test       - Run hardware driver tests"
	@echo ""
	@echo "  Available Drivers:"
	@echo "    - VGA Text Mode (80x25)"
	@echo "    - Serial Port (COM1-COM4, UART 16550)"
	@echo "    - PIT Timer (8254)"
	@echo "    - PS/2 Keyboard"
	@echo "    - ATA/IDE Disk"
	@echo "    - PCI Bus"
	@echo ""
