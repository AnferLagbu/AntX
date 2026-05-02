#!/bin/bash

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# 全局变量
AUTO_INSTALL=false
VERBOSE=false
SKIP_OPTIONAL=false

# 依赖数组
REQUIRED_PACKAGES=()
OPTIONAL_PACKAGES=()
MISSING_COMMANDS=()

print_header() {
    echo ""
    echo -e "${CYAN}╔══════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║     AntX 内核构建环境依赖检查工具 v2.0       ║${NC}"
    echo -e "${CYAN}╚══════════════════════════════════════════════╝${NC}"
    echo ""
}

print_section() {
    echo ""
    echo -e "${YELLOW}━━━ $1 ━━━${NC}"
    echo ""
}

print_check() {
    printf "  ${CYAN}%-35s${NC}" "$1"
}

print_ok() {
    echo -e "${GREEN}✓ 已安装${NC}"
}

print_missing() {
    echo -e "${RED}✗ 未找到${NC}"
}

print_optional() {
    echo -e "${YELLOW}○ 未安装 (可选)${NC}"
}

print_info() {
    echo -e "     ${BLUE}└─ $1${NC}"
}

print_warning() {
    echo -e "  ${YELLOW}⚠ 警告: $1${NC}"
}

print_success() {
    echo -e "${GREEN}✓ 成功: $1${NC}"
}

print_error() {
    echo -e "${RED}✗ 错误: $1${NC}"
}

# 询问用户 yes/no
ask_yes_no() {
    local prompt="$1"
    local default="${2:-n}"
    
    if [ "$AUTO_INSTALL" = true ]; then
        return 0
    fi
    
    local prompt_str
    if [ "$default" = "y" ]; then
        prompt_str="${prompt} [Y/n] "
    else
        prompt_str="${prompt} [y/N] "
    fi
    
    while true; do
        printf "  ${YELLOW}?${NC} %b" "$prompt_str"
        read -r response
        
        case "$response" in
            [Yy]|[Yy][Ee][Ss]) return 0 ;;
            [Nn]|[Nn][Oo]) return 1 ;;
            "") 
                if [ "$default" = "y" ]; then
                    return 0
                else
                    return 1
                fi
                ;;
            *) 
                echo -e "  ${RED}请输入 y 或 n${NC}"
                ;;
        esac
    done
}

# 检测包管理器
detect_package_manager() {
    if command -v apt-get &> /dev/null; then
        echo "apt"
    elif command -v dnf &> /dev/null; then
        echo "dnf"
    elif command -v yum &> /dev/null; then
        echo "yum"
    elif command -v pacman &> /dev/null; then
        echo "pacman"
    else
        echo "unknown"
    fi
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

# 安装包的通用函数
install_packages() {
    local pkg_manager=$(detect_package_manager)
    local packages=("$@")
    
    echo ""
    echo -e "${YELLOW}正在安装以下软件包:${NC}"
    
    for pkg in "${packages[@]}"; do
        echo -e "  • ${CYAN}$pkg${NC}"
    done
    
    if ! ask_yes_no "确认安装以上软件包？"; then
        print_warning "用户取消安装"
        return 1
    fi
    
    echo ""
    echo -e "${BLUE}执行安装命令...${NC}"
    
    case "$pkg_manager" in
        apt)
            sudo apt update && sudo apt install -y "${packages[@]}"
            ;;
        dnf)
            sudo dnf install -y "${packages[@]}"
            ;;
        yum)
            sudo yum install -y "${packages[@]}"
            ;;
        pacman)
            sudo pacman -S --noconfirm "${packages[@]}"
            ;;
        *)
            print_error "无法识别包管理器，请手动安装:"
            echo "  sudo apt install ${packages[*]}  # Debian/Ubuntu"
            echo "  sudo dnf install ${packages[*]}  # Fedora"
            return 1
            ;;
    esac
    
    if [ $? -eq 0 ]; then
        print_success "所有软件包安装完成！"
        return 0
    else
        print_error "安装过程中出现错误"
        return 1
    fi
}

