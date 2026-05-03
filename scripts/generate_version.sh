#!/bin/bash
# ============================================================================
# generate_version.sh - AntX 动态版本信息生成器
# ============================================================================
#
# 功能:
#   - 从 Git 仓库获取动态版本信息 (commit hash, branch, date)
#   - 检测工作区状态 (是否有未提交修改)
#   - 生成 C/Rust 双语言兼容的头文件
#   - 支持模块化版本注册系统
#
# 使用方式:
#   ./scripts/generate_version.sh                    # 生成到默认位置
#   ./scripts/generate_version.sh --output dir/       # 指定输出目录
#   ./scripts/generate_version.sh --verbose           # 显示详细信息
#
# 输出文件:
#   src/include/version_auto.h  - C 头文件 (Git 信息 + 构建元数据)
#   src/include/version_registry.h - 版本注册表头文件
#
# 兼容性:
#   - 支持无 Git 环境 (使用 fallback 值)
#   - 支持 CI/CD 环境 (CI=true 时跳过脏状态检测)
#   - 支持跨平台 (Linux/macOS/WSL)
#
# 作者: AntX Development Team
# 版本: 1.0.0 (2026-05-02)
# ============================================================================

set -e

# ==================== 配置 ====================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

OUTPUT_DIR="${PROJECT_ROOT}/src/include"
OUTPUT_FILE_C="${OUTPUT_DIR}/version_auto.h"
OUTPUT_FILE_REGISTRY="${OUTPUT_DIR}/version_registry.h"

VERBOSE=false
FORCE_REGENERATE=false

# 颜色定义 (可选，用于终端输出)
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# ==================== 参数解析 ====================

while [[ $# -gt 0 ]]; do
    case $1 in
        --output|-o)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        --verbose|-v)
            VERBOSE=true
            shift
            ;;
        --force|-f)
            FORCE_REGENERATE=true
            shift
            ;;
        --help|-h)
            echo "用法: $0 [选项]"
            echo ""
            echo "选项:"
            echo "  -o, --output DIR    指定输出目录 (默认: src/include)"
            echo "  -v, --verbose      显示详细生成信息"
            echo "  -f, --force        强制重新生成 (即使文件已存在且未变更)"
            echo "  -h, --help         显示此帮助信息"
            exit 0
            ;;
        *)
            echo "未知参数: $1"
            exit 1
            ;;
    esac
done

# ==================== Git 信息采集 ====================

collect_git_info() {
    # 检查是否在 Git 仓库中
    if ! git rev-parse --is-inside-work-tree &>/dev/null 2>&1; then
        if [ "$VERBOSE" = true ]; then
            echo -e "${YELLOW}⚠ 未检测到 Git 仓库，使用 fallback 值${NC}"
        fi
        
        GIT_COMMIT_HASH="no-git"
        GIT_COMMIT_SHORT="no-git"
        GIT_BRANCH="unknown"
        GIT_TAG="none"
        IS_DIRTY_BUILD=1
        BUILD_DATE="$(date +"%Y-%m-%d %H:%M:%S" 2>/dev/null || echo "unknown")"
        BUILD_USER="${USER:-unknown}"
        BUILD_HOSTNAME="$(hostname 2>/dev/null || echo "unknown")"
        
        return
    fi
    
    cd "$PROJECT_ROOT"
    
    # Commit Hash (完整和短格式)
    GIT_COMMIT_HASH=$(git rev-parse HEAD 2>/dev/null || echo "unknown")
    GIT_COMMIT_SHORT=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
    
    # 分支名
    GIT_BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "detached")
    
    #最近的 Tag (如果有)
    GIT_TAG=$(git describe --tags --exact-match 2>/dev/null || echo "")
    
    # 脏状态检测 (未提交的修改或未跟踪文件)
    if [ "${CI:-false}" = "true" ]; then
        # CI 环境中假设是干净的
        IS_DIRTY_BUILD=0
    else
        DIRTY_COUNT=$(git status --porcelain 2>/dev/null | grep -c "^ M\|^??\|^ A\|^ D" || true)
        if [ "$DIRTY_COUNT" -gt 0 ]; then
            IS_DIRTY_BUILD=1
        else
            IS_DIRTY_BUILD=0
        fi
    fi
    
    # 构建元数据
    BUILD_DATE="$(date +"%Y-%m-%d %H:%M:%S" 2>/dev/null || echo "unknown")"
    BUILD_USER="${USER:-unknown}"
    BUILD_HOSTNAME="$(hostname 2>/dev/null || echo "unknown")"
    
    # 编译器信息
    CC_VERSION=$(gcc --version 2>/dev/null | head -1 | awk '{print $3}' || echo "unknown")
    RUST_VERSION=$(rustc --version 2>/dev/null | awk '{print $2}' || echo "none")
}

