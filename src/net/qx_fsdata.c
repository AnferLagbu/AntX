/* ============================================================
 * qx_fsdata.c — lwIP HTTP 自定义文件系统
 *
 * 提供 HTTP 服务器所需的静态文件数据。
 * 格式: 文件名(含\0) + 文件内容 (无内嵌 HTTP 头)
 * ============================================================ */

#include "lwip/apps/fs.h"
#include "lwip/def.h"

/* ---- 数据数组 (size_t 对齐) ---- */
#define FSDATA_ALIGN_PRE  static const unsigned char
#define FSDATA_ALIGN_POST __attribute__((aligned(sizeof(size_t))))

/* --- /index.html --- */
FSDATA_ALIGN_PRE data__index_html[] FSDATA_ALIGN_POST = {
    '/', 'i', 'n', 'd', 'e', 'x', '.', 'h', 't', 'm', 'l', 0,
    '<', '!', 'D', 'O', 'C', 'T', 'Y', 'P', 'E', ' ', 'h', 't', 'm', 'l', '>', '\n',
    '<', 'h', 't', 'm', 'l', '>', '\n',
    '<', 'h', 'e', 'a', 'd', '>', '<', 't', 'i', 't', 'l', 'e', '>', 'A', 'n', 't',
    'X', ' ', 'W', 'e', 'b', ' ', 'S', 'e', 'r', 'v', 'e', 'r', '<', '/', 't', 'i',
    't', 'l', 'e', '>', '<', '/', 'h', 'e', 'a', 'd', '>', '\n',
    '<', 'b', 'o', 'd', 'y', ' ', 's', 't', 'y', 'l', 'e', '=', '"', 'f', 'o', 'n',
    't', '-', 'f', 'a', 'm', 'i', 'l', 'y', ':', 's', 'a', 'n', 's', '-', 's', 'e',
    'r', 'i', 'f', '"', '>', '\n',
    ' ', ' ', '<', 'h', '1', '>', 'A', 'n', 't', 'X', ' ', 'K', 'e', 'r', 'n', 'e',
    'l', '<', '/', 'h', '1', '>', '\n',
    ' ', ' ', '<', 'p', '>', 'l', 'w', 'I', 'P', ' ', 'T', 'C', 'P', '/', 'I', 'P',
    ' ', 's', 't', 'a', 'c', 'k', ' ', 'i', 's', ' ', 'r', 'u', 'n', 'n', 'i', 'n',
    'g', '.', '<', '/', 'p', '>', '\n',
    ' ', ' ', '<', 'p', '>', 'E', '1', '0', '0', '0', ' ', 'N', 'I', 'C', ' ', '1',
    '0', '0', '0', 'M', 'b', 'p', 's', ' ', 'F', 'u', 'l', 'l', '-', 'D', 'u', 'p',
    'l', 'e', 'x', '.', '<', '/', 'p', '>', '\n',
    '<', '/', 'b', 'o', 'd', 'y', '>', '\n',
    '<', '/', 'h', 't', 'm', 'l', '>', '\n',
};

/* --- /404.html --- */
FSDATA_ALIGN_PRE data__404_html[] FSDATA_ALIGN_POST = {
    '/', '4', '0', '4', '.', 'h', 't', 'm', 'l', 0,
    '<', 'h', 't', 'm', 'l', '>', '<', 'b', 'o', 'd', 'y', '>',
    '<', 'h', '1', '>', '4', '0', '4', '<', '/', 'h', '1', '>',
    '<', '/', 'b', 'o', 'd', 'y', '>', '<', '/', 'h', 't', 'm', 'l', '>',
};

/* ---- 链表节点 (从后往前链接) ---- */
const struct fsdata_file file__404_html[] = { {
    NULL,
    data__404_html,
    data__404_html + 10,
    sizeof(data__404_html) - 10,
    FS_FILE_FLAGS_HEADER_INCLUDED | FS_FILE_FLAGS_HEADER_PERSISTENT,
} };

const struct fsdata_file file__index_html[] = { {
    file__404_html,
    data__index_html,
    data__index_html + 12,
    sizeof(data__index_html) - 12,
    FS_FILE_FLAGS_HEADER_INCLUDED | FS_FILE_FLAGS_HEADER_PERSISTENT,
} };

/* ---- fs.c 需要的宏 ---- */
#define FS_ROOT     file__index_html
#define FS_NUMFILES 2
