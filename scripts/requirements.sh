#!/bin/bash

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

print_header() {
    echo -e "${YELLOW}========================================${NC}"
    echo -e "${YELLOW}  AntX Kernel Build Requirements${NC}"
    echo -e "${YELLOW}========================================${NC}"
    echo ""
}

print_check() {
    printf "  %-30s" "$1"
}

print_ok() {
    echo -e "${GREEN}[OK]${NC}"
}

print_missing() {
    echo -e "${RED}[MISSING]${NC}"
}

print_info() {
    echo -e "    -> $1"
}

check_command() {
    if command -v "$1" &> /dev/null; then
        print_ok
        return 0
    else
        print_missing
        return 1
    fi
}

check_package() {
    if dpkg -l "$1" &> /dev/null 2>&1; then
        return 0
    else
        return 1
    fi
}

MISSING_PACKAGES=()
MISSING_COMMANDS=()

print_header

echo "Checking required tools..."
echo ""

print_check "GCC Cross Compiler (x86_64)"
if check_command "x86_64-linux-gnu-gcc"; then
    VERSION=$(x86_64-linux-gnu-gcc --version | head -1)
    print_info "$VERSION"
else
    MISSING_PACKAGES+=("gcc-x86-64-linux-gnu")
fi

print_check "LD Linker (x86_64)"
if check_command "x86_64-linux-gnu-ld"; then
    VERSION=$(x86_64-linux-gnu-ld --version | head -1)
    print_info "$VERSION"
else
    MISSING_PACKAGES+=("binutils-x86-64-linux-gnu")
fi

print_check "NASM Assembler"
if check_command "nasm"; then
    VERSION=$(nasm --version)
    print_info "$VERSION"
else
    MISSING_PACKAGES+=("nasm")
fi

print_check "Rust Compiler"
if check_command "rustc"; then
    VERSION=$(rustc --version)
    print_info "$VERSION"
else
    MISSING_COMMANDS+=("rustc")
fi

print_check "Cargo"
if check_command "cargo"; then
    VERSION=$(cargo --version)
    print_info "$VERSION"
else
    MISSING_COMMANDS+=("cargo")
fi

print_check "Rustup"
if check_command "rustup"; then
    VERSION=$(rustup --version 2>/dev/null | head -1)
    print_info "$VERSION"
else
    MISSING_COMMANDS+=("rustup")
fi

print_check "Rust Nightly Toolchain"
if rustup show | grep -q "nightly"; then
    print_ok
    TOOLCHAIN=$(rustup show active-toolchain 2>/dev/null || echo "unknown")
    print_info "$TOOLCHAIN"
else
    echo -e "${YELLOW}[NOT INSTALLED]${NC}"
    print_info "Run: rustup toolchain install nightly"
fi

print_check "Rust rust-src Component"
if rustup component list | grep -q "rust-src.*installed"; then
    print_ok
else
    echo -e "${YELLOW}[NOT INSTALLED]${NC}"
    print_info "Run: rustup component add rust-src"
fi

print_check "QEMU System x86_64"
if check_command "qemu-system-x86_64"; then
    VERSION=$(qemu-system-x86_64 --version | head -1)
    print_info "$VERSION"
else
    MISSING_PACKAGES+=("qemu-system-x86")
fi

print_check "GRUB2 Mkrescue"
if check_command "grub2-mkrescue"; then
    print_ok
else
    if check_command "grub-mkrescue"; then
        print_ok
        print_info "Using grub-mkrescue (alternative)"
    else
        MISSING_PACKAGES+=("grub2-common" "grub-pc-bin")
    fi
fi

print_check "XORRISO"
if check_command "xorriso"; then
    VERSION=$(xorriso --version 2>&1 | head -1)
    print_info "$VERSION"
else
    MISSING_PACKAGES+=("xorriso")
fi

print_check "Python 3"
if check_command "python3"; then
    VERSION=$(python3 --version)
    print_info "$VERSION"
else
    MISSING_PACKAGES+=("python3")
fi

print_check "Make"
if check_command "make"; then
    VERSION=$(make --version | head -1)
    print_info "$VERSION"
else
    MISSING_PACKAGES+=("make")
fi

print_check "Script (terminal recorder)"
if check_command "script"; then
    print_ok
else
    MISSING_PACKAGES+=("util-linux")
fi

echo ""
echo "========================================"
echo "  Summary"
echo "========================================"

if [ ${#MISSING_PACKAGES[@]} -eq 0 ] && [ ${#MISSING_COMMANDS[@]} -eq 0 ]; then
    echo -e "${GREEN}All requirements are satisfied!${NC}"
    exit 0
fi

if [ ${#MISSING_PACKAGES[@]} -gt 0 ]; then
    echo ""
    echo -e "${RED}Missing packages:${NC}"
    for pkg in "${MISSING_PACKAGES[@]}"; do
        echo "  - $pkg"
    done
fi

if [ ${#MISSING_COMMANDS[@]} -gt 0 ]; then
    echo ""
    echo -e "${RED}Missing commands (need manual install):${NC}"
    for cmd in "${MISSING_COMMANDS[@]}"; do
        echo "  - $cmd"
    done
fi

echo ""
echo "========================================"
echo "  Install Commands"
echo "========================================"

if [ ${#MISSING_PACKAGES[@]} -gt 0 ]; then
    echo ""
    echo "For Debian/Ubuntu:"
    echo "  sudo apt update"
    echo "  sudo apt install ${MISSING_PACKAGES[*]}"
    echo ""
    echo "For Fedora:"
    INSTALL_CMD="sudo dnf install"
    for pkg in "${MISSING_PACKAGES[@]}"; do
        case $pkg in
            "gcc-x86-64-linux-gnu") INSTALL_CMD="$INSTALL_CMD gcc-x86_64-linux-gnu" ;;
            "binutils-x86-64-linux-gnu") INSTALL_CMD="$INSTALL_CMD binutils-x86_64-linux-gnu" ;;
            "grub2-common") INSTALL_CMD="$INSTALL_CMD grub2-tools" ;;
            "grub-pc-bin") INSTALL_CMD="$INSTALL_CMD grub2-pc-modules" ;;
            "util-linux") ;;  # Already installed by default on Fedora
            *) INSTALL_CMD="$INSTALL_CMD $pkg" ;;
        esac
    done
    echo "  $INSTALL_CMD"
fi

if [ ${#MISSING_COMMANDS[@]} -gt 0 ]; then
    echo ""
    echo "For Rust (if missing):"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "  rustup toolchain install nightly"
    echo "  rustup component add rust-src"
fi

echo ""
echo "After installing, run this script again to verify."
exit 1