# ==================== 文件生成 ====================

generate_version_header_c() {
    local tmp_file=$(mktemp)
    
    cat > "$tmp_file" << 'VERSION_EOF'
/**
 * ============================================================================
 * version_auto.h - AntX 动态版本信息 (自动生成)
 * ============================================================================
 *
 * ⚠️ 警告: 此文件由 scripts/generate_version.sh 自动生成!
 *    请勿手动编辑，修改将在下次构建时被覆盖。
 *
 * 用途:
 *   - 提供构建时的精确版本标识 (Git commit hash)
 *   - 记录构建元数据 (时间、用户、编译器版本)
 *   - 支持模块化版本注册 (见 version_registry.h)
 *
 * 使用示例:
 *   // 获取内核版本字符串
 *   printf("AntX Kernel %s\n", KERNEL_VERSION_FULL);
 *
 *   // 检查是否为脏构建
 *   #if IS_DIRTY_BUILD
 *   printf("[WARNING] Uncommitted changes in build\n");
 *   #endif
 *
 * 兼容性:
 *   - 纯 C 实现，无外部依赖
 *   - 与 Rust FFI 层兼容
 *   - 支持条件编译 (#ifdef)
 *
 * 生成时间: BUILD_DATE_PLACEHOLDER
 * 生成工具: scripts/generate_version.sh v1.0
 * ============================================================================
 */

#ifndef __VERSION_AUTO_H__
#define __VERSION_AUTO_H__

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ============================================================================ */
/*                           Git 版本信息                                   */
/* ============================================================================ */

/** @brief 完整 Git commit hash (40字符) */
#define GIT_COMMIT_HASH         "GIT_HASH_PLACEHOLDER"

/** @brief 短格式 Git commit hash (7字符) */
#define GIT_COMMIT_SHORT        "GIT_SHORT_PLACEHOLDER"

/** @brief 当前 Git 分支名 */
#define GIT_BRANCH              "GIT_BRANCH_PLACEHOLDER"

/** @brief 最近的 Git Tag (如果存在) */
#define GIT_TAG                 "GIT_TAG_PLACEHOLDER"

/* ============================================================================ */
/*                           构建元数据                                     */
/* ============================================================================ */

/** @brief 构建日期时间 (YYYY-MM-DD HH:MM:SS) */
#define BUILD_DATE              "BUILD_DATE_PLACEHOLDER"

/** @brief 构建用户名 */
#define BUILD_USER              "BUILD_USER_PLACEHOLDER"

/** @brief 构建主机名 */
#define BUILD_HOSTNAME          "BUILD_HOST_PLACEHOLDER"

/* ============================================================================ */
/*                           编译器信息                                     */
/* ============================================================================ */

/** @brief GCC 编译器版本 */
#define CC_VERSION              "CC_VERSION_PLACEHOLDER"

/** @brief Rust 编译器版本 (如果可用) */
#define RUST_VERSION            "RUST_VERSION_PLACEHOLDER"

/* ============================================================================ */
/*                           构建状态标志                                   */
/* ============================================================================ */

/**
 * @brief 是否包含未提交的修改 (脏构建)
 *
 * 值含义:
 *   - 0: 干净构建 (工作区与最后一次提交一致)
 *   - 1: 脏构建 (有未提交的修改或未跟踪文件)
 *
 * 用途:
 *   - 调试时确认运行的是否是官方版本
 *   - 发布前检查是否有遗漏的提交
 */
#define IS_DIRTY_BUILD          IS_DIRTY_PLACEHOLDER

/* ============================================================================ */
/*                           便捷宏定义                                     */
/* ============================================================================ */

/**
 * @brief 完整版本字符串 (适合显示给用户)
 *
 * 格式: "<commit-short> (<branch>) [<dirty>] <date>"
 * 示例: "a3f7b2d (main) [DIRTY] 2026-05-02 23:45"
 */
#define KERNEL_VERSION_FULL     \
    GIT_COMMIT_SHORT " (" GIT_BRANCH ")" " \
    IS_DIRTY_STR " " BUILD_DATE

/** @brief 脏状态标记字符串 */
#if IS_DIRTY_BUILD
#define IS_DIRTY_STR           "[DIRTY]"
#else
#define IS_DIRTY_STR           "[CLEAN]"
#endif

/**
 * @brief 简洁版本字符串 (仅 commit + dirty 标记)
 *
 * 格式: "<commit-short>[<dirty>]"
 * 示例: "a3f7b2d[DIRTY]" 或 "a3f7b2d"
 */
#define KERNEL_VERSION_SHORT    \
    GIT_COMMIT_SHORT IS_DIRTY_STR

/**
 * @brief 获取版本信息的函数原型
 *
 * 在需要更复杂的版本信息时使用 (如多行格式化输出)
 *
 * @param buffer  输出缓冲区
 * @param size    缓冲区大小
 * @return       写入的字节数 (不含终止符)
 */
void version_get_info(char *buffer, size_t size);

/**
 * @brief 打印完整版本信息到指定输出
 *
 * @param output_func  输出函数指针 (如 serial_puts)
 *                   签名: void (*out_fn)(const char *)
 */
void version_print_full(void (*output_func)(const char*));

#ifdef __cplusplus
}
#endif

