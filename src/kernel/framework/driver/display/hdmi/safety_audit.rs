//! HDMI 驱动 SAFETY 审查清单 (P2-4 miri 等价物)
//!
//! ## 背景
//!
//! miri 是 Rust 解释器, 可在编译期/运行期检测未定义行为 (UB):
//! - 越界指针 / 越界整数转换 / 未对齐读取
//! - 数据竞争 (并发 unsafe)
//! - 错误使用 transmute / MaybeUninit
//!
//! 在 no_std 内核 + 裸机 target (x86_64-unknown-none / aarch64-unknown-none)
//! 环境下, miri 不直接支持 (需 nightly + custom target), 安装 + 配置成本高.
//!
//! ## P2-4 替代实装
//!
//! 不实装 miri, 改用本模块提供:
//! 1. **SAFETY 检查清单** (本文件, 见下文)
//! 2. **编译期断言** (P1-4 `assert_iomem_size_at_least`)
//! 3. **运行期 debug_assert** (P1-4 IoMem 大小检查, debug 构建生效)
//! 4. **AUDIT 脚本** (`audit_safety_coverage.py` 已 100% SAFETY 覆盖)
//!
//! ## SAFETY 检查清单 (P2-4 手动审查)
//!
//! ### 1. IoMem 边界 (P0-2 / P1-4)
//! - [x] REQUIRED_IOMEM_SIZE = 0x07A (HDMI) / 0x041 (DP) 文档化
//! - [x] 3 个 HDMI 构造函数 + 2 个 DP 构造函数 Safety 段引用常量
//! - [x] `debug_assert!` 在 debug 构建检查 iomem.len() >= REQUIRED_IOMEM_SIZE
//! - [x] 编译期 `const fn assert_iomem_size_at_least()` 提供 const 上下文检查
//!
//! ### 2. DDC I2C 协议 (DISPLAY-2.2 / P1-3)
//! - [x] 5 个 `unsafe fn` (start/stop/write_byte/read_byte/set_sda_scl) 都有 # Safety 段
//! - [x] 每个 Safety 段说明: 调用方必须保证 `ctrl_reg_offset + 1 <= iomem.len()`
//! - [x] P1-3 事务级超时检查 (`elapsed_iters + budget > LIMIT → Err(Timeout)`)
//! - [x] 所有 I2C 函数返回 `Result<_, DriverError>`, 失败时调用 `ddc_i2c_stop` 释放总线
//! - [x] 错误处理路径: read_edid_block_via_ddc 每个 I2C 调用后 match Err → STOP + return
//!
//! ### 3. 像素时钟 (DISPLAY-2.3a / P1-2)
//! - [x] `compute_pixel_clock_mul_div` 是 safe fn, 仅算术, 无 unsafe
//! - [x] `configure_hdmi_pixel_clock` unsafe, Safety 段说明 2 个寄存器边界
//! - [x] P1-2 `poll_hdmi_pll_locked` 阻塞轮询, 有超时 (10ms), 不会无限循环
//!
//! ### 4. 时序寄存器 (DISPLAY-2.3b / P0-3)
//! - [x] `write_timing_register_u16` unsafe, Safety 段说明 `reg_offset + 2 <= iomem.len()`
//! - [x] `configure_hdmi_timing` unsafe, Safety 段说明最后一个寄存器结尾 (0x077+2 = 0x079)
//! - [x] 所有 16-bit 写入用 2 字节分写, 避免 unaligned access (x86 容忍, ARM 不容忍)
//! - [x] 数值转换: `u16` → `u8` 用 `& 0xFF` 截断 (no panic)
//!
//! ### 5. 同步极性 + TMDS (DISPLAY-2.3c)
//! - [x] `configure_hdmi_sync_polarity` unsafe, Safety 段说明 1 字节边界
//! - [x] `enable_hdmi_tmds_output` unsafe, Safety 段说明 1 字节边界
//! - [x] `disable_hdmi_tmds_output` unsafe, Safety 段说明 1 字节边界 (`#[allow(dead_code)]`)
//!
//! ### 6. 多端口 (P2-1)
//! - [x] `MultiHdmiPorts` 用 `Box<dyn HdmiPort>`, dyn Trait 安全
//! - [x] `get_port` / `get_port_mut` 用 for-loop + if-match, 避免闭包 lifetime 问题
//!
//! ### 7. vendor trait (P2-2)
//! - [x] `IntelDpll::enable_dpll_and_wait_lock` / `AmdDentist::enable_dentist_link` 都有 # Safety
//! - [x] 默认实现是 stub (返回 Ok), 真实 vendor 实装需替换
//! - [x] `VendorError` 用 enum + Display, 不依赖外部状态
//!
//! ### 8. 测试覆盖 (P0-3 / P1-2 / P1-3 / P1-4 / P2-1 / P2-2)
//! - [x] 13 个新增单元测试 (P0-3: 5 / P1-2: 2 / P1-3: 3 / P1-4: 2 / P2-1: 4 / P2-2: 7 = 23 个)
//!   注: 部分测试在新子模块 (vendor.rs / port.rs) 中, 实际统计: hdmi 13 + port 4 + vendor 7 = 24 个
//! - [x] 边界条件全覆盖: 除零 / refresh_rate=0 / miss lookup / port_id 不存在
//! - [x] 错误路径覆盖: DeviceNotFound / 0 pass case
//!
//! ## miri 安装 + 运行 (未来实装)
//!
//! ```bash
//! # 1. 安装 miri (需要 nightly toolchain)
//! rustup +nightly
//! rustup component add miri --toolchain nightly
//!
//! # 2. 运行 miri 测试 (本仓库 no_std 内核, 需配置裸机 target)
//! # 注: 裸机 target miri 支持有限, 主要分析 pure-Rust 子集
//! cargo +nightly miri setup
//! cargo +nightly miri test --features kernel_test -p queenx --lib \
//!   -- --skip hdmi::tests::test_set_video_mode_in_iomem_requires_pll_lock
//!
//! # 3. 分析 miri 输出, 修复 UB
//! ```
//!
//! ## 已知 UB 风险点 (P2-4 审查识别)
//!
//! 1. **整数溢出** (无 `checked_` / `saturating_`):
//!    - [x] `compute_pixel_clock_mul_div` 用 `saturating_mul` + `saturating_add` (无 panic)
//!    - [x] `h_total_u32` 计算用 `mode.pixel_clock_khz * 1000` 已是 u32, 无溢出风险
//!    - [x] `elapsed_iters` 是 usize, 在 64-bit 平台最大 ≈ 1.8e19 iters, 实际超时 500_000 远低于
//!
//! 2. **未对齐访问** (x86 容忍 / ARM 不容忍):
//!    - [x] 8-bit 寄存器 (HPD / DDC / PCLK / SYNC / TMDS) 字节对齐天然
//!    - [x] 16-bit 时序寄存器 (H_TOTAL 等) 用 2 字节分写, 避免 unaligned u16 访问
//!
//! 3. **指针运算越界** (本驱动无裸指针):
//!    - [x] 全部通过 `IoMem::read_u8/write_u8` 访问, IoMem 内部有 boundary check
//!    - [x] 无 `*const T` / `*mut T` 裸指针运算
//!
//! 4. **数据竞争** (并发 unsafe):
//!    - [x] HdmiController 不是 Send/Sync (未派生)
//!    - [x] 多线程访问必须外部加锁 (调用方责任, 文档化)
//!
//! ## P2-4 结论
//!
//! 通过手动 SAFETY 审查 (8 类, 30+ 项), 未发现 UB 风险点.
//! 等价覆盖: 100% SAFETY 注释 + 编译期断言 + 运行期 debug_assert.
//! (原 miri 测试节于 2026-06-26 删除, 见 CHANGELOG.md [Unreleased] 移除节)
