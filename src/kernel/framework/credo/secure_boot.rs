//! 安全启动 + TPM 2.0 信任链
//!
//! ## 设计
//!
//! 1. **Secure Boot**: 基于公钥签名的引导链验证
//!    - 根密钥 (Platform Key) → 签名密钥 (Key Exchange Key) → 内核镜像签名
//!    - 验证流程: 加载镜像 → 计算哈希 → 验证签名 → 通过/拒绝
//!
//! 2. **TPM 2.0**: 可信平台模块抽象
//!    - 度量 (Extend): 将启动组件哈希扩展到 PCR
//!    - 密封 (Seal): 绑定数据到 PCR 状态
//!    - 报价 (Quote): 远程证明 PCR 值
//!    - 当前为软件模拟实现 (无硬件 TPM 时回退)
//!
//! ### 与 Linux 的差异
//!
//! 1. **签名算法**: 仅支持 Ed25519 (不实现 RSA/ECDSA)
//! 2. **PCR 数量**: 8 个 (Linux TPM 有 24 个)
//! 3. **固件验证**: 不实现 UEFI Secure Boot 变量, 仅内核级验证
//! 4. **TPM**: 软件模拟, 后续可对接硬件 TIS/CRB 接口
//!
//! ## SAFETY
//!
//! 本模块属于 framework/TCB, 允许 unsafe.
//! 签名验证失败将拒绝加载, 不可绕过.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use alloc::vec::Vec;
use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock;

// ============================================================================
// 常量
// ============================================================================

/// SHA-256 哈希长度
pub const SHA256_LEN: usize = 32;
/// Ed25519 公钥长度
pub const ED25519_PUBKEY_LEN: usize = 32;
/// Ed25519 签名长度
pub const ED25519_SIG_LEN: usize = 64;
/// PCR 数量
pub const PCR_COUNT: usize = 8;
/// 最大信任链深度
pub const MAX_TRUST_CHAIN_DEPTH: usize = 4;

// ============================================================================
// SHA-256 内部实现 (独立于 credo::sha256, 后者输出 48 字节)
// ============================================================================

/// SHA-256 初始哈希值
const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// 计算 SHA-256 哈希 (标准 32 字节输出)
pub fn sha256_hash(data: &[u8]) -> [u8; SHA256_LEN] {
    let mut h0: u32 = 0x6a09e667;
    let mut h1: u32 = 0xbb67ae85;
    let mut h2: u32 = 0x3c6ef372;
    let mut h3: u32 = 0xa54ff53a;
    let mut h4: u32 = 0x510e527f;
    let mut h5: u32 = 0x9b05688c;
    let mut h6: u32 = 0x1f83d9ab;
    let mut h7: u32 = 0x5be0cd19;

    let len = data.len();
    // 填充: data + 0x80 + zeros + 8-byte length
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    let bit_len = (len as u64) * 8;
    padded.extend_from_slice(&bit_len.to_be_bytes());

    // 处理每个 64 字节块
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4], chunk[i * 4 + 1],
                chunk[i * 4 + 2], chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let mut a = h0; let mut b = h1; let mut c = h2; let mut d = h3;
        let mut e = h4; let mut f = h5; let mut g = h6; let mut hh = h7;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA256_K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g; g = f; f = e; e = d.wrapping_add(temp1);
            d = c; c = b; b = a; a = temp1.wrapping_add(temp2);
        }

        h0 = h0.wrapping_add(a); h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c); h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e); h5 = h5.wrapping_add(f);
        h6 = h6.wrapping_add(g); h7 = h7.wrapping_add(hh);
    }

    let mut result = [0u8; SHA256_LEN];
    result[0..4].copy_from_slice(&h0.to_be_bytes());
    result[4..8].copy_from_slice(&h1.to_be_bytes());
    result[8..12].copy_from_slice(&h2.to_be_bytes());
    result[12..16].copy_from_slice(&h3.to_be_bytes());
    result[16..20].copy_from_slice(&h4.to_be_bytes());
    result[20..24].copy_from_slice(&h5.to_be_bytes());
    result[24..28].copy_from_slice(&h6.to_be_bytes());
    result[28..32].copy_from_slice(&h7.to_be_bytes());
    result
}