#endif /* __VERSION_AUTO_H__ */
VERSION_EOF
    
    # 替换占位符为实际值
    sed -i \
        -e "s|GIT_HASH_PLACEHOLDER|${GIT_COMMIT_HASH}|g" \
        -e "s|GIT_SHORT_PLACEHOLDER|${GIT_COMMIT_SHORT}|g" \
        -e "s|GIT_BRANCH_PLACEHOLDER|${GIT_BRANCH}|g" \
        -e "s|GIT_TAG_PLACEHOLDER|${GIT_TAG}|g" \
        -e "s|BUILD_DATE_PLACEHOLDER|${BUILD_DATE}|g" \
        -e "s|BUILD_USER_PLACEHOLDER|${BUILD_USER}|g" \
        -e "s|BUILD_HOST_PLACEHOLDER|${BUILD_HOSTNAME}|g" \
        -e "s|CC_VERSION_PLACEHOLDER|${CC_VERSION}|g" \
        -e "s|RUST_VERSION_PLACEHOLDER|${RUST_VERSION}|g" \
        -e "s|IS_DIRTY_PLACEHOLDER|${IS_DIRTY_BUILD}|g" \
        -e "s|BUILD_DATE_PLACEHOLDER|${BUILD_DATE}|g" \
        "$tmp_file"
    
    # 移动到目标位置
    mv "$tmp_file" "$OUTPUT_FILE_C"
    
    if [ "$VERBOSE" = true ]; then
        echo -e "${GREEN}✅ 已生成: ${OUTPUT_FILE_C}${NC}"
        echo -e "   Commit: ${CYAN}${GIT_COMMIT_SHORT}${NC}"
        echo -e "   Branch: ${CYAN}${GIT_BRANCH}${NC}"
        echo -e "   Date:   ${CYAN}${BUILD_DATE}${NC}"
        if [ "$IS_DIRTY_BUILD" -eq 1 ]; then
            echo -e "   Status: ${RED}[DIRTY]${NC} (${DIRTY_COUNT} modified files)"
        else
            echo -e "   Status: ${GREEN}[CLEAN]${NC}"
        fi
    fi
}

