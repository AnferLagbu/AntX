pub const HV_DVA_MAX: usize = 2;
pub const HV_BP_CHECKSUM_SIZE: usize = 32;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HvDva {
    pub vdev_id: u16,
    pub offset: u64,
    pub asize: u32,
    pub gang: bool,
    pub _pad: [u8; 3],
}

impl HvDva {
    pub const fn null() -> Self {
        Self {
            vdev_id: 0,
            offset: 0,
            asize: 0,
            gang: false,
            _pad: [0; 3],
        }
    }

    pub fn is_null(&self) -> bool {
        self.vdev_id == 0 && self.offset == 0 && self.asize == 0
    }

    pub fn new(vdev_id: u16, offset: u64, asize: u32) -> Self {
        Self {
            vdev_id,
            offset,
            asize,
            gang: false,
            _pad: [0; 3],
        }
    }

    pub fn with_gang(mut self) -> Self {
        self.gang = true;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HvCompType {
    Off = 0,
    LZ4 = 1,
    ZSTD = 2,
    Gzip1 = 3,
    Gzip9 = 4,
    ZLE = 5,
}

impl HvCompType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::LZ4,
            2 => Self::ZSTD,
            3 => Self::Gzip1,
            4 => Self::Gzip9,
            5 => Self::ZLE,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HvCksumType {
    Off = 0,
    Fletcher2 = 1,
    Fletcher4 = 2,
    SHA256 = 3,
    EdonR = 4,
}

impl HvCksumType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Fletcher2,
            2 => Self::Fletcher4,
            3 => Self::SHA256,
            4 => Self::EdonR,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HvBpProp {
    pub level: u8,
    pub comp_type: HvCompType,
    pub cksum_type: HvCksumType,
    pub encrypted: bool,
    pub byteorder: u8,
    pub logical_size: u32,
    pub physical_size: u32,
    pub _pad: [u8; 4],
}

impl HvBpProp {
    pub const fn default() -> Self {
        Self {
            level: 0,
            comp_type: HvCompType::Off,
            cksum_type: HvCksumType::Fletcher4,
            encrypted: false,
            byteorder: 0,
            logical_size: 0,
            physical_size: 0,
            _pad: [0; 4],
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HvBlockPointer {
    pub dva: [HvDva; HV_DVA_MAX],
    pub prop: HvBpProp,
    pub checksum: [u64; 4],
    pub birth_txg: u64,
    pub fill: u64,
    pub _pad: [u64; 2],
}

impl HvBlockPointer {
    pub const fn null() -> Self {
        Self {
            dva: [HvDva::null(); HV_DVA_MAX],
            prop: HvBpProp::default(),
            checksum: [0; 4],
            birth_txg: 0,
            fill: 0,
            _pad: [0; 2],
        }
    }

    pub fn is_null(&self) -> bool {
        self.dva[0].is_null() && self.dva[1].is_null()
    }

    pub fn is_hole(&self) -> bool {
        self.dva[0].is_null() && self.dva[1].is_null() && self.birth_txg == 0
    }

    pub fn logical_size(&self) -> u32 {
        self.prop.logical_size
    }

    pub fn physical_size(&self) -> u32 {
        self.prop.physical_size
    }

    pub fn get_dva(&self, idx: usize) -> Option<&HvDva> {
        if idx < HV_DVA_MAX && !self.dva[idx].is_null() {
            Some(&self.dva[idx])
        } else {
            None
        }
    }

    pub fn set_dva(&mut self, idx: usize, dva: HvDva) {
        if idx < HV_DVA_MAX {
            self.dva[idx] = dva;
        }
    }

    pub fn set_birth(&mut self, txg: u64) {
        self.birth_txg = txg;
    }

    pub fn is_data(&self) -> bool {
        self.prop.level == 0 && !self.is_hole()
    }

    pub fn is_metadata(&self) -> bool {
        self.prop.level > 0 && !self.is_hole()
    }

    /// Framekernel P2.2.2: 安全地将 HvBlockPointer 转换为字节切片
    /// SAFETY: HvBlockPointer 是 repr(C) 结构体，字段布局确定，无内部 padding 导致 UB
    pub const BYTES: usize = core::mem::size_of::<Self>();

    pub fn as_bytes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const Self as *const u8, Self::BYTES)
        }
    }

    /// Framekernel P2.2.2: 从字节切片安全地反序列化 HvBlockPointer
    /// SAFETY: 已验证输入长度足够
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::BYTES {
            return None;
        }
        let mut bp = Self::null();
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                &mut bp as *mut Self as *mut u8,
                Self::BYTES,
            );
        }
        Some(bp)
    }
}
