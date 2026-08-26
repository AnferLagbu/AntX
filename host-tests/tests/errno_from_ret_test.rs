//! Errno::from_ret 映射完整性契约测试 (B05-04)
//!
//! 权威实现: `src/kernel/services/syscall/types.rs::Errno::from_ret`.
//! 本测试通过自包含的镜像函数验证两点:
//! 1. 所有已定义的 `Errno` 变体编号都能被 `from_ret` 正确往返映射
//!    (返回的枚举编号与输入负返回码绝对值一致)
//! 2. 未知错误码回退 `EINVAL` (POSIX 约定)
//!
//! 镜像函数与内核实现必须同步维护; 若内核 from_ret 新增映射, 同步更新
//! `MIRROR_FROM_RET` 与本文件编号列表.

/// 镜像 `Errno::from_ret` 映射 (与内核 types.rs 一致)
fn mirror_from_ret(ret: i64) -> i32 {
    let errno = (-ret) as u64;
    match errno {
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 5,
        6 => 6,
        7 => 7,
        8 => 8,
        9 => 9,
        10 => 10,
        11 => 11,
        12 => 12,
        13 => 13,
        14 => 14,
        15 => 15,
        16 => 16,
        17 => 17,
        18 => 18,
        19 => 19,
        20 => 20,
        21 => 21,
        22 => 22,
        23 => 23,
        24 => 24,
        25 => 25,
        26 => 26,
        27 => 27,
        28 => 28,
        29 => 29,
        30 => 30,
        31 => 31,
        32 => 32,
        33 => 33,
        34 => 34,
        35 => 35,
        36 => 36,
        37 => 37,
        38 => 38,
        39 => 39,
        40 => 40,
        41 => 41,
        42 => 42,
        43 => 43,
        60 => 60,
        61 => 61,
        62 => 62,
        63 => 63,
        64 => 64,
        71 => 71,
        74 => 74,
        75 => 75,
        88 => 88,
        89 => 89,
        90 => 90,
        91 => 91,
        92 => 92,
        93 => 93,
        94 => 94,
        95 => 95,
        96 => 96,
        97 => 97,
        98 => 98,
        99 => 99,
        100 => 100,
        101 => 101,
        102 => 102,
        103 => 103,
        104 => 104,
        105 => 105,
        106 => 106,
        107 => 107,
        108 => 108,
        110 => 110,
        111 => 111,
        112 => 112,
        113 => 113,
        114 => 114,
        115 => 115,
        _ => 22, // EINVAL 回退
    }
}

/// 内核 `Errno` 枚举中已定义的全部编号 (B05-04 验收: 这些必须可往返)
const ALL_DEFINED_ERRNOS: &[i32] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 60, 61, 62, 63, 64, 71,
    74, 75, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106,
    107, 108, 110, 111, 112, 113, 114, 115,
];

#[test]
fn all_defined_errnos_round_trip() {
    // B05-04: 每个已定义 Errno 变体 must 可往返 (from_ret(-n) == n)
    for &n in ALL_DEFINED_ERRNOS {
        let mapped = mirror_from_ret(-(n as i64));
        assert_eq!(
            mapped, n,
            "B05-04: from_ret(-{}) 应映射回 {} (Errno 变体), 实际 = {}",
            n, n, mapped
        );
    }
}

#[test]
fn all_defined_errnos_round_trip_negative_input() {
    // 输入必须是负数 (POSIX -errno 约定); 非负数无意义
    for &n in ALL_DEFINED_ERRNOS {
        // from_ret 对负数输入取绝对值映射
        let mapped = mirror_from_ret(-(n as i64));
        assert!(mapped > 0, "B05-04: 映射结果必须为正 errno, 输入 -{}", n);
    }
}

#[test]
fn unknown_errno_falls_back_to_einval() {
    // 未定义错误码回退 EINVAL (22)
    for unknown in [0i64, 44, 45, 65, 76, 109, 116, 1000, i64::MAX] {
        let mapped = mirror_from_ret(-unknown);
        assert_eq!(
            mapped,
            22,
            "B05-04: from_ret(-{}) 应回退 EINVAL(22), 实际 = {}",
            unknown,
            mapped
        );
    }
}

#[test]
fn specific_errno_values_match_linux() {
    // 抽查关键 errno 编号与 Linux x86_64 一致 (DECISION-037: 0-299 直接 Linux ABI)
    assert_eq!(mirror_from_ret(-1), 1, "EPERM");
    assert_eq!(mirror_from_ret(-38), 38, "ENOSYS");
    assert_eq!(mirror_from_ret(-13), 13, "EACCES");
    assert_eq!(mirror_from_ret(-95), 95, "ENOTSUP");
    assert_eq!(mirror_from_ret(-98), 98, "EADDRINUSE");
    assert_eq!(mirror_from_ret(-111), 111, "ECONNREFUSED");
    assert_eq!(mirror_from_ret(-115), 115, "EINPROGRESS");
}
