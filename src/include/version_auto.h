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
 * 生成时间: 2026-05-11 22:28:31
 * 生成工具: scripts/generate_version.sh v1.0
 * ============================================================================
 */

#ifndef __VERSION_AUTO_H__
#define __VERSION_AUTO_H__

#include "user/user.h"  /* 用户态类型定义 (uint8_t, uint32_t 等) */

#ifdef __cplusplus
extern "C" {
#endif

/* ============================================================================ */
/*                           Git 版本信息                                   */
/* ============================================================================ */

/** @brief 完整 Git commit hash (40字符) */
#define GIT_COMMIT_HASH         "ed87d84040f21d582b9967187995c07f49fc0aff"

/** @brief 短格式 Git commit hash (7字符) */
#define GIT_COMMIT_SHORT        "ed87d84"

/** @brief 当前 Git 分支名 */
#define GIT_BRANCH              "main"

/** @brief 最近的 Git Tag (如果存在) */
#define GIT_TAG                 ""

/* ============================================================================ */
/*                           构建元数据                                     */
/* ============================================================================ */

/** @brief 构建日期时间 (YYYY-MM-DD HH:MM:SS) */
#define BUILD_DATE              "2026-05-11 22:28:31"

/** @brief 构建用户名 */
#define BUILD_USER              "anfer"

/** @brief 构建主机名 */
#define BUILD_HOSTNAME          "BIXI"

/* ============================================================================ */
/*                           编译器信息                                     */
/* ============================================================================ */

/** @brief GCC 编译器版本 */
#define CC_VERSION              "16.1.1"

/** @brief Rust 编译器版本 (如果可用) */
#define RUST_VERSION            "1.97.0-nightly"

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
#define IS_DIRTY_BUILD          1

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