# 安装 Rust 工具链
install_rust() {
    echo ""
    echo -e "${YELLOW}正在安装 Rust 工具链...${NC}"
    
    if ! ask_yes_no "确认安装 Rust (包括 rustc, cargo, rustup)？"; then
        print_warning "用户取消安装"
        return 1
    fi
    
    echo ""
    echo -e "${BLUE}下载并运行 Rust 安装脚本...${NC}"
    
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    
    if [ $? -eq 0 ]; then
        print_success "Rust 基础工具链安装完成！"
        
        # 加载环境变量
        source "$HOME/.cargo/env"
        
        # 询问是否安装 nightly
        if ask_yes_no "是否安装 Rust Nightly 工具链？" "y"; then
            echo ""
            echo -e "${BLUE}安装 Rust Nightly...${NC}"
            rustup toolchain install nightly
            
            if [ $? -eq 0 ]; then
                print_success "Rust Nightly 安装完成！"
                
                # 询问是否添加 rust-src 组件
                if ask_yes_no "是否添加 rust-src 组件（内核编译需要）？" "y"; then
                    rustup component add rust-src
                    if [ $? -eq 0 ]; then
                        print_success "rust-src 组件添加成功！"
                    else
                        print_warning "添加 rust-src 失败，可稍后手动运行: rustup component add rust-src"
                    fi
                fi
            else
                print_warning "Rust Nightly 安装失败，可稍后手动运行: rustup toolchain install nightly"
            fi
        fi
        
        return 0
    else
        print_error "Rust 安装失败"
        return 1
    fi
}

# 显示帮助信息
show_help() {
    echo ""
    echo -e "${CYAN}用法: $0 [选项]${NC}"
    echo ""
    echo "选项:"
    echo "  -y, --yes          自动确认所有提示（非交互模式）"
    echo "  -s, --skip-optional 跳过可选依赖（仅检查必需依赖）"
    echo "  -v, --verbose      显示详细的版本信息"
    echo "  -h, --help         显示此帮助信息"
    echo "  --check-only       仅检查，不提示安装"
    echo ""
    echo "示例:"
    echo "  $0                  # 交互式模式，询问每个操作"
    echo "  $0 -y               # 自动安装所有缺失依赖"
    echo "  $0 -s               # 仅检查必需依赖"
    echo "  $0 --check-only     # 仅显示状态，不安装"
    echo ""
}

# 解析命令行参数
parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            -y|--yes)
                AUTO_INSTALL=true
                shift
                ;;
            -s|--skip-optional)
                SKIP_OPTIONAL=true
                shift
                ;;
            -v|--verbose)
                VERBOSE=true
                shift
                ;;
            -h|--help)
                show_help
                exit 0
                ;;
            --check-only)
                # 仅检查模式，后面会设置标志
                CHECK_ONLY=true
                shift
                ;;
            *)
                print_error "未知参数: $1"
                show_help
                exit 1
                ;;
        esac
    done
}

# ==================== 主逻辑 ====================

parse_args "$@"

print_header

echo -e "${BLUE}检测系统信息...${NC}"
OS_INFO=$(cat /etc/os-release 2>/dev/null | grep PRETTY_NAME | cut -d'"' -f2 || echo "Unknown")
PKG_MANAGER=$(detect_package_manager)
echo -e "  操作系统: ${CYAN}$OS_INFO${NC}"
echo -e "  包管理器: ${CYAN}$PKG_MANAGER${NC}"

# ==================== 必需依赖检查 ====================
print_section "必需依赖 (Required)"

print_check "GCC 交叉编译器 (x86_64)"
if check_command "x86_64-linux-gnu-gcc"; then
    if [ "$VERBOSE" = true ]; then
        VERSION=$(x86_64-linux-gnu-gcc --version | head -1)
        print_info "$VERSION"
    fi
else
    REQUIRED_PACKAGES+=("gcc-x86-64-linux-gnu")
fi

print_check "LD 链接器 (x86_64)"
if check_command "x86_64-linux-gnu-ld"; then
    if [ "$VERBOSE" = true ]; then
        VERSION=$(x86_64-linux-gnu-ld --version | head -1)
        print_info "$VERSION"
    fi
else
    REQUIRED_PACKAGES+=("binutils-x86-64-linux-gnu")
fi

print_check "NASM 汇编器"
if check_command "nasm"; then
    if [ "$VERBOSE" = true ]; then
        VERSION=$(nasm --version | head -1)
        print_info "$VERSION"
    fi
else
    REQUIRED_PACKAGES+=("nasm")
fi

print_check "QEMU 系统 (x86_64)"
if check_command "qemu-system-x86_64"; then
    if [ "$VERBOSE" = true ]; then
        VERSION=$(qemu-system-x86_64 --version | head -1)
        print_info "$VERSION"
    fi
else
    REQUIRED_PACKAGES+=("qemu-system-x86")
fi