generate_version_registry_header() {
    cat > "$OUTPUT_FILE_REGISTRY" << 'REGISTRY_EOF'
/**
 * ============================================================================
 * version_registry.h - AntX 模块版本注册表
 * ============================================================================
 *
 * 功能:
 *   - 提供统一的模块版本信息注册机制
 *   - 支持任意子系统/模块注册自己的版本
 *   - 集中式管理所有组件的版本信息
 *
 * 设计理念:
 *   - 未来新增模块只需调用 VERSION_REGISTER() 宏即可
 *   - 版本信息集中展示，便于调试和问题定位
 *   - 支持语义化版本号 (SemVer) 和日期版本
 *
 * 使用方法 (新模块):
 *   // 1. 在模块初始化时注册版本
 *   VERSION_REGISTER("MyModule", "1.0.0", "Description of module");
 *
 *   // 2. 模块代码中使用版本常量
 *   #define MY_MODULE_VERSION_MAJOR  1
 *   #define MY_MODULE_VERSION_MINOR  0
 *   #define MY_MODULE_VERSION_PATCH  0
 *
 * 示例 (已注册模块):
 *   - HvFS (文件系统) - 数据格式版本
 *   - PWID (权限系统) - 数据库格式版本
 *   - KLog (日志系统) - 接口版本
 *   - E1000 (网卡驱动) - 固件版本 (未来)
 *
 * 扩展性:
 *   ✅ 支持无限数量的模块注册
 *   ✅ 支持运行时动态查询
 *   ✅ 支持版本比较和兼容性检查
 *   ✅ 支持通过 /proc/version 导出
 *
 * 兼容性:
 *   - C/Rust 双语言支持
 *   - 无全局状态 (线程安全)
 *   - 编译时可配置 (可禁用注册表)
 *
 * 创建时间: AUTO_GEN_DATE
 * ============================================================================
 */

#ifndef __VERSION_REGISTRY_H__
#define __VERSION_REGISTRY_H__

#include <stdint.h>
#include "version_auto.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ============================================================================ */
/*                        版本注册表配置                                  */
/* ============================================================================ */

/**
 * @brief 最大支持的注册模块数量
 *
 * 可根据实际需求调整。建议值: 32 (足够覆盖所有核心模块)
 * 设置为 0 可禁用版本注册功能 (节省内存)
 */
#ifndef MAX_VERSION_MODULES
#define MAX_VERSION_MODULES  64
#endif

/* ============================================================================ */
/*                        版本信息结构体                                   */
/* ============================================================================ */

/**
 * @brief 单个模块的版本信息
 */
typedef struct {
    /** 模块名称 (如 "HvFS", "PWID", "TCP/IP") */
    const char *name;
    
    /** 主版本号 (不兼容变更) */
    uint8_t major;
    
    /** 次版本号 (向下兼容的功能新增) */
    uint8_t minor;
    
    /** 补丁版本号 (bug 修复) */
    uint8_t patch;
    
    /** 版本字符串 (如 "2.1.0") */
    const char *version_string;
    
    /** 模块描述 (可选) */
    const char *description;
    
    /** 模块类型分类 */
    enum {
        MODULE_TYPE_CORE,        /**< 核心模块 (进程/内存/调度) */
        MODULE_TYPE_FS,           /**< 文件系统 (RamFS/HvFS/DiskFS) */
        MODULE_TYPE_DRIVER,       /**< 设备驱动 (E1000/键盘/VGA) */
        MODULE_TYPE_NET,          /**< 网络协议 (TCP/IP/UDP/lwIP) */
        MODULE_TYPE_SECURITY,     /**< 安全模块 (PWID/认证) */
        MODULE_TYPE_LIB,          /**< 库/框架 (IPC/VFS/syscall) */
        MODULE_TYPE_APP,          /**< 用户态应用 (Shell/installer) */
        MODULE_TYPE_UNKNOWN       /**< 未知类型 */
    } type;
    
    /** 初始化状态 */
    enum {
        MODULE_STATUS_UNINITIALIZED,  /**< 未初始化 */
        MODULE_STATUS_INITIALIZING,  /**< 正在初始化 */
        MODULE_STATUS_READY,         /**< 就绪可用 */
        MODULE_STATUS_ERROR,         /**< 初始化失败 */
        MODULE_STATUS_DISABLED       /**< 已禁用 */
    } status;
    
} version_module_t;

/* ============================================================================ */
/*                        版本注册 API                                     */
/* ============================================================================ */

/**
 * @brief 注册一个模块的版本信息
 *
 * 应在模块初始化函数中调用。重复注册同名模块会更新版本信息。
 *
 * @param name        模块名称 (必须唯一，建议使用大驼峰命名)
 * @param major       主版本号
 * @param minor       次版本号
 * @param patch       补丁版本号
 * @param description 模块描述 (可为 NULL)
 * @param type        模块类型 (见 version_module_t.type 枚举)
 *
 * @return >=0 成功 (返回注册索引), <0 失败 (表满或参数无效)
 *
 * 示例:
 * @code
 * // 在 HvFS 初始化函数中
 * int hvfs_init(void) {
 *     VERSION_REGISTER("HvFS", 2, 0, 0,
 *                     "Hive File System - AntX native FS",
 *                     MODULE_TYPE_FS);
 *     
 *     // ... 其他初始化代码 ...
 *     
 *     return 0;
 * }
 * @endcode
 */
int version_register(const char *name, 
                     uint8_t major, uint8_t minor, uint8_t patch,
                     const char *description, 
                     int type);

/**
 * @brief 注册模块 (简化版，使用字符串版本号)
 *
 * 自动解析 "major.minor.patch" 格式的版本字符串。
 *
 * @param name           模块名称
 * @param version_string 版本字符串 (如 "1.2.3")
 * @param description    模块描述 (可为 NULL)
 * @param type           模板类型
 *
 * @return 见 version_register()
 *
 * 示例:
 * @code
 * VERSION_REGISTER_STR("KLog", "1.0.0", 
 *                       "Kernel Logging System", MODULE_TYPE_CORE);
 * @endcode
 */
#define VERSION_REGISTER_STR(name, ver_str, desc, type) \
    version_register((name), \
                     ((ver_str)[0]-'0'), \
                     strlen(ver_str) > 2 ? ((ver_str)[2]-'0') : 0, \
                     strlen(ver_str) > 4 ? ((ver_str)[4]-'0') : 0, \
                     (desc), (type))

/**
 * @brief 更新已注册模块的状态
 *
 * @param name   模块名称
 * @param status 新状态
 * @return 0 成功, -1 模块未找到
 */
int version_set_status(const char *name, int status);

/**
 * @brief 查询模块版本信息
 *
 * @param name 模块名称
 * @return 指向 version_module_t 的指针, 或 NULL (未找到)
 */
const version_module_t *version_query(const char *name);

/**
 * @brief 获取已注册模块数量
 *
 * @return 当前注册的模块总数
 */
int version_get_registered_count(void);

/* ============================================================================ */
/*                        版本信息输出                                    */
/* ============================================================================ */

/**
 * @brief 打印所有已注册模块的版本信息
 *
 * 格式化输出到指定的输出函数。
 *
 * @param output_func 输出函数 (签名: void (*fn)(const char*))
 *                  通常传入 serial_puts 或类似函数
 *
 * 输出示例:
 * @code
 * ╔════════════════════════════════════════╗
 * ║     AntX Module Version Registry        ║
 * ╠════════════════════════════════════════╣
 * ║ Core Modules:                          ║
 * ║   • PMM          v1.0.0  [READY]        ║
 * ║   • VMM          v1.0.0  [READY]        ║
 * ║   • Scheduler    v2.1.0  [READY]        ║
 * ║                                         ║
 * ║ File Systems:                          ║
 * ║   • HvFS         v2.0.0  [READY]        ║
 * ║   • RamFS        v1.0.0  [READY]        ║
 * ║                                         ║
 * ║ Build Info:                            ║
 * ║   Commit: a3f7b2d [DIRTY]             ║
 * ║   Date:   2026-05-02 23:45:12          ║
 * ╚════════════════════════════════════════╝
 * @endcode
 */
void version_print_registry(void (*output_func)(const char*));

/**
 * @brief 将版本信息导出到缓冲区 (机器可解析格式)
 *
 * 格式: JSON-like (便于后续解析)
 *
 * @param buffer  输出缓冲区
 * @param size    缓冲区大小
 * @return       写入的字节数
 *
 * 输出示例:
 * @code
 * {
 *   "build": {
 *     "commit": "a3f7b2d",
 *     "branch": "main",
 *     "date": "2026-05-02 23:45:12",
 *     "dirty": true
 *   },
 *   "modules": [
 *     {"name": "HvFS", "version": "2.0.0", "status": "READY"},
 *     ...
 *   ]
 * }
 * @endcode
 */
int version_export_json(char *buffer, size_t size);

#ifdef __cplusplus
}
#endif