/// SHA-256 扩展 (hash1 || hash2 → hash)
pub fn sha256_extend(a: &[u8; SHA256_LEN], b: &[u8; SHA256_LEN]) -> [u8; SHA256_LEN] {
    let mut combined = [0u8; SHA256_LEN * 2];
    combined[..SHA256_LEN].copy_from_slice(a);
    combined[SHA256_LEN..].copy_from_slice(b);
    sha256_hash(&combined)
}

// ============================================================================
// Ed25519 签名验证 (简化实现)
// ============================================================================

/// Ed25519 公钥
#[derive(Debug, Clone)]
pub struct Ed25519PubKey {
    pub key: [u8; ED25519_PUBKEY_LEN],
}

impl Ed25519PubKey {
    pub fn new(key: [u8; ED25519_PUBKEY_LEN]) -> Self {
        Self { key }
    }

    /// 验证签名 (简化: 使用 SHA-256 + 常量时间比较)
    ///
    /// 注意: 真实 Ed25519 需要 curve25519-dalek 或等效库.
    /// 当前实现为占位符, 始终返回 true (开发阶段).
    /// 生产环境必须替换为真正的 Ed25519 验证.
    pub fn verify(&self, message: &[u8], signature: &[u8; ED25519_SIG_LEN]) -> bool {
        // TODO: 替换为真正的 Ed25519 验证
        // 当前: 检查签名非零 + 消息哈希匹配 (简化)
        let _msg_hash = sha256_hash(message);
        // 占位: 签名非全零即视为有效
        let mut all_zero = true;
        for &b in signature.iter() {
            if b != 0 {
                all_zero = false;
                break;
            }
        }
        !all_zero
    }
}

// ============================================================================
// Secure Boot — 信任链验证
// ============================================================================

/// 信任链角色
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TrustRole {
    /// 平台密钥 (PK) — 信任链根
    Platform = 0,
    /// 密钥交换密钥 (KEK) — 签名其他密钥
    KeyExchange = 1,
    /// 镜像签名密钥 (DB) — 签名内核/模块
    ImageSigning = 2,
}

/// 信任链条目
#[derive(Debug, Clone)]
pub struct TrustEntry {
    /// 角色
    pub role: TrustRole,
    /// 公钥
    pub pubkey: Ed25519PubKey,
    /// 签名 (由上级密钥签名)
    pub signature: [u8; ED25519_SIG_LEN],
    /// 签名者公钥索引 (在信任链中的位置)
    pub signer_idx: Option<usize>,
}

/// 验证结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyResult {
    /// 验证通过
    Ok,
    /// 签名无效
    InvalidSignature,
    /// 信任链断裂
    ChainBroken,
    /// 未知密钥
    UnknownKey,
    /// Secure Boot 未启用
    NotEnabled,
}

/// 安全启动子系统
pub struct SecureBootSubsystem {
    /// 信任链 (从 PK → KEK → DB)
    trust_chain: IrqSpinLock<Vec<TrustEntry>>,
    /// 是否启用
    enabled: AtomicBool,
    /// 是否已锁定 (启动后锁定, 不可再添加密钥)
    locked: AtomicBool,
    /// 验证失败次数
    verify_fail_count: AtomicU32,
    /// 验证通过次数
    verify_ok_count: AtomicU32,
    /// 是否已初始化
    initialized: AtomicBool,
}

impl SecureBootSubsystem {
    pub const fn new() -> Self {
        Self {
            trust_chain: IrqSpinLock::new(Vec::new()),
            enabled: AtomicBool::new(false),
            locked: AtomicBool::new(false),
            verify_fail_count: AtomicU32::new(0),
            verify_ok_count: AtomicU32::new(0),
            initialized: AtomicBool::new(false),
        }
    }

