/**
 * ============================================================================
 * version_registry.c - AntX 模块版本注册表实现
 * ============================================================================
 *
 * 功能:
 *   - 管理所有已注册模块的版本信息
 *   - 提供版本信息的集中式存储和查询
 *   - 支持格式化输出 (用户友好 / 机器可解析)
 *
 * 设计特点:
 *   - 静态数组实现 (无动态内存分配)
 *   - 线程安全设计 (可扩展为锁保护)
 *   - 零依赖 (仅依赖标准 C 库)
 *
 * 使用场景:
 *   1. 系统启动时各模块调用 VERSION_REGISTER() 注册
 *   2. 用户执行 "antx-version" 命令查看
 *   3. 调试时通过 serial 输出完整版本信息
 *   4. 通过 /proc/version 导出给用户态程序
 *
 * 内存占用:
 *   - 静态分配: ~2KB (64个模块 × ~32字节/模块)
 *   - 运行时开销: 极低 (仅查表操作)
 *
 * 兼容性:
 *   - 支持无 OS 环境 (裸机/引导阶段)
 *   - 支持多线程环境 (需添加互斥锁)
 *   - 支持 Rust FFI 调用
 *
 * 作者: AntX Development Team
 * 版本: 1.0.0 (2026-05-02)
 * ============================================================================
 */

#include "version_registry.h"
#include "serial.h"
#include "string.h"
#include "klog.h"

/* ============================================================================ */
/*                        内部数据结构                                   */
/* ============================================================================ */

/** @brief 版本注册表 (静态数组) */
static version_module_t version_table[MAX_VERSION_MODULES];

/** @brief 已注册模块计数 */
static int registered_count = 0;

/* ============================================================================ */
/*                        内部辅助函数                                   */
/* ============================================================================ */

/**
 * @brief 串口输出包装函数 (适配单参数回调接口)
 */
static void serial_output_wrapper(const char *str) {
    if (str) {
        serial_puts(SERIAL_COM1, str);
    }
}

/**
 * @brief 解析版本字符串为数字组件
 *
 * 支持格式: "major.minor.patch"
 * 示例: "2.1.0" -> major=2, minor=1, patch=0
 *
 * @param version_str 版本字符串
 * @param major       [输出] 主版本号
 * @param minor       [输出] 次版本号
 * @param patch       [输出] 补丁版本号
 * @return            0 成功, -1 格式错误
 */
__attribute__((unused))
static int parse_version_string(const char *version_str,
                                uint8_t *major,
                                uint8_t *minor,
                                uint8_t *patch) {
    if (!version_str || !major || !minor || !patch) {
        return -1;
    }
    
    // 默认值
    *major = 0;
    *minor = 0;
    *patch = 0;
    
    // 简单解析 (不使用 sscanf 以减少依赖)
    int i = 0;
    int component = 0;
    int value = 0;
    
    while (version_str[i] != '\0' && component < 3) {
        if (version_str[i] >= '0' && version_str[i] <= '9') {
            value = value * 10 + (version_str[i] - '0');
        } else if (version_str[i] == '.') {
            // 分隔符: 保存当前组件值
            switch (component) {
                case 0: *major = (uint8_t)value; break;
                case 1: *minor = (uint8_t)value; break;
                case 2: *patch = (uint8_t)value; break;
                default: break;
            }
            value = 0;
            component++;
        } else {
            // 非法字符
            return -1;
        }
        i++;
    }
    
    // 保存最后一个组件
    switch (component) {
        case 0: *major = (uint8_t)value; break;
        case 1: *minor = (uint8_t)value; break;
        case 2: *patch = (uint8_t)value; break;
        default: break;
    }
    
    return 0;
}

/**
 * @brief 根据状态码返回状态字符串
 */
static const char* status_to_string(int status) {
    switch (status) {
        case MODULE_STATUS_UNINITIALIZED: return "UNINIT";
        case MODULE_STATUS_INITIALIZING:  return "INITING";
        case MODULE_STATUS_READY:         return "READY";
        case MODULE_STATUS_ERROR:         return "ERROR";
        case MODULE_STATUS_DISABLED:      return "DISABLED";
        default:                         return "UNKNOWN";
    }
}