print_check "GRUB2 引导工具"
if check_command "grub2-mkrescue"; then
    :
elif check_command "grub-mkrescue"; then
    print_info "使用 grub-mkrescue (替代方案)"
else
    REQUIRED_PACKAGES+=("grub2-common" "grub-pc-bin")
fi

print_check "XORRISO ISO 工具"
if check_command "xorriso"; then
    if [ "$VERBOSE" = true ]; then
        VERSION=$(xorriso --version 2>&1 | head -1)
        print_info "$VERSION"
    fi
else
    REQUIRED_PACKAGES+=("xorriso")
fi

print_check "Python 3"
if check_command "python3"; then
    if [ "$VERBOSE" = true ]; then
        VERSION=$(python3 --version)
        print_info "$VERSION"
    fi
else
    REQUIRED_PACKAGES+=("python3")
fi

print_check "Make 构建工具"
if check_command "make"; then
    if [ "$VERBOSE" = true ]; then
        VERSION=$(make --version | head -1)
        print_info "$VERSION"
    fi
else
    REQUIRED_PACKAGES+=("make")
fi

print_check "Script 终端录制工具"
if check_command "script"; then
    :
else
    REQUIRED_PACKAGES+=("util-linux")
fi

# ==================== 可选依赖检查 ====================
if [ "$SKIP_OPTIONAL" = false ]; then
    print_section "可选依赖 (Optional) - Rust 工具链"

    print_check "Rust 编译器 (rustc)"
    if check_command "rustc"; then
        if [ "$VERBOSE" = true ]; then
            VERSION=$(rustc --version)
            print_info "$VERSION"
        fi
    else
        OPTIONAL_PACKAGES+=("rustc")
        MISSING_COMMANDS+=("rustc")
    fi

    print_check "Cargo 包管理器"
    if check_command "cargo"; then
        if [ "$VERBOSE" = true ]; then
            VERSION=$(cargo --version)
            print_info "$VERSION"
        fi
    else
        OPTIONAL_PACKAGES+=("cargo")
        MISSING_COMMANDS+=("cargo")
    fi

    print_check "Rustup 工具链管理"
    if check_command "rustup"; then
        if [ "$VERBOSE" = true ]; then
            VERSION=$(rustup --version 2>/dev/null | head -1)
            print_info "$VERSION"
        fi
    else
        OPTIONAL_PACKAGES+=("rustup")
        MISSING_COMMANDS+=("rustup")
    fi

    print_check "Rust Nightly 工具链"
    if command -v rustup &> /dev/null && rustup show | grep -q "nightly"; then
        TOOLCHAIN=$(rustup show active-toolchain 2>/dev/null || echo "unknown")
        print_info "$TOOLCHAIN"
    else
        print_optional
        OPTIONAL_PACKAGES+=("rust-nightly")
    fi

    print_check "Rust rust-src 组件"
    if command -v rustup &> /dev/null && rustup component list 2>/dev/null | grep -q "rust-src.*installed"; then
        :
    else
        print_optional
        OPTIONAL_PACKAGES+=("rust-src")
    fi
fi

# ==================== 结果汇总 ====================
print_section "检查结果汇总"