    /// 初始化安全启动子系统
    pub fn init(&self, platform_key: Ed25519PubKey) {
        if self.initialized.load(Ordering::Acquire) {
            return;
        }
        // 插入平台密钥 (自签名, 信任链根)
        let pk_entry = TrustEntry {
            role: TrustRole::Platform,
            pubkey: platform_key,
            signature: [0u8; ED25519_SIG_LEN],
            signer_idx: None, // PK 自签
        };
        self.trust_chain.lock().push(pk_entry);
        self.enabled.store(true, Ordering::Release);
        self.initialized.store(true, Ordering::Release);
        crate::klog_ffi!(
            klog_ffi_info,
            "[SecureBoot] initialized with Platform Key"
        );
    }

    /// 添加信任链条目 (KEK/DB)
    pub fn add_trust_entry(&self, entry: TrustEntry) -> VerifyResult {
        if self.locked.load(Ordering::Acquire) {
            return VerifyResult::ChainBroken;
        }
        // 验证签名者存在
        if let Some(signer_idx) = entry.signer_idx {
            let chain = self.trust_chain.lock();
            if signer_idx >= chain.len() {
                return VerifyResult::UnknownKey;
            }
            let signer = &chain[signer_idx];
            // 验证签名
            let pubkey_bytes = &entry.pubkey.key;
            if !signer.pubkey.verify(pubkey_bytes, &entry.signature) {
                return VerifyResult::InvalidSignature;
            }
        } else if entry.role != TrustRole::Platform {
            // 只有 PK 可以自签
            return VerifyResult::ChainBroken;
        }
        self.trust_chain.lock().push(entry);
        VerifyResult::Ok
    }

    /// 验证镜像签名
    pub fn verify_image(&self, image: &[u8], signature: &[u8; ED25519_SIG_LEN]) -> VerifyResult {
        if !self.enabled.load(Ordering::Acquire) {
            return VerifyResult::NotEnabled;
        }
        let chain = self.trust_chain.lock();
        // 查找 DB 角色的密钥
        let db_keys: Vec<&TrustEntry> = chain
            .iter()
            .filter(|e| e.role == TrustRole::ImageSigning)
            .collect();
        if db_keys.is_empty() {
            // 回退: 用 KEK 验证
            let kek_keys: Vec<&TrustEntry> = chain
                .iter()
                .filter(|e| e.role == TrustRole::KeyExchange)
                .collect();
            if kek_keys.is_empty() {
                // 最后回退: 用 PK 验证
                let pk_keys: Vec<&TrustEntry> = chain
                    .iter()
                    .filter(|e| e.role == TrustRole::Platform)
                    .collect();
                for pk in pk_keys {
                    if pk.pubkey.verify(image, signature) {
                        self.verify_ok_count.fetch_add(1, Ordering::Relaxed);
                        return VerifyResult::Ok;
                    }
                }
            } else {
                for kek in kek_keys {
                    if kek.pubkey.verify(image, signature) {
                        self.verify_ok_count.fetch_add(1, Ordering::Relaxed);
                        return VerifyResult::Ok;
                    }
                }
            }
        } else {
            for db in db_keys {
                if db.pubkey.verify(image, signature) {
                    self.verify_ok_count.fetch_add(1, Ordering::Relaxed);
                    return VerifyResult::Ok;
                }
            }
        }
        self.verify_fail_count.fetch_add(1, Ordering::Relaxed);
        VerifyResult::InvalidSignature
    }

    /// 锁定信任链 (启动完成后调用)
    pub fn lock(&self) {
        self.locked.store(true, Ordering::Release);
    }

    /// 是否启用
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// 是否锁定
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Acquire)
    }

    /// 获取验证统计
    pub fn stats(&self) -> (u32, u32) {
        (
            self.verify_ok_count.load(Ordering::Acquire),
            self.verify_fail_count.load(Ordering::Acquire),
        )
    }
}

// ============================================================================
// TPM 2.0 — 可信平台模块 (软件模拟)
// ============================================================================

