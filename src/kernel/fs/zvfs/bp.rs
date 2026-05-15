use core::mem::size_of;

pub const ZV_DVA_MAX: usize = 2;
pub const ZV_BP_CHECKSUM_SIZE: usize = 32;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ZvDva {
    pub vdev_id: u16,
    pub offset: u64,
    pub asize: u32,
    pub gang: bool,
    pub _pad: [u8; 3],
}

impl ZvDva {
    pub const fn null() -> Self {
        Self { vdev_id: 0, offset: 0, asize: 0, gang: false, _pad: [0; 3] }
    }

    pub fn is_null(&self) -> bool {
        self.vdev_id == 0 && self.offset == 0 && self.asize == 0
    }

    pub fn new(vdev_id: u16, offset: u64, asize: u32) -> Self {
        Self { vdev_id, offset, asize, gang: false, _pad: [0; 3] }
    }

    pub fn with_gang(mut self) -> Self {
        self.gang = true;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ZvCompType {
    Off = 0,
    LZ4 = 1,
    ZSTD = 2,
    Gzip1 = 3,
    Gzip9 = 4,
    ZLE = 5,
}

impl ZvCompType {
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
pub enum ZvCksumType {
    Off = 0,
    Fletcher2 = 1,
    Fletcher4 = 2,
    SHA256 = 3,
    EdonR = 4,
}

impl ZvCksumType {
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
pub struct ZvBpProp {
    pub level: u8,
    pub comp_type: ZvCompType,
    pub cksum_type: ZvCksumType,
    pub encrypted: bool,
    pub byteorder: u8,
    pub logical_size: u32,
    pub physical_size: u32,
    pub _pad: [u8; 4],
}

impl ZvBpProp {
    pub const fn default() -> Self {
        Self {
            level: 0,
            comp_type: ZvCompType::Off,
            cksum_type: ZvCksumType::Fletcher4,
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
pub struct ZvBlockPointer {
    pub dva: [ZvDva; ZV_DVA_MAX],
    pub prop: ZvBpProp,
    pub checksum: [u64; 4],
    pub birth_txg: u64,
    pub fill: u64,
    pub _pad: [u64; 2],
}

impl ZvBlockPointer {
    pub const fn null() -> Self {
        Self {
            dva: [ZvDva::null(); ZV_DVA_MAX],
            prop: ZvBpProp::default(),
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

    pub fn get_dva(&self, idx: usize) -> Option<&ZvDva> {
        if idx < ZV_DVA_MAX && !self.dva[idx].is_null() {
            Some(&self.dva[idx])
        } else {
            None
        }
    }

    pub fn set_dva(&mut self, idx: usize, dva: ZvDva) {
        if idx < ZV_DVA_MAX {
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
}