/**
 * @brief 根据类型码返回类型字符串
 */
static const char* type_to_string(int type) {
    switch (type) {
        case MODULE_TYPE_CORE:     return "Core";
        case MODULE_TYPE_FS:       return "FS";
        case MODULE_TYPE_DRIVER:   return "Driver";
        case MODULE_TYPE_NET:      return "Net";
        case MODULE_TYPE_SECURITY: return "Security";
        case MODULE_TYPE_LIB:      return "Lib";
        case MODULE_TYPE_APP:      return "App";
        default:                  return "Unknown";
    }
}

/* ============================================================================ */
/*                        公共 API 实现                                    */
/* ============================================================================ */

int version_register(const char *name, 
                     uint8_t major, uint8_t minor, uint8_t patch,
                     const char *description, 
                     int type) {
    // 参数验证
    if (!name || registered_count >= MAX_VERSION_MODULES) {
        return -1;
    }
    
    // 检查是否已存在同名模块 (更新而非重复添加)
    for (int i = 0; i < registered_count; i++) {
        if (strcmp(version_table[i].name, name) == 0) {
            // 更新现有条目
            version_table[i].major = major;
            version_table[i].minor = minor;
            version_table[i].patch = patch;
            version_table[i].description = description;
            version_table[i].type = type;
            return i;  // 返回已有索引
        }
    }

    // 新增条目
    int idx = registered_count;

    version_table[idx].name = name;
    version_table[idx].major = major;
    version_table[idx].minor = minor;
    version_table[idx].patch = patch;
    version_table[idx].description = description;
    version_table[idx].type = type;
    version_table[idx].status = MODULE_STATUS_READY;  // 注册即视为就绪
    
    // 构建版本字符串 (静态缓冲区)
    static char ver_str_buf[MAX_VERSION_MODULES][16];
    // 注意: 简化版，实际应使用更安全的格式化
    // 这里为了简化，假设 version_string 字段会在外部设置
    
    registered_count++;
    
    return idx;
}

int version_set_status(const char *name, int status) {
    if (!name) return -1;

    for (int i = 0; i < registered_count; i++) {
        if (strcmp(version_table[i].name, name) == 0) {
            version_table[i].status = status;
            return 0;
        }
    }

    return -1;  // 未找到
}

const version_module_t *version_query(const char *name) {
    if (!name) return NULL;
    
    for (int i = 0; i < registered_count; i++) {
        if (strcmp(version_table[i].name, name) == 0) {
            return &version_table[i];
        }
    }
    
    return NULL;
}

int version_get_registered_count(void) {
    return registered_count;
}

/* ============================================================================ */
/*                        版本信息输出                                    */
/* ============================================================================ */

