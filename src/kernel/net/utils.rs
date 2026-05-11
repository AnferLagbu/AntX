/// 网络工具函数模块 (Network Utilities)
///
/// 提供标准库兼容函数和网络工具函数，替代原来的 lib_compat.c。
///
/// ## 功能清单
///
/// 1. **标准库兼容** - atoi, strtol 等（供 lwIP 使用）
/// 2. **校验和计算** - Internet Checksum（IP/TCP/UDP）
/// 3. **字节序转换** - hton/ntoh 系列
/// 4. **MAC/IP 地址工具** - 地址格式化、解析
/// 5. **内存操作** - 安全的内存拷贝、比较
///
/// ## 架构设计
///
/// ```text
/// Network Utils Module
/// ├── lib_compat/    # 标准库兼容层 (替代 C 版 lib_compat.c)
/// │   ├── atoi       # 字符串转整数
/// │   └── strtol     # 字符串转长整数
/// ├── checksum/      # 校验和计算
/// │   └── inet_chksum # Internet Checksum
/// ├── byteorder/     # 字节序转换
/// │   ├── htons      # 主机到网络短整数
/// │   └── ntohs      # 网络到主机短整数
/// └── address/       # 地址工具
///     ├── mac_fmt    # MAC 地址格式化
///     └── ip_fmt     # IP 地址格式化
/// ```

// ============================================================================
// 标准库兼容函数 (替代 lib_compat.c)
// ============================================================================

/// 字符串转整数 (简化版 atoi)
///
/// # Safety
/// 此函数通过 FFI 暴露给 C 代码使用
#[no_mangle]
pub unsafe extern "C" fn atoi(str: *const i8) -> i32 {
    if str.is_null() {
        return 0;
    }

    let mut result: i32 = 0;
    let mut sign: i32 = 1;
    let mut ptr = str;

    // 跳过空白字符
    loop {
        let ch = *ptr;
        if ch != b' ' as i8 && ch != b'\t' as i8 && ch != b'\n' as i8 {
            break;
        }
        ptr = ptr.add(1);
    }

    // 处理符号
    if *ptr == b'-' as i8 {
        sign = -1;
        ptr = ptr.add(1);
    } else if *ptr == b'+' as i8 {
        ptr = ptr.add(1);
    }

    // 转换数字
    loop {
        let ch = *ptr;
        if ch >= b'0' as i8 && ch <= b'9' as i8 {
            result = result.wrapping_mul(10).wrapping_add((ch - b'0' as i8) as i32);
            ptr = ptr.add(1);
        } else {
            break;
        }
    }

    sign * result
}

/// 字符串转长整数 (简化版 strtol)
///
/// # Arguments
/// * `str` - 输入字符串
/// * `endptr` - 输出: 指向转换结束位置的指针（可选）
/// * `base` - 进制 (0=自动检测, 8, 10, 16)
///
/// # Returns
/// 转换后的长整数值
///
/// # Safety
/// 此函数通过 FFI 暴露给 C 代码使用
#[no_mangle]
pub unsafe extern "C" fn strtol(
    str: *const i8,
    endptr: *mut *mut i8,
    base: i32,
) -> i64 {
    if str.is_null() {
        if !endptr.is_null() {
            *endptr = str as *mut i8;
        }
        return 0;
    }

    let mut result: i64 = 0;
    let mut sign: i64 = 1;
    let mut ptr = str;
    let mut actual_base = base;

    // 跳过空白
    loop {
        let ch = *ptr;
        if ch != b' ' as i8 && ch != b'\t' as i8 && ch != b'\n' as i8 {
            break;
        }
        ptr = ptr.add(1);
    }

    // 处理符号
    if *ptr == b'-' as i8 {
        sign = -1;
        ptr = ptr.add(1);
    } else if *ptr == b'+' as i8 {
        ptr = ptr.add(1);
    }

    // 自动检测 base
    if actual_base == 0 {
        if *ptr == b'0' as i8 {
            let next = *ptr.add(1);
            if next == b'x' as i8 || next == b'X' as i8 {
                actual_base = 16;
                ptr = ptr.add(2);
            } else {
                actual_base = 8;
                ptr = ptr.add(1);
            }
        } else {
            actual_base = 10;
        }
    }

    // 转换数字
    loop {
        let ch = *ptr;
        let digit: i64;

        if ch >= b'0' as i8 && ch <= b'9' as i8 {
            digit = (ch - b'0' as i8) as i64;
        } else if ch >= b'a' as i8 && ch <= b'f' as i8 {
            digit = (ch - b'a' as i8) as i64 + 10;
        } else if ch >= b'A' as i8 && ch <= b'F' as i8 {
            digit = (ch - b'A' as i8) as i64 + 10;
        } else {
            break; // 无效字符
        }

        if digit >= actual_base as i64 {
            break;
        }

        result = result.wrapping_mul(actual_base as i64).wrapping_add(digit);
        ptr = ptr.add(1);
    }

    if !endptr.is_null() {
        *endptr = ptr as *mut i8;
    }

    sign * result
}

// ============================================================================
// Internet Checksum 计算
// ============================================================================