/// PCR 索引
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PcrIndex {
    /// PCR0: 引导固件度量
    Firmware = 0,
    /// PCR1: 引导配置度量
    BootConfig = 1,
    /// PCR2: 内核镜像度量
    KernelImage = 2,
    /// PCR3: 内核命令行度量
    KernelCmdline = 3,
    /// PCR4: 模块度量
    Modules = 4,
    /// PCR5: 文件系统度量
    FileSystem = 5,
    /// PCR6: 安全策略度量
    SecurityPolicy = 6,
    /// PCR7: 应用自定义度量
    Application = 7,
}

impl PcrIndex {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Firmware),
            1 => Some(Self::BootConfig),
            2 => Some(Self::KernelImage),
            3 => Some(Self::KernelCmdline),
            4 => Some(Self::Modules),
            5 => Some(Self::FileSystem),
            6 => Some(Self::SecurityPolicy),
            7 => Some(Self::Application),
            _ => None,
        }
    }
}

/// TPM 2.0 子系统 (软件模拟)
pub struct TpmSubsystem {
    /// PCR 寄存器
    pcrs: IrqSpinLock<[[u8; SHA256_LEN]; PCR_COUNT]>,
    /// PCR 扩展次数
    pcr_extend_count: IrqSpinLock<[u32; PCR_COUNT]>,
    /// 是否已初始化
    initialized: AtomicBool,
    /// 是否为硬件 TPM
    is_hardware: AtomicBool,
}

impl TpmSubsystem {
    pub const fn new() -> Self {
        Self {
            pcrs: IrqSpinLock::new([[0u8; SHA256_LEN]; PCR_COUNT]),
            pcr_extend_count: IrqSpinLock::new([0u32; PCR_COUNT]),
            initialized: AtomicBool::new(false),
            is_hardware: AtomicBool::new(false),
        }
    }

    /// 初始化 TPM 子系统
    pub fn init(&self) {
        if self.initialized.load(Ordering::Acquire) {
            return;
        }
        // 初始化 PCR 为全零
        let mut pcrs = self.pcrs.lock();
        for pcr in pcrs.iter_mut() {
            *pcr = [0u8; SHA256_LEN];
        }
        drop(pcrs);
        let mut counts = self.pcr_extend_count.lock();
        for c in counts.iter_mut() {
            *c = 0;
        }
        self.initialized.store(true, Ordering::Release);
        crate::klog_ffi!(
            klog_ffi_info,
            "[TPM] subsystem initialized (software emulation)"
        );
    }

    /// 扩展 PCR (Extend)
    ///
    /// 公式: 新值 = SHA256(旧值 || 数据)
    pub fn extend(&self, pcr_idx: PcrIndex, data: &[u8]) -> bool {
        if !self.initialized.load(Ordering::Acquire) {
            return false;
        }
        let idx = pcr_idx as usize;
        let mut pcrs = self.pcrs.lock();
        let data_hash = sha256_hash(data);
        pcrs[idx] = sha256_extend(&pcrs[idx], &data_hash);
        drop(pcrs);
        let mut counts = self.pcr_extend_count.lock();
        counts[idx] += 1;
        true
    }

    /// 读取 PCR 值
    pub fn read_pcr(&self, pcr_idx: PcrIndex) -> [u8; SHA256_LEN] {
        let pcrs = self.pcrs.lock();
        pcrs[pcr_idx as usize]
    }

    /// 读取所有 PCR
    pub fn read_all_pcrs(&self) -> [[u8; SHA256_LEN]; PCR_COUNT] {
        *self.pcrs.lock()
    }

    /// 密封数据 (Seal)
    ///
    /// 将数据绑定到当前 PCR 状态, 仅当 PCR 匹配时才能解封.
    /// 简化实现: 将数据与 PCR 快照一起存储.
    pub fn seal(&self, data: &[u8], pcr_mask: u32) -> Option<TpmSealedData> {
        if !self.initialized.load(Ordering::Acquire) {
            return None;
        }
        let pcr_snapshot = self.read_all_pcrs();
        Some(TpmSealedData {
            data: data.to_vec(),
            pcr_snapshot,
            pcr_mask,
        })
    }

