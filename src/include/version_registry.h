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
 * 创建时间: 2026-05-05 23:06:12
 * ============================================================================
 */

#ifndef __VERSION_REGISTRY_H__
#define __VERSION_REGISTRY_H__

#include "types.h"  /* 内核类型定义 */
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
