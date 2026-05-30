#![allow(dead_code)]
/// HTTP 服务器静态文件数据模块

// ============================================================================
// 常量定义
// ============================================================================


pub const FS_NUM_FILES: usize = 2;

// ============================================================================
// 文件元数据结构 (与 lwIP fs.c 兼容)
// ============================================================================

#[repr(C)]
pub struct FsFileEntry {
    pub next: *const FsFileEntry,
    pub data: *const u8,
    pub data_offset: *const u8,
    pub len: usize,
    pub flags: u8,
}

// SAFETY: FsFileEntry contains raw pointers to static data (embedded in
// the binary). No ownership or mutation; pointers are read-only after init.
unsafe impl Sync for FsFileEntry {}

// ============================================================================
// 文件数据 (使用 const 数组)
// ============================================================================

/// index.html 内容 (312字节)
const DATA_INDEX_HTML_LEN: usize = 312;
const INDEX_HTML_NAME_LEN: usize = 12; // "/index.html\0"
const INDEX_HTML_DATA_OFFSET: usize = 12;

static mut DATA_INDEX_HTML: [u8; 312] = [0u8; 312]; // 运行时初始化

/// 404.html 内容 (58字节)
const DATA_404_HTML_LEN: usize = 58;
const HTML_404_NAME_LEN: usize = 10; // "/404.html\0"
const HTML_404_DATA_OFFSET: usize = 10;

static mut DATA_404_HTML: [u8; 58] = [0u8; 58]; // 运行时初始化

// ============================================================================
// 文件系统链表
// ============================================================================

#[no_mangle]
pub static FILE_404_HTML: FsFileEntry = FsFileEntry {
    next: core::ptr::null(),
    data: unsafe { DATA_404_HTML.as_ptr() },
    data_offset: unsafe { DATA_404_HTML.as_ptr().add(HTML_404_DATA_OFFSET) },
    len: DATA_404_HTML_LEN - HTML_404_DATA_OFFSET,
    flags: 0x03,
};

#[no_mangle]
pub static FILE_INDEX_HTML: FsFileEntry = FsFileEntry {
    next: &FILE_404_HTML,
    data: unsafe { DATA_INDEX_HTML.as_ptr() },
    data_offset: unsafe { DATA_INDEX_HTML.as_ptr().add(INDEX_HTML_DATA_OFFSET) },
    len: DATA_INDEX_HTML_LEN - INDEX_HTML_DATA_OFFSET,
    flags: 0x03,
};

#[no_mangle]
pub static mut FS_ROOT: *const FsFileEntry = &FILE_INDEX_HTML;

// ============================================================================
// 初始化函数 (在启动时调用一次)
// ============================================================================

/// 初始化 HTTP 静态文件数据
/// 
/// 必须在第一次使用前调用此函数填充数据。
/// 通常在 qx_net_apps_init() 中自动调用。
#[no_mangle]
pub unsafe extern "C" fn fsdata_init() {
    // 填充 index.html 数据
    let html = b"/index.html\0<!DOCTYPE html>\n<html>\n<head><title>AntX Web Server</title></head>\n<body style='font-family:sans-serif'>\n  <h1>QueenX</h1>\n  <p>lwIP TCP/IP stack is running.</p>\n  <p>E1000 NIC 1000Mbps Full-Duplex.</p>\n</body>\n</html>\n";
    
    for (i, &byte) in html.iter().enumerate() {
        if i < DATA_INDEX_HTML.len() {
            DATA_INDEX_HTML[i] = byte;
        } else {
            break;
        }
    }
    
    // 填充 404.html 数据
    let html_404 = b"/404.html\0<html><body><h1>404</h1></body></html>";
    
    for (i, &byte) in html_404.iter().enumerate() {
        if i < DATA_404_HTML.len() {
            DATA_404_HTML[i] = byte;
        } else {
            break;
        }
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

pub fn get_file(index: usize) -> Option<&'static FsFileEntry> {
    match index {
        0 => Some(&FILE_INDEX_HTML),
        1 => Some(&FILE_404_HTML),
        _ => None,
    }
}

pub fn get_file_count() -> usize { FS_NUM_FILES }

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fsdata() {
        assert_eq!(get_file_count(), 2);
        assert!(get_file(0).is_some());
        assert!(get_file(1).is_some());
        assert!(get_file(2).is_none());
        
        let idx = get_file(0).unwrap();
        assert_eq!(idx.len, DATA_INDEX_HTML_LEN - INDEX_HTML_DATA_OFFSET);
    }
}