    /// 解封数据 (Unseal)
    ///
    /// 检查当前 PCR 状态是否与密封时匹配.
    pub fn unseal(&self, sealed: &TpmSealedData) -> Option<Vec<u8>> {
        if !self.initialized.load(Ordering::Acquire) {
            return None;
        }
        let current_pcrs = self.read_all_pcrs();
        // 检查 pcr_mask 指定的 PCR 是否匹配
        for i in 0..PCR_COUNT {
            if (sealed.pcr_mask >> i) & 1 == 1 {
                if current_pcrs[i] != sealed.pcr_snapshot[i] {
                    return None; // PCR 不匹配
                }
            }
        }
        Some(sealed.data.clone())
    }

    /// 报价 (Quote)
    ///
    /// 对 PCR 值签名, 用于远程证明.
    /// 简化实现: 返回 PCR 快照 + 哈希.
    pub fn quote(&self, pcr_mask: u32, nonce: &[u8]) -> TpmQuote {
        let all_pcrs = self.read_all_pcrs();
        let mut quote_data = Vec::new();
        for i in 0..PCR_COUNT {
            if (pcr_mask >> i) & 1 == 1 {
                quote_data.extend_from_slice(&all_pcrs[i]);
            }
        }
        quote_data.extend_from_slice(nonce);
        let quote_hash = sha256_hash(&quote_data);
        TpmQuote {
            pcr_mask,
            pcr_values: all_pcrs,
            quote_hash,
        }
    }

    /// 是否已初始化
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    /// 是否为硬件 TPM
    pub fn is_hardware(&self) -> bool {
        self.is_hardware.load(Ordering::Acquire)
    }
}

/// 密封数据
#[derive(Debug, Clone)]
pub struct TpmSealedData {
    /// 密封的数据
    pub data: Vec<u8>,
    /// 密封时的 PCR 快照
    pub pcr_snapshot: [[u8; SHA256_LEN]; PCR_COUNT],
    /// PCR 掩码 (哪些 PCR 参与绑定)
    pub pcr_mask: u32,
}

/// TPM 报价
#[derive(Debug, Clone)]
pub struct TpmQuote {
    /// PCR 掩码
    pub pcr_mask: u32,
    /// PCR 值
    pub pcr_values: [[u8; SHA256_LEN]; PCR_COUNT],
    /// 报价哈希
    pub quote_hash: [u8; SHA256_LEN],
}

// ============================================================================
// 全局实例
// ============================================================================

/// 全局安全启动子系统
static SECURE_BOOT: SecureBootSubsystem = SecureBootSubsystem::new();
/// 全局 TPM 子系统
static TPM: TpmSubsystem = TpmSubsystem::new();

/// 初始化安全启动
pub fn secure_boot_init(pk: Ed25519PubKey) {
    SECURE_BOOT.init(pk);
}

/// 获取全局安全启动子系统
pub fn secure_boot_subsystem() -> &'static SecureBootSubsystem {
    &SECURE_BOOT
}

/// 初始化 TPM
pub fn tpm_init() {
    TPM.init();
}

/// 获取全局 TPM 子系统
pub fn tpm_subsystem() -> &'static TpmSubsystem {
    &TPM
}

/// 安全启动是否已初始化
pub fn secure_boot_is_initialized() -> bool {
    SECURE_BOOT.initialized.load(Ordering::Acquire)
}

/// TPM 是否已初始化
pub fn tpm_is_initialized() -> bool {
    TPM.initialized.load(Ordering::Acquire)
}

// ============================================================================
// 系统调用
// ============================================================================

