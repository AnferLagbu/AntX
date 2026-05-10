/* ============================================================
 * lib_compat.c — 标准库兼容函数
 * 
 * 提供 lwIP 需要的标准库函数存根
 * (在裸机环境中没有 libc)
 * ============================================================ */

/// 字符串转整数 (简化版 atoi)
int atoi(const char *str) {
    int result = 0;
    int sign = 1;
    
    if (!str) return 0;
    
    // 跳过空白字符
    while (*str == ' ' || *str == '\t' || *str == '\n') str++;
    
    // 处理符号
    if (*str == '-') {
        sign = -1;
        str++;
    } else if (*str == '+') {
        str++;
    }
    
    // 转换数字
    while (*str >= '0' && *str <= '9') {
        result = result * 10 + (*str - '0');
        str++;
    }
    
    return sign * result;
}

/// 字符串转长整数 (简化版 strtol)
long strtol(const char *str, char **endptr, int base) {
    long result = 0;
    int sign = 1;
    
    if (!str) {
        if (endptr) *endptr = (char *)str;
        return 0;
    }
    
    // 跳过空白
    while (*str == ' ' || *str == '\t' || *str == '\n') str++;
    
    // 处理符号
    if (*str == '-') {
        sign = -1;
        str++;
    } else if (*str == '+') {
        str++;
    }
    
    // 自动检测base
    if (base == 0) {
        if (*str == '0') {
            if (*(str+1) == 'x' || *(str+1) == 'X') {
                base = 16;
                str += 2;
            } else {
                base = 8;
                str++;
            }
        } else {
            base = 10;
        }
    }
    
    // 转换数字
    while (*str) {
        int digit;
        if (*str >= '0' && *str <= '9') {
            digit = *str - '0';
        } else if (*str >= 'a' && *str <= 'f') {
            digit = *str - 'a' + 10;
        } else if (*str >= 'A' && *str <= 'F') {
            digit = *str - 'A' + 10;
        } else {
            break;  // 无效字符
        }
        
        if (digit >= base) break;
        
        result = result * base + digit;
        str++;
    }
    
    if (endptr) *endptr = (char *)str;
    
    return sign * result;
}