/// 计算 Internet Checksum (RFC 1071)
///
/// 用于 IP、TCP、UDP 协议头的校验和计算。
///
/// # Arguments
/// * `data` - 数据缓冲区
/// * `len` - 数据长度（字节）
///
/// # Returns
/// 16位校验和值
pub fn inet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;

    // 按 16 位字累加
    while i + 1 < data.len() {
        let word = ((data[i] as u16) << 8) | data[i + 1] as u16;
        sum = sum.wrapping_add(word as u32);
        i += 2;
    }

    // 处理奇数长度字节
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u16) as u32);
    }

    // 折叠 32 位和到 16 位
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    (!sum) as u16
}

/// 通过 FFI 暴露的 checksum 函数（供 C 代码调用）
///
/// # Safety
/// 此函数通过 FFI 暴露给 C 代码使用
#[no_mangle]
pub unsafe extern "C" fn rust_inet_chksum(data: *const u8, len: usize) -> u16 {
    if data.is_null() || len == 0 {
        return 0;
    }

    let slice = core::slice::from_raw_parts(data, len);
    inet_checksum(slice)
}

// ============================================================================
// 字节序转换
// ============================================================================

/// 主机字节序转网络字节序 (16位)
#[inline(always)]
pub const fn htons(host: u16) -> u16 {
    host.to_be()
}

/// 网络字节序转主机字节序 (16位)
#[inline(always)]
pub const fn ntohs(net: u16) -> u16 {
    u16::from_be(net)
}

/// 主机字节序转网络字节序 (32位)
#[inline(always)]
pub const fn htonl(host: u32) -> u32 {
    host.to_be()
}

/// 网络字节序转主机字节序 (32位)
#[inline(always)]
pub const fn ntohl(net: u32) -> u32 {
    u32::from_be(net)
}

/// 通过 FFI 暴露的字节序转换函数
#[no_mangle]
pub extern "C" fn rust_htons(host: u16) -> u16 { htons(host) }

#[no_mangle]
pub extern "C" fn rust_ntohs(net: u16) -> u16 { ntohs(net) }

#[no_mangle]
pub extern "C" fn rust_htonl(host: u32) -> u32 { htonl(host) }

#[no_mangle]
pub extern "C" fn rust_ntohl(net: u32) -> u32 { ntohl(net) }

// ============================================================================
// MAC/IP 地址工具
// ============================================================================

/// MAC 地址格式化为字符串
///
/// 格式: "XX:XX:XX:XX:XX:XX"
///
/// # Arguments
/// * `mac` - 6字节 MAC 地址
/// * `buf` - 输出缓冲区 (至少18字节)
///
/// # Returns
/// 写入的字符串长度
pub fn format_mac(mac: &[u8; 6], buf: &mut [u8]) -> usize {
    const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";

    if buf.len() < 17 {
        return 0;
    }

    for i in 0..6 {
        if i > 0 {
            buf[i * 3 - 1] = b':';
        }
        buf[i * 3] = HEX_CHARS[(mac[i] >> 4) as usize];
        buf[i * 3 + 1] = HEX_CHARS[(mac[i] & 0x0F) as usize];
    }

    17
}

/// IP 地址格式化为字符串 (IPv4)
///
/// 格式: "XXX.XXX.XXX.XXX"
///
/// # Arguments
/// * `ip` - 4字节 IPv4 地址 (网络字节序)
/// * `buf` - 输出缓冲区 (至少16字节)
///
/// # Returns
/// 写入的字符串长度
pub fn format_ipv4(ip: [u8; 4], buf: &mut [u8]) -> usize {
    if buf.len() < 15 {
        return 0;
    }

    let mut pos = 0;
    for i in 0..4 {
        if i > 0 {
            buf[pos] = b'.';
            pos += 1;
        }

        // 将数字转为字符串
        let mut num = ip[i];
        let mut digits = [0u8; 3];
        let mut len = 0;

        if num == 0 {
            digits[len] = b'0';
            len += 1;
        } else {
            while num > 0 {
                digits[len] = b'0' + (num % 10);
                num /= 10;
                len += 1;
            }
        }

        // 反转并复制
        for j in (0..len).rev() {
            buf[pos] = digits[j];
            pos += 1;
        }
    }

    pos
}

/// 解析 IPv4 地址字符串
///
/// # Arguments
/// * `str` - 输入字符串 ("XXX.XXX.XXX.XXX")
///
/// # Returns
/// Some([u8; 4]) 如果解析成功，None 如果失败
pub fn parse_ipv4(str: &[u8]) -> Option<[u8; 4]> {
    let mut result = [0u8; 4];
    let mut octet_idx = 0;
    let mut current_val: u8 = 0;

    for &ch in str {
        match ch {
            b'0'..=b'9' => {
                current_val = current_val.wrapping_mul(10).wrapping_add(ch - b'0');
                if current_val > 255 {
                    return None;
                }
            }
            b'.' => {
                if octet_idx >= 3 {
                    return None;
                }
                result[octet_idx] = current_val;
                octet_idx += 1;
                current_val = 0;
            }
            _ => return None,
        }
    }

    // 最后一个八位组
    if octet_idx != 3 {
        return None;
    }
    result[octet_idx] = current_val;

    Some(result)
}