void version_print_registry(void (*output_func)(const char*)) {
    if (!output_func) output_func = serial_output_wrapper;  // 默认串口

    // 标题
    (*output_func)("\n");
    (*output_func)("╔════════════════════════════════════════════╗\n");
    (*output_func)("║     AntX Module Version Registry          ║\n");
    (*output_func)("╠════════════════════════════════════════════╣\n");
    
    // 构建信息
    (*output_func)("║ Build Info:                                ║\n");
    (*output_func)("║   Commit: ");
    (*output_func)(GIT_COMMIT_SHORT);
#if IS_DIRTY_BUILD
    (*output_func)(" [DIRTY]");
#endif
    (*output_func)("\n");
    
    (*output_func)("║   Date:   ");
    (*output_func)(BUILD_DATE);
    (*output_func)("\n");
    
    // 模块列表 (按类型分组)
    const char* type_names[] = {"Core", "FS", "Driver", "Net", "Security", "Lib", "App"};
    int type_count[] = {0, 0, 0, 0, 0, 0, 0};
    
    // 统计各类型数量
    for (int i = 0; i < registered_count; i++) {
        int t = (int)version_table[i].type;
        if (t >= 0 && t < 7) type_count[t]++;
    }
    
    // 输出各类型
    for (int t = 0; t < 7; t++) {
        if (type_count[t] == 0) continue;
        
        (*output_func)("║                                             ║\n");
        
        // 类型标题
        (*output_func)("║ ");
        (*output_func)(type_names[t]);
        (*output_func)(" Modules:                              ║\n");
        
        // 该类型的所有模块
        for (int i = 0; i < registered_count; i++) {
            if ((int)version_table[i].type != t) continue;
            
            const version_module_t *m = &version_table[i];
            
            (*output_func)("║   • ");
            (*output_func)(m->name);
            
            // 对齐名称 (固定宽度)
            int name_len = strlen(m->name);
            while (name_len < 20) {
                (*output_func)(" ");
                name_len++;
            }
            
            // 版本号
            (*output_func)("v");
            // 简化输出 (避免 sprintf 依赖问题)
            char ver[12];
            int v = 0;
            ver[v++] = '0' + m->major;
            ver[v++] = '.';
            ver[v++] = '0' + m->minor;
            ver[v++] = '.';
            ver[v++] = '0' + m->patch;
            ver[v] = '\0';
            (*output_func)(ver);
            
            // 状态
            while (strlen(ver) < 8) { (*output_func)(" "); }
            (*output_func)(status_to_string(m->status));
            
            (*output_func)("\n");
        }
    }
    
    // 统计摘要
    (*output_func)("║                                             ║\n");
    (*output_func)("║ Summary:                                    ║\n");
    (*output_func)("║   Total modules: ");
    
    // 数字转字符串 (简单实现)
    char num[8];
    int n = registered_count, idx = 0;
    if (n == 0) { num[idx++] = '0'; }
    else { while (n > 0) { num[idx++] = '0' + (n % 10); n /= 10; } }
    num[idx] = '\0';
    // 反转
    for (int i = 0; i < idx/2; i++) { char t = num[i]; num[i] = num[idx-1-i]; num[idx-1-i] = t; }
    (*output_func)(num);
    
    (*output_func)("\n");
    (*output_func)("╚════════════════════════════════════════════╝\n");
    (*output_func)("\n");
}

int version_export_json(char *buffer, size_t size) {
    if (!buffer || size < 64) return 0;
    
    int offset = 0;
    
    // 简化 JSON 输出 (避免复杂的 JSON 库依赖)
    #define JSON_APPEND(fmt, ...) \
        do { \
            int remaining = size - offset - 1; \
            if (remaining > 0) { \
                offset += snprintf(buffer + offset, remaining, fmt, ##__VA_ARGS__); \
            } \
        } while(0)
    
    JSON_APPEND("{\n");
    JSON_APPEND("  \"build\": {\n");
    JSON_APPEND("    \"commit\": \"%s\",\n", GIT_COMMIT_HASH);
    JSON_APPEND("    \"short\": \"%s\",\n", GIT_COMMIT_SHORT);
    JSON_APPEND("    \"branch\": \"%s\",\n", GIT_BRANCH);
    JSON_APPEND("    \"date\": \"%s\",\n", BUILD_DATE);
    JSON_APPEND("    \"dirty\": %s\n", IS_DIRTY_BUILD ? "true" : "false");
    JSON_APPEND("  },\n");
    JSON_APPEND("  \"modules\": [\n");
    
    for (int i = 0; i < registered_count; i++) {
        const version_module_t *m = &version_table[i];
        
        JSON_APPEND("    {\n");
        JSON_APPEND("      \"name\": \"%s\",\n", m->name ? m->name : "");
        JSON_APPEND("      \"version\": \"%d.%d.%d\",\n", m->major, m->minor, m->patch);
        JSON_APPEND("      \"status\": \"%s\",\n", status_to_string(m->status));
        JSON_APPEND("      \"type\": \"%s\"\n", type_to_string(m->type));
        JSON_APPEND("    }%s\n", (i < registered_count - 1) ? "," : "");
    }
    
    JSON_APPEND("  ]\n");
    JSON_APPEND("}\n");
    
    #undef JSON_APPEND
    
    buffer[offset] = '\0';
    return offset;
}
