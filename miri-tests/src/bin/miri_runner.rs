//! Miri runner binary
//!
//! 显式调用每个模块的健壮性测试, 避免 cfg(test) 依赖
//! 适用于 `cargo miri run --bin miri-runner`

use queenx_miri_tests::boot_image::{crc32, BootImageHeader, HEADER_MAGIC};
use queenx_miri_tests::frame::{Frame, PhysAddr, MAX_ORDER, PAGE_SIZE};
use queenx_miri_tests::gf256::{gf_add, gf_div, gf_inv, gf_mul, gf_sub};
use queenx_miri_tests::racy_cell::RacyCell;
use queenx_miri_tests::validators::{validate_memory_layout, MemRegion};

fn main() {
    println!("=== QueenX Miri Soundness Runner ===");

    // 1. RacyCell
    {
        let cell = RacyCell::new(0u64);
        // SAFETY: 单线程
        unsafe { cell.modify(|v| *v += 100) };
        // SAFETY: 同上
        let v = unsafe { *cell.get() };
        assert_eq!(v, 100, "RacyCell basic");
        println!("[OK] RacyCell basic");
    }

    // 2. Frame 对齐
    {
        let aligned = PhysAddr::new(0x10000);
        assert!(aligned.is_page_aligned());
        let unaligned = PhysAddr::new(0x10001);
        assert!(!unaligned.is_page_aligned());
        // SAFETY: 对齐 + 合法 order
        let f = unsafe { Frame::from_raw(PhysAddr::new(0x10000), 2) };
        assert_eq!(f.size_bytes(), 4 * PAGE_SIZE);
        assert_eq!(f.end(), PhysAddr::new(0x10000 + 4 * PAGE_SIZE as u64));
        println!("[OK] Frame alignment & size");

        // 大 order 不溢出
        let _ = unsafe { Frame::from_raw(PhysAddr::new(0), MAX_ORDER) };
        println!("[OK] Frame large order no overflow");
    }

    // 3. GF(2^8) 算术
    {
        // 零律
        for a in 0u8..=255 {
            assert_eq!(gf_mul(a, 0), 0);
        }
        // 单位元
        for a in 0u8..=255 {
            assert_eq!(gf_mul(a, 1), a);
        }
        // 逆元
        for a in 1u8..=255 {
            let inv = gf_inv(a);
            assert_eq!(gf_mul(a, inv), 1, "inv failed for a={}", a);
        }
        // 加减 = XOR
        assert_eq!(gf_add(0xab, 0xcd), 0xab ^ 0xcd);
        assert_eq!(gf_sub(0xab, 0xcd), 0xab ^ 0xcd);
        // 除法
        for a in 0u8..=255 {
            for b in 1u8..=10 {
                assert_eq!(gf_mul(gf_div(a, b), b), a);
            }
        }
        println!("[OK] GF(2^8) full arithmetic");
    }

    // 4. BootImage 编码
    {
        let h = BootImageHeader {
            magic: HEADER_MAGIC,
            version: 0x0001_0000,
            flags: 0xabcd_1234,
            total_size: 8192,
            capabilities: 0x1234_5678_9abc_def0,
            crc: 0,
        };
        let buf = h.encode();
        let decoded = BootImageHeader::decode(&buf).expect("decode failed");
        // 注意: encode 内部会重新计算 crc 并写入, 所以 decoded.crc
        // 是实际编码值, h.crc 是 0, 这里只比较其他字段
        assert_eq!(decoded.magic, h.magic);
        assert_eq!(decoded.version, h.version);
        assert_eq!(decoded.flags, h.flags);
        assert_eq!(decoded.total_size, h.total_size);
        assert_eq!(decoded.capabilities, h.capabilities);

        // CRC-32 已知值
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        println!("[OK] BootImage roundtrip + CRC32");
    }

    // 5. 校验器
    {
        assert_eq!(validate_memory_layout(), Ok(()));

        let r = MemRegion::new(0x1000, 0x2000);
        assert!(r.contains(0x1000, 0x100));
        assert!(!r.contains(u64::MAX, 1)); // 不溢出
        assert!(r.overlaps(&MemRegion::new(0x2000, 0x1000)));
        assert!(!r.overlaps(&MemRegion::new(0x4000, 0x1000)));
        println!("[OK] Validators");
    }

    println!();
    println!("=== ALL MIRI SOUNDNESS CHECKS PASSED ===");
}