// ============================================================================
// 内存操作工具
// ============================================================================

/// 安全的内存拷贝（带边界检查）
///
/// # Arguments
/// * `dst` - 目标缓冲区
/// * `src` - 源数据
///
/// # Returns
/// 实际拷贝的字节数
pub fn safe_memcpy(dst: &mut [u8], src: &[u8]) -> usize {
    let len = dst.len().min(src.len());
    dst[..len].copy_from_slice(&src[..len]);
    len
}

/// 内存比较
///
/// # Returns
/// * 0 - 相等
/// * <0 - dst < src
/// * \>0 - dst > src
pub fn memcmp(dst: &[u8], src: &[u8]) -> i32 {
    let min_len = dst.len().min(src.len());

    for i in 0..min_len {
        match dst[i].cmp(&src[i]) {
            core::cmp::Ordering::Equal => continue,
            core::cmp::Ordering::Less => return -1,
            core::cmp::Ordering::Greater => return 1,
        }
    }

    dst.len().cmp(&src.len()) as i32
}

/// 内存设置 (类似 memset)
///
/// # Arguments
/// * `buf` - 目标缓冲区
/// * `val` - 设置的值
/// * `len` - 设置的字节数
pub fn memset(buf: &mut [u8], val: u8, len: usize) {
    let actual_len = buf.len().min(len);
    for i in 0..actual_len {
        buf[i] = val;
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atoi_basic() {
        unsafe {
            assert_eq!(atoi(b"123\0".as_ptr() as *const i8), 123);
            assert_eq!(atoi(b"-456\0".as_ptr() as *const i8), -456);
            assert_eq!(atoi(b"0\0".as_ptr() as *const i8), 0);
            assert_eq!(atoi(b"  789  \0".as_ptr() as *const i8), 789);
        }
    }

    #[test]
    fn test_strtol_basic() {
        unsafe {
            let mut endptr: *mut i8 = core::ptr::null_mut();
            
            // 十进制
            let val = strtol(b"12345\0".as_ptr() as *const i8, &mut endptr, 0);
            assert_eq!(val, 12345);
            
            // 十六进制
            let val = strtol(b"0xFF\0".as_ptr() as *const i8, &mut endptr, 0);
            assert_eq!(val, 255);
            
            // 八进制
            let val = strtol(b"0777\0".as_ptr() as *const i8, &mut endptr, 0);
            assert_eq!(val, 511);
            
            // 负数
            let val = strtol(b"-100\0".as_ptr() as *const i8, &mut endptr, 0);
            assert_eq!(val, -100);
        }
    }

    #[test]
    fn test_inet_checksum() {
        // 测试数据: "Hello World" 的 ASCII
        let data = b"Hello World";
        let cksum = inet_checksum(data);
        
        // 验证: 对相同数据多次计算应该得到相同结果
        assert_eq!(cksum, inet_checksum(data));
        
        // 验证: 改变数据后校验和应该不同
        let data2 = b"Hello World!";
        assert_ne!(cksum, inet_checksum(data2));
    }

    #[test]
    fn test_byteorder_conversion() {
        // 在小端系统上测试
        assert_eq!(htons(0x1234), 0x3412.to_be());
        assert_eq!(ntohs(0x3412), u16::from_be(0x3412));
        assert_eq!(htonl(0x12345678), 0x78563412.to_be());
        assert_eq!(ntohl(0x78563412), u32::from_be(0x78563412));
    }

    #[test]
    fn test_mac_formatting() {
        let mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        let mut buf = [0u8; 18];
        let len = format_mac(&mac, &mut buf);
        
        assert_eq!(len, 17);
        assert_eq!(&buf[..17], b"00:11:22:33:44:55");
    }

    #[test]
    fn test_ipv4_formatting_and_parsing() {
        let ip = [192, 168, 1, 1];
        let mut buf = [0u8; 16];
        let len = format_ipv4(ip, &mut buf);
        
        assert_eq!(len, 11);
        assert_eq!(&buf[..11], b"192.168.1.1");
        
        // 测试解析
        let parsed = parse_ipv4(b"192.168.1.1");
        assert_eq!(parsed, Some([192, 168, 1, 1]));
        
        // 测试无效地址
        assert_eq!(parse_ipv4(b"256.1.1.1"), None); // 超范围
        assert_eq!(parse_ipv4(b"1.2.3"), None);      // 缺少八位组
    }

    #[test]
    fn test_memory_operations() {
        let mut dst = [0u8; 10];
        let src = [1, 2, 3, 4, 5];
        
        let copied = safe_memcpy(&mut dst, &src);
        assert_eq!(copied, 5);
        assert_eq!(&dst[..5], &src[..5]);
        
        // 测试 memcmp
        assert_eq!(memcmp(&dst[..5], &src), 0);
        assert_eq!(memcmp(&[1, 2, 3], &[1, 2, 4]), -1);
        
        // 测试 memset
        memset(&mut dst, 0xFF, 10);
        assert_eq!(dst, [0xFF; 10]);
    }
}
