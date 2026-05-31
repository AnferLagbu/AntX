use std::vec;
use std::vec::Vec;

pub const HV_RAIDZ_MIN_COLS: usize = 2;
pub const HV_RAIDZ_MAX_COLS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HvRaidzLevel {
    Single = 0,
    RaidZ1 = 1,
    RaidZ2 = 2,
    RaidZ3 = 3,
    Mirror = 4,
}

impl HvRaidzLevel {
    pub fn parity_cols(&self) -> usize {
        match self {
            Self::Single => 0,
            Self::RaidZ1 => 1,
            Self::RaidZ2 => 2,
            Self::RaidZ3 => 3,
            Self::Mirror => 0,
        }
    }

    pub fn max_failures(&self) -> usize {
        match self {
            Self::Single => 0,
            Self::RaidZ1 => 1,
            Self::RaidZ2 => 2,
            Self::RaidZ3 => 3,
            Self::Mirror => 1,
        }
    }
}

pub struct HvRaidzMap {
    pub level: HvRaidzLevel,
    pub ncols: usize,
    pub nparity: usize,
    pub ashift: u8,
    pub cols: Vec<HvRaidzCol>,
}

pub struct HvRaidzCol {
    pub col_id: usize,
    pub devidx: usize,
    pub offset: u64,
    pub size: usize,
    pub data: Vec<u8>,
    pub is_parity: bool,
}

impl HvRaidzMap {
    pub fn new(level: HvRaidzLevel, ncols: usize, ashift: u8) -> Self {
        let nparity = level.parity_cols();
        Self {
            level,
            ncols: ncols.clamp(HV_RAIDZ_MIN_COLS, HV_RAIDZ_MAX_COLS),
            nparity,
            ashift,
            cols: Vec::new(),
        }
    }

    pub fn data_cols(&self) -> usize {
        self.ncols.saturating_sub(self.nparity)
    }

    pub fn generate_parity(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        let unit_size = 4096;
        let data_cols = self.data_cols();
        if data_cols == 0 {
            return Vec::new();
        }
        let total_units = data.len().div_ceil(unit_size * data_cols);
        let mut result: Vec<Vec<u8>> = (0..self.ncols).map(|_| Vec::new()).collect();
        for unit_idx in 0..total_units {
            for col in 0..data_cols {
                let src_off = unit_idx * unit_size * data_cols + col * unit_size;
                let src_end = (src_off + unit_size).min(data.len());
                let src_len = src_end.saturating_sub(src_off);
                let mut chunk = vec![0u8; unit_size];
                if src_len > 0 {
                    chunk[..src_len].copy_from_slice(&data[src_off..src_end]);
                }
                result[self.nparity + col].extend_from_slice(&chunk);
            }
            for p in 0..self.nparity {
                let mut parity = vec![0u8; unit_size];
                for col in 0..data_cols {
                    let off =
                        (unit_idx * unit_size) + (self.nparity + col) * unit_size * total_units;
                    let _ = off;
                    let src = &result[self.nparity + col]
                        [unit_idx * unit_size..(unit_idx + 1) * unit_size];
                    for (i, byte) in src.iter().enumerate() {
                        parity[i] ^= byte;
                    }
                }
                result[p] = parity;
            }
        }
        result
    }

    pub fn reconstruct_data(
        &self,
        parity_data: &[Vec<u8>],
        failed_cols: &[usize],
    ) -> Option<Vec<u8>> {
        let _data_cols = self.data_cols();
        if failed_cols.len() > self.level.max_failures() {
            return None;
        }

        if self.level != HvRaidzLevel::Single && !failed_cols.is_empty() {
            return None;
        }

        if self.level == HvRaidzLevel::RaidZ1 && failed_cols.len() == 1 {
            let failed = failed_cols[0];
            if failed >= self.nparity && failed < self.ncols {
                let unit_size = 4096;
                let total_units = parity_data
                    .get(self.nparity)
                    .map(|d| d.len() / unit_size)
                    .unwrap_or(0);
                let mut reconstructed = vec![0u8; total_units * unit_size];
                for unit_idx in 0..total_units {
                    let mut block = vec![0u8; unit_size];
                    for col in 0..self.ncols {
                        if col == failed {
                            continue;
                        }
                        let src = &parity_data[col];
                        let src_start = unit_idx * unit_size;
                        let src_end = (src_start + unit_size).min(src.len());
                        if src_start < src.len() {
                            for (i, byte) in src[src_start..src_end].iter().enumerate() {
                                block[i] ^= byte;
                            }
                        }
                    }
                    let dst_start = unit_idx * unit_size;
                    let dst_end = (dst_start + unit_size).min(reconstructed.len());
                    reconstructed[dst_start..dst_end]
                        .copy_from_slice(&block[..dst_end - dst_start]);
                }
                let mut result = Vec::new();
                for col in self.nparity..self.ncols {
                    if col == failed {
                        result.extend_from_slice(&reconstructed);
                    } else {
                        result.extend_from_slice(&parity_data[col]);
                    }
                }
                return Some(result);
            }
        }

        let all_data = parity_data.to_vec();
        let mut result = Vec::new();
        for col in self.nparity..self.ncols {
            result.extend_from_slice(&all_data[col]);
        }
        Some(result)
    }

    pub fn verify_parity(&self, parity_data: &[Vec<u8>]) -> bool {
        let unit_size = 4096;
        let data_cols = self.data_cols();
        if parity_data.len() < self.ncols {
            return false;
        }
        for p in 0..self.nparity {
            let mut computed = vec![0u8; unit_size];
            for col in 0..data_cols {
                let src = &parity_data[self.nparity + col];
                let len = src.len().min(unit_size);
                for (i, byte) in src[..len].iter().enumerate() {
                    computed[i] ^= byte;
                }
            }
            let stored = &parity_data[p];
            let len = stored.len().min(unit_size);
            if computed[..len] != stored[..len] {
                return false;
            }
        }
        true
    }
}

pub struct HvRaidzEngine;

impl HvRaidzEngine {
    pub fn create_stripe(level: HvRaidzLevel, ncols: usize, ashift: u8) -> HvRaidzMap {
        HvRaidzMap::new(level, ncols, ashift)
    }

    pub fn scrub_block(map: &HvRaidzMap, parity_data: &[Vec<u8>]) -> HvScrubResult {
        if map.verify_parity(parity_data) {
            HvScrubResult::Clean
        } else {
            HvScrubResult::Corrupted {
                failed_cols: vec![0],
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum HvScrubResult {
    Clean,
    Corrupted { failed_cols: Vec<usize> },
    Repaired { repaired_cols: Vec<usize> },
    Unrepairable,
}
