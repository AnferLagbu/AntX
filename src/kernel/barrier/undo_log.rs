use super::types::{UndoEntry, MAX_UNDO_ENTRIES};

pub fn fnv1a_32(data: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

pub struct UndoLog {
    pub entries: [UndoEntry; MAX_UNDO_ENTRIES],
    pub count: usize,
    pub current_generation: u64,
}

unsafe impl Send for UndoLog {}
unsafe impl Sync for UndoLog {}

impl UndoLog {
    pub fn new() -> Self {
        Self {
            entries: [UndoEntry {
                generation: 0, field_ptr: core::ptr::null_mut(),
                old_value: [0u8; 8], value_size: 0, checksum: 0,
            }; MAX_UNDO_ENTRIES],
            count: 0,
            current_generation: 0,
        }
    }

    pub fn record<T: Copy>(&mut self, field: *mut T, old_value: T) {
        let field_ptr = field as *mut u8;
        let size = core::mem::size_of::<T>().min(8);

        if self.count > 0 {
            let last = &self.entries[self.count - 1];
            if last.field_ptr == field_ptr && last.generation == self.current_generation {
                return;
            }
        }

        for i in (0..self.count).rev() {
            if self.entries[i].generation < self.current_generation {
                break;
            }
            if self.entries[i].field_ptr == field_ptr {
                return;
            }
        }

        if self.count >= MAX_UNDO_ENTRIES {
            self.emergency_compact(self.current_generation.saturating_sub(1));
        }

        let raw = unsafe {
            core::slice::from_raw_parts(
                &old_value as *const T as *const u8, size,
            )
        };
        let mut old_bytes = [0u8; 8];
        old_bytes[..size].copy_from_slice(&raw[..size]);

        let checksum = fnv1a_32(&old_bytes[..size]);

        self.entries[self.count] = UndoEntry {
            generation: self.current_generation,
            field_ptr,
            old_value: old_bytes,
            value_size: size as u8,
            checksum,
        };
        self.count += 1;
    }

    pub fn rollback_to(&mut self, target_generation: u64) -> usize {
        let mut rolled_back = 0;
        while self.count > 0 {
            let entry = &self.entries[self.count - 1];
            if entry.generation < target_generation { break; }

            let size = entry.value_size as usize;
            let current_checksum = unsafe {
                fnv1a_32(core::slice::from_raw_parts(entry.field_ptr, size))
            };

            if current_checksum == entry.checksum {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        entry.old_value.as_ptr(), entry.field_ptr, size,
                    );
                }
            }

            self.count -= 1;
            rolled_back += 1;
        }
        if self.count > MAX_UNDO_ENTRIES / 2 { self.compact(); }
        rolled_back
    }

    fn emergency_compact(&mut self, keep_gen: u64) {
        let mut write = 0;
        let mut seen_ptrs = [false; 64];
        for i in (0..self.count).rev() {
            if self.entries[i].generation < keep_gen {
                let ptr_hash = (self.entries[i].field_ptr as usize) % 64;
                if !seen_ptrs[ptr_hash] {
                    seen_ptrs[ptr_hash] = true;
                    self.entries[write] = self.entries[i];
                    write += 1;
                }
            } else {
                self.entries[write] = self.entries[i];
                write += 1;
            }
        }

        let mut w = 0;
        for i in (0..write).rev() {
            self.entries[w] = self.entries[i];
            w += 1;
        }
        self.count = w;
    }

    fn compact(&mut self) {
        if self.count == 0 { return; }
        let mut gen_starts: [u64; 8] = [0; 8];
        let mut gen_count = 0;
        let mut prev_gen = u64::MAX;
        for i in 0..self.count {
            if self.entries[i].generation != prev_gen {
                if gen_count < 8 {
                    gen_starts[gen_count] = i as u64;
                    gen_count += 1;
                }
                prev_gen = self.entries[i].generation;
            }
        }
        if gen_count > 4 {
            let keep_from = gen_starts[gen_count - 4] as usize;
            let new_count = self.count - keep_from;
            for i in 0..new_count {
                self.entries[i] = self.entries[keep_from + i];
            }
            self.count = new_count;
        }
    }
}