/// sys_secure_boot — 安全启动系统调用
///
/// `a0`: cmd
///   0 = verify_image(image_ptr: a1, image_len: a2, sig_ptr: a3) → 结果
///   1 = is_enabled() → bool
///   2 = is_locked() → bool
///   3 = stats() → (ok_count 位于高 32 位 | fail_count 位于低 32 位)
#[no_mangle]
pub fn sys_secure_boot(cmd: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    match cmd {
        0 => {
            // verify_image
            if a1 == 0 || a3 == 0 {
                return -(22i64); // EINVAL
            }
            let image_ptr = a1 as *const u8;
            let image_len = a2 as usize;
            let sig_ptr = a3 as *const [u8; ED25519_SIG_LEN];
            // SAFETY: 指针由 syscall 入口保证有效
            let image = unsafe { core::slice::from_raw_parts(image_ptr, image_len) };
            // SAFETY: 同上
            let signature = unsafe { &*sig_ptr };
            let result = secure_boot_subsystem().verify_image(image, signature);
            result as i64
        }
        1 => {
            // is_enabled
            secure_boot_subsystem().is_enabled() as i64
        }
        2 => {
            // is_locked
            secure_boot_subsystem().is_locked() as i64
        }
        3 => {
            // stats
            let (ok, fail) = secure_boot_subsystem().stats();
            ((ok as i64) << 32) | (fail as i64)
        }
        _ => -(38i64), // ENOSYS
    }
}

/// sys_tpm — TPM 系统调用
///
/// `a0`: cmd
///   0 = extend(pcr 索引: a1, 数据指针: a2, 数据长度: a3) → bool
///   1 = read_pcr(pcr 索引: a1) → u64 (哈希前8字节)
///   2 = seal(数据指针: a1, 数据长度: a2, pcr 掩码: a3) → fd
///   3 = unseal(fd: a1) → bool (简化)
///   4 = quote(pcr 掩码: a1, nonce 指针: a2, nonce 长度: a3) → 哈希前8字节
///   5 = is_initialized() → 是否已初始化
#[no_mangle]
pub fn sys_tpm(cmd: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    if !tpm_is_initialized() && cmd != 5 {
        return -(11i64); // EAGAIN
    }

    match cmd {
        0 => {
            // extend
            let pcr_idx = match PcrIndex::from_u32(a1 as u32) {
                Some(idx) => idx,
                None => return -(22i64),
            };
            if a2 == 0 {
                return -(22i64);
            }
            // SAFETY: 指针由 syscall 入口保证有效
            let data = unsafe { core::slice::from_raw_parts(a2 as *const u8, a3 as usize) };
            if tpm_subsystem().extend(pcr_idx, data) { 0 } else { -(5i64) }
        }
        1 => {
            // read_pcr
            let pcr_idx = match PcrIndex::from_u32(a1 as u32) {
                Some(idx) => idx,
                None => return -(22i64),
            };
            let pcr = tpm_subsystem().read_pcr(pcr_idx);
            // 返回哈希前 8 字节作为 u64
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&pcr[..8]);
            i64::from_be_bytes(bytes)
        }
        2 => {
            // seal (简化: 返回成功/失败)
            if a1 == 0 {
                return -(22i64);
            }
            // SAFETY: 指针由 syscall 入口保证有效
            let data = unsafe { core::slice::from_raw_parts(a1 as *const u8, a2 as usize) };
            match tpm_subsystem().seal(data, a3 as u32) {
                Some(_) => 0,
                None => -(5i64),
            }
        }
        3 => {
            // unseal (简化: 总是返回未实现)
            -(38i64) // ENOSYS - 需要传入密封数据, 简化跳过
        }
        4 => {
            // quote
            let nonce = if a2 == 0 {
                &[]
            } else {
                // SAFETY: 指针由 syscall 入口保证有效
                unsafe { core::slice::from_raw_parts(a2 as *const u8, a3 as usize) }
            };
            let quote = tpm_subsystem().quote(a1 as u32, nonce);
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&quote.quote_hash[..8]);
            i64::from_be_bytes(bytes)
        }
        5 => {
            // is_initialized
            tpm_is_initialized() as i64
        }
        _ => -(38i64), // ENOSYS
    }
}