TOTAL_REQUIRED=${#REQUIRED_PACKAGES[@]}
TOTAL_OPTIONAL=${#OPTIONAL_PACKAGES[@]}
TOTAL_MISSING=$((TOTAL_REQUIRED + TOTAL_OPTIONAL))

if [ $TOTAL_REQUIRED -eq 0 ] && [ $TOTAL_OPTIONAL -eq 0 ]; then
    echo -e "${GREEN}✓ 所有依赖已满足！可以开始构建 AntX 内核。${NC}"
    echo ""
    echo -e "${CYAN}下一步操作:${NC}"
    echo "  1. 运行 'make all' 编译内核"
    echo "  2. 运行 'make run-iso' 在 QEMU 中启动"
    echo "  3. 运行 'make test-unit' 执行测试套件"
    exit 0
fi

# 显示缺失的必需依赖
if [ $TOTAL_REQUIRED -gt 0 ]; then
    echo ""
    echo -e "${RED}✗ 缺失必需依赖 ($TOTAL_REQUIRED 项):${NC}"
    for pkg in "${REQUIRED_PACKAGES[@]}"; do
        echo -e "  ${RED}•${NC} $pkg"
    done
fi

# 显示缺失的可选依赖
if [ $TOTAL_OPTIONAL -gt 0 ] && [ "$SKIP_OPTIONAL" = false ]; then
    echo ""
    echo -e "${YELLOW}○ 缺失可选依赖 ($TOTAL_OPTIONAL 项):${NC}"
    for pkg in "${OPTIONAL_PACKAGES[@]}"; do
        echo -e "  ${YELLOW}•${NC} $pkg"
    done
    echo ""
    echo -e "${BLUE}说明: 可选依赖用于编译 Rust 模块，如不需要可跳过 (-s 参数)${NC}"
fi

# ==================== 交互式安装 ====================
if [ "${CHECK_ONLY:-false}" = true ]; then
    echo ""
    echo -e "${BLUE}检查模式：未执行任何安装操作${NC}"
    echo ""
    echo -e "${CYAN}要安装缺失的依赖，请重新运行:${NC}"
    echo "  $0  # 交互式安装"
    echo "  $0 -y  # 自动安装"
    exit 1
fi

# 询问是否安装必需依赖
if [ $TOTAL_REQUIRED -gt 0 ]; then
    echo ""
    if ask_yes_no "是否自动安装缺失的必需依赖？" "y"; then
        # 根据包管理器转换包名
        case "$PKG_MANAGER" in
            apt)
                INSTALL_LIST=()
                for pkg in "${REQUIRED_PACKAGES[@]}"; do
                    case $pkg in
                        "gcc-x86-64-linux-gnu") INSTALL_LIST+=("gcc-x86_64-linux-gnu") ;;
                        "binutils-x86-64-linux-gnu") INSTALL_LIST+=("binutils-x86_64-linux-gnu") ;;
                        *) INSTALL_LIST+=("$pkg") ;;
                    esac
                done
                install_packages "${INSTALL_LIST[@]}"
                ;;
            dnf|yum)
                INSTALL_LIST=()
                for pkg in "${REQUIRED_PACKAGES[@]}"; do
                    case $pkg in
                        "gcc-x86-64-linux-gnu") INSTALL_LIST+=("gcc-x86_64-linux-gnu") ;;
                        "binutils-x86-64-linux-gnu") INSTALL_LIST+=("binutils-x86_64-linux-gnu") ;;
                        "grub2-common") INSTALL_LIST+=("grub2-tools") ;;
                        "grub-pc-bin") INSTALL_LIST+=("grub2-pc-modules") ;;
                        "util-linux") ;;  # Fedora 默认已安装
                        *) INSTALL_LIST+=("$pkg") ;;
                    esac
                done
                install_packages "${INSTALL_LIST[@]}"
                ;;
            *)
                print_error "无法自动安装，请手动执行:"
                echo "  sudo apt install ${REQUIRED_PACKAGES[*]}"
                ;;
        esac
    else
        print_warning "跳过必需依赖安装"
    fi
fi

# 询问是否安装可选依赖（Rust）
if [ $TOTAL_OPTIONAL -gt 0 ] && [ "$SKIP_OPTIONAL" = false ]; then
    echo ""
    if ask_yes_no "是否安装 Rust 工具链（可选依赖）？"; then
        install_rust
    else
        print_warning "跳过 Rust 工具链安装"
        echo ""
        echo -e "${BLUE}注意: 如需编译 Rust 模块，可稍后运行:${NC}"
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    fi
fi

# 最终总结
echo ""
echo -e "${CYAN}═════════════════════════════════════════${NC}"
echo -e "${CYAN}           检查完成                       ${NC}"
echo -e "${CYAN}═════════════════════════════════════════${NC}"
echo ""
echo -e "${BLUE}建议操作:${NC}"
echo ""

if [ $TOTAL_REQUIRED -gt 0 ]; then
    echo -e "  ${RED}1. 请先安装所有必需依赖后重试${NC}"
else
    echo -e "  ${GREEN}1. ✓ 必须依赖已满足${NC}"
fi

if [ $TOTAL_OPTIONAL -gt 0 ] && [ "$SKIP_OPTIONAL" = false ]; then
    echo -e "  ${YELLOW}2. 可选安装 Rust 以支持完整构建${NC}"
else
    echo -e "  ${GREEN}2. ✓ 可选依赖已处理${NC}"
fi

echo ""
echo -e "${CYAN}验证安装:${NC}"
echo "  重新运行此脚本以验证所有依赖: $0"
echo ""
echo -e "${CYAN}开始构建:${NC}"
echo "  make all              # 完整编译"
echo "  make test-unit        # 运行测试"
echo "  make run-iso          # 启动 QEMU"
echo ""
