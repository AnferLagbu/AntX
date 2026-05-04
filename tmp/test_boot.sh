#!/bin/bash
# PCI/E1000 非测试模式快速验证
cd /home/anfer/Code/C/AntX
rm -rf build
make all 2>&1 | tail -1
mkdir -p isodir/boot/grub
cp build/kernel.bin isodir/boot/kernel.bin
cat > isodir/boot/grub/grub.cfg << 'GRUBEOF'
set timeout=0
set default=0
menuentry "AntX" {
    multiboot2 /boot/kernel.bin
}
GRUBEOF
grub2-mkrescue -o build/antx_test.iso isodir 2>/dev/null
timeout 15 qemu-system-x86_64 -m 512 -no-reboot -cdrom build/antx_test.iso \
  -netdev user,id=n0 -device e1000,netdev=n0 \
  -nographic 2>/dev/null | head -60
echo "==DONE=="
