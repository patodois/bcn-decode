#![allow(dead_code)]

#[rustfmt::skip]
pub mod spec_tables;

/// xorshift64*: a fixed seed makes every run reproducible.
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    pub fn fill(&mut self, buf: &mut [u8]) {
        for c in buf.chunks_mut(8) {
            let v = self.next_u64().to_le_bytes();
            c.copy_from_slice(&v[..c.len()]);
        }
    }

    pub fn block16(&mut self) -> [u8; 16] {
        let mut b = [0u8; 16];
        self.fill(&mut b);
        b
    }
}

/// Sets `n` bits of a 128-bit block starting at bit `pos` (LSB first).
pub fn put(v: &mut u128, pos: u32, n: u32, value: u128) {
    let mask = if n >= 128 {
        u128::MAX
    } else {
        (1u128 << n) - 1
    };
    *v &= !(mask << pos);
    *v |= (value & mask) << pos;
}

/// Reads `n` bits of a 128-bit block starting at bit `pos`.
pub fn get(v: u128, pos: u32, n: u32) -> u128 {
    (v >> pos) & ((1u128 << n) - 1)
}