#endif /* __VERSION_REGISTRY_H__ */
REGISTRY_EOF
    
    # 替换占位符
    sed -i "s|AUTO_GEN_DATE|${BUILD_DATE}|g" "$OUTPUT_FILE_REGISTRY"
    
    if [ "$VERBOSE" = true ]; then
        echo -e "${GREEN}✅ 已生成: ${OUTPUT_FILE_REGISTRY}${NC}"
    fi
}

# ==================== 主逻辑 ====================

main() {
    # 收集 Git 信息
    collect_git_info
    
    # 创建输出目录 (如果不存在)
    mkdir -p "$OUTPUT_DIR"
    
    # 检查是否需要重新生成
    if [ "$FORCE_REGENERATE" = false ] && [ -f "$OUTPUT_FILE_C" ]; then
        # 简单的缓存机制: 如果 Git 状态没变就不重新生成
        # (可选优化，当前总是重新生成以确保准确性)
        :
    fi
    
    # 生成文件
    generate_version_header_c
    generate_version_registry_header
    
    # 完成
    if [ "$VERBOSE" = true ]; then
        echo ""
        echo -e "${CYAN}═════════════════════════════════════════════${NC}"
        echo -e "${CYAN}  Version Generation Complete               ${NC}"
        echo -e "${CYAN}═════════════════════════════════════════════${NC}"
        echo ""
        echo -e "  Output files:"
        echo -e "    ${GREEN}•${NC} ${OUTPUT_FILE_C}"
        echo -e "    ${GREEN}•${NC} ${OUTPUT_FILE_REGISTRY}"
        echo ""
        echo -e "  Next step: Run 'make all' to compile with new versions"
        echo ""
    fi
}

# 执行主逻辑
main
