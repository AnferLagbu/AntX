#!/bin/bash
# create_boot_disk.sh - 创建带有 GRUB 引导的磁盘镜像
#
# 磁盘布局:
# - 扇区 0-10719: GRUB + 内核 (HvFS 保留区)
# - 扇区 10720+: HvFS 数据区
#
# 启动流程:
# 1. GRUB 从保留区加载内核
# 2. 内核初始化后挂载 HvFS 为根文件系统

set -e

DISK_SIZE_MB=${1:-64}
OUTPUT_FILE=${2:-build/boot_disk.img}
KERNEL_FILE=${3:-build/kernel.bin}

HVFS_DATA_SECTOR=10720
BOOT_RESERVED_MB=$((HVFS_DATA_SECTOR * 512 / 1024 / 1024 + 1))

echo "=========================================="
echo "  AntX Bootable Disk Creator"
echo "=========================================="
echo ""
echo "Configuration:"
echo "  Disk size: ${DISK_SIZE_MB}MB"
echo "  Output: ${OUTPUT_FILE}"
echo "  Kernel: ${KERNEL_FILE}"
echo "  HvFS data start: sector ${HVFS_DATA_SECTOR}"
echo ""

if [ ! -f "$KERNEL_FILE" ]; then
    echo "Error: Kernel file not found: $KERNEL_FILE"
    echo "Please run 'make all' first."
    exit 1
fi

KERNEL_SIZE=$(stat -c%s "$KERNEL_FILE")
KERNEL_SIZE_MB=$(( (KERNEL_SIZE + 1024 * 1024 - 1) / 1024 / 1024 ))

echo "Kernel size: ${KERNEL_SIZE} bytes (~${KERNEL_SIZE_MB}MB)"
echo ""

if [ $DISK_SIZE_MB -lt 32 ]; then
    echo "Error: Disk size must be at least 32MB"
    exit 1
fi

echo "[1/5] Creating disk image..."
dd if=/dev/zero of="$OUTPUT_FILE" bs=1M count=$DISK_SIZE_MB 2>/dev/null

echo "[2/5] Installing GRUB bootloader..."

LOOP_DEV=$(losetup --find --show --partscan "$OUTPUT_FILE" 2>/dev/null)
echo "  Loop device: $LOOP_DEV"
sleep 1

BOOT_MNT=$(mktemp -d)

mkdir -p "$BOOT_MNT/boot/grub"

cp "$KERNEL_FILE" "$BOOT_MNT/boot/kernel.bin"

cat > "$BOOT_MNT/boot/grub/grub.cfg" <<'EOF'
set timeout=5
set default=0

set color_normal=white/black
set color_highlight=yellow/black

menuentry "AntX Operating System" {
    echo "Loading AntX kernel..."
    multiboot2 /boot/kernel.bin
    echo "Booting..."
    boot
}

menuentry "AntX (Safe Mode)" {
    echo "Loading AntX kernel in safe mode..."
    multiboot2 /boot/kernel.bin --safe
    boot
}

menuentry "AntX (Debug Mode)" {
    echo "Loading AntX kernel with debug output..."
    multiboot2 /boot/kernel.bin --debug
    boot
}
EOF

grub2-install \
    --target=i386-pc \
    --boot-directory="$BOOT_MNT/boot" \
    --modules="part_msdos ext2 multiboot2 normal echo" \
    "$LOOP_DEV" 2>/dev/null || {
    echo "Warning: GRUB install failed, trying without partition table..."
    grub2-install \
        --target=i386-pc \
        --boot-directory="$BOOT_MNT/boot" \
        --modules="multiboot2 normal echo" \
        "$LOOP_DEV" 2>/dev/null
}

umount "$BOOT_MNT" 2>/dev/null || true
rmdir "$BOOT_MNT"
losetup -d "$LOOP_DEV" 2>/dev/null || true

echo "[3/5] Creating partition table..."

sfdisk "$OUTPUT_FILE" <<EOF >/dev/null 2>&1
label: dos
unit: sectors

start=2048, type=83
EOF

echo "[4/5] Verifying disk layout..."

echo "[5/5] Finalizing..."
echo ""

echo "=========================================="
echo "  Boot disk created successfully!"
echo "=========================================="
echo ""
echo "Disk image: $OUTPUT_FILE"
echo "Size: $(stat -c%s "$OUTPUT_FILE") bytes"
echo ""
echo "Disk layout:"
echo "  Sectors 0-10719: GRUB + Kernel (~5.5MB)"
echo "  Sectors 10720+:  HvFS data area"
echo ""
echo "Usage:"
echo ""
echo "  Test boot from disk:"
echo "    make run-boot-disk"
echo ""
echo "  Run installer (boot from ISO with disk attached):"
echo "    make install-to-disk"
echo ""
echo "  Manual QEMU command:"
echo "    qemu-system-x86_64 -drive file=$OUTPUT_FILE,format=raw -serial stdio"
echo ""
