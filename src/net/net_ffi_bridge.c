/* ============================================================
 * net_ffi_bridge.c — 网络子系统 FFI 桥接
 * 
 * 提供 Rust FFI 需要的真实函数 (替代 C 宏)
 * ============================================================ */

#include <stdarg.h>

/* 声明 klog_vwrite 函数 */
extern int klog_vwrite(int level, int category, 
                       const char *file, const char *func, int line,
                       const char *fmt, va_list args);

/* 日志常量 */
#define LOG_NET     0x0100
#define LOG_INIT    0x0020
#define LOG_ERROR   0x0001

/// 网络信息日志 (替代 klog_net 宏)
void klog_net(const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    klog_vwrite(LOG_NET, 0, __FILE__, __func__, __LINE__, fmt, args);
    va_end(args);
}

/// 网络错误日志 (替代 klog_net_err 宏)
void klog_net_err(const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    klog_vwrite(LOG_ERROR | LOG_NET, 0, __FILE__, __func__, __LINE__, fmt, args);
    va_end(args);
}

/// 初始化消息日志 (替代 klog_init_msg 宏)
void klog_init_msg(const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    klog_vwrite(LOG_INIT, 0, __FILE__, __func__, __LINE__, fmt, args);
    va_end(args);
}
