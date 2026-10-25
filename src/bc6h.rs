//! BC6H (high dynamic range, 16-bit half floats), unsigned (`UF16`) and signed (`SF16`).
//!
//! Sources: Khronos Data Format Specification 1.3.1 (DFS) section 20.2 "BC6H" (with Tables 114,
//! 119 and 121 to 124) and Microsoft's "BC6H Format" (D3D11 BC6H). The bit layout of every mode
//! (Tables 122 to 124) is generated into `tables.rs` by `tools/gen_tables.py`.
//!
//! # Where this decoder follows the specification and `texture2ddecoder` does not
//!
//! * Mode 15 has 16-bit endpoints, so the wrap-around of the delta transform must mask with
//!   `0xFFFF` (DFS 20.2: "wrapped at the number of endpoint bits"). A `1u16 << 16` mask is zero.
//! * Unquantizing maps only the all-ones endpoint to the maximum (DFS 20.2 pseudocode:
//!   `x == (1 << EPB) - 1`), not the half-way value `1 << (EPB - 1)`.
//! * Signed endpoints are two's complement throughout; the unquantizer takes the magnitude and
//!   restores the sign (DFS 20.2).
//!
//! The output is the bit pattern of an IEEE 754 half float (`u16`) per channel. Alpha is always
//! 1.0. Reserved modes (19, 23, 27, 31) and "invalid" blocks decode to (0.0, 0.0, 0.0, 1.0).

use crate::tables::{Bc6hRun, ANCHOR2, BC6H_MODE_NUMBERS, BC6H_RUNS, PARTITION2};

/// Half-float bit pattern of 1.0 (the alpha of every BC6H texel).
pub const HALF_ONE: u16 = 0x3C00;

/// Per-mode parameters, DFS Table 121. Index = position in [`BC6H_MODE_NUMBERS`].
struct ModeInfo {
    /// "Transformed endpoints": E1..E3 are deltas from E0.
    transformed: bool,
    /// Two subsets with a 5-bit partition number; otherwise one subset.
    partition: bool,
    /// Bits per endpoint channel (EPB in the pseudocode).
    endpoint_bits: u32,
    /// Bits of the R, G and B deltas (zero when not transformed).
    delta_bits: [u32; 3],
}

const fn mode(transformed: bool, partition: bool, endpoint_bits: u32, d: [u32; 3]) -> ModeInfo {
    ModeInfo {
        transformed,
        partition,
        endpoint_bits,
        delta_bits: d,
    }
}

// Same order as BC6H_MODE_NUMBERS: 0, 1, 2, 6, 10, 14, 18, 22, 26, 30, 3, 7, 11, 15
static MODES: [ModeInfo; 14] = [
    mode(true, true, 10, [5, 5, 5]),
    mode(true, true, 7, [6, 6, 6]),
    mode(true, true, 11, [5, 4, 4]),
    mode(true, true, 11, [4, 5, 4]),
    mode(true, true, 11, [4, 4, 5]),
    mode(true, true, 9, [5, 5, 5]),
    mode(true, true, 8, [6, 5, 5]),
    mode(true, true, 8, [5, 6, 5]),
    mode(true, true, 8, [5, 5, 6]),
    mode(false, true, 6, [0, 0, 0]),
    mode(false, false, 10, [0, 0, 0]),
    mode(true, false, 11, [9, 9, 9]),
    mode(true, false, 12, [8, 8, 8]),
    mode(true, false, 16, [4, 4, 4]),
];

/// Interpolation weights, DFS Table 119 (the three-bit row; BC6H uses 3-bit indices with two
/// subsets and 4-bit indices with one).
const WEIGHTS3: [i32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const WEIGHTS4: [i32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

/// Sign-extends the low `bits` bits of `v`.
#[inline]
fn sign_extend(v: i32, bits: u32) -> i32 {
    let s = 32 - bits;
    (v << s) >> s
}

/// Expands an endpoint of `bits` bits to the 16-bit working range (DFS 20.2).
///
/// Unsigned: 0..=0xFFFF. Signed: -0x7FFF..=0x7FFF (or the raw 16-bit value for `bits >= 16`).
#[inline]
fn unquantize(x: i32, signed: bool, bits: u32) -> i32 {
    if !signed {
        if bits >= 15 {
            x
        } else if x == 0 {
            0
        } else if x == (1 << bits) - 1 {
            0xFFFF
        } else {
            ((x << 15) + 0x4000) >> (bits - 1)
        }
    } else if bits >= 16 {
        x
    } else {
        let (neg, m) = if x < 0 { (true, -x) } else { (false, x) };
        let unq = if m == 0 {
            0
        } else if m >= (1 << (bits - 1)) - 1 {
            0x7FFF
        } else {
            ((m << 15) + 0x4000) >> (bits - 1)
        };
        if neg {
            -unq
        } else {
            unq
        }
    }
}

/// Maps an interpolated value to the half-float bit pattern (the last step of the pseudocode).
///
/// For the signed format the magnitude is scaled by 31/32 first and the sign is applied after
/// that, as in `finish_unquantize` of Microsoft's "BC6H Format" (D3D11 BC6H: "scale the magnitude
/// by 31/32", then `if (comp < 0)` sets the sign bit). The DFS 20.2 text sets the sign from the
/// interpolated value instead (`i < 0 ? (((-i) * 31) >> 5) | 0x8000`), which differs only for
/// `i == -1`: the scaled magnitude is 0 and DFS gives negative zero (0x8000) where Direct3D gives
/// positive zero (0x0000). Direct3D requires hardware to be bit-exact with its functions ("All
/// hardware decompressors are required to return bit-accurate results"), so this follows Direct3D.
#[inline]
fn finish(i: i32, signed: bool) -> u16 {
    if !signed {
        // i is in 0..=0xFFFF here; scaling by 31/64 limits the result to 0x7BFF (65504.0)
        ((i as u32 * 31) >> 6) as u16
    } else {
        let magnitude = ((i.unsigned_abs() * 31) >> 5) as u16;
        if i < 0 && magnitude != 0 {
            magnitude | 0x8000
        } else {
            magnitude
        }
    }
}

/// What the header of a BC6H block holds, before unquantizing and interpolating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct BlockInfo {
    /// The mode number (0 to 18, the values of DFS Table 121).
    pub mode: u8,
    /// Bits per endpoint channel (EPB in the DFS pseudocode).
    pub endpoint_bits: u32,
    /// True if endpoints 1 to 3 were stored as deltas from endpoint 0.
    pub transformed: bool,
    /// The partition number (0 to 31) of a two-subset mode, `None` for a one-subset mode.
    pub partition: Option<u8>,
    /// The quantized endpoints as `[endpoint][r, g, b]`, with the delta transform undone and, for
    /// the signed format, sign-extended. Endpoints 2 and 3 are only used by two-subset modes.
    pub endpoints: [[i32; 3]; 4],
}

/// Reads the mode and the endpoints of a block. `None` for the reserved modes 19, 23, 27 and 31.
pub fn parse_block(block: &[u8; 16], signed: bool) -> Option<BlockInfo> {
    let v = u128::from_le_bytes(*block);

    // The mode is in the low 2 bits if they are less than 2, else in the low 5 bits (DFS 20.2)
    let low2 = (v & 3) as u8;
    let mode_number = if low2 < 2 { low2 } else { (v & 31) as u8 };
    let mi = BC6H_MODE_NUMBERS.iter().position(|&m| m == mode_number)?;
    let info = &MODES[mi];

    // Gather the endpoint bits: slot = channel * 4 + endpoint (R0..R3, G0..G3, B0..B3)
    let mut raw = [0i32; 12];
    for Bc6hRun {
        src,
        len,
        slot,
        dst,
    } in BC6H_RUNS[mi].iter().copied()
    {
        let bits = (v.checked_shr(src as u32).unwrap_or(0) & ((1u128 << len) - 1)) as i32;
        raw[slot as usize] |= bits << dst;
    }

    let n_ep = if info.partition { 4 } else { 2 };
    let epb = info.endpoint_bits;
    let mut ep = [[0i32; 3]; 4]; // [endpoint][channel]
    for ch in 0..3 {
        // E0 is sign-extended if the format is signed (DFS 20.2)
        let e0 = if signed {
            sign_extend(raw[ch * 4], epb)
        } else {
            raw[ch * 4]
        };
        ep[0][ch] = e0;
        for (k, e) in ep.iter_mut().enumerate().take(n_ep).skip(1) {
            let mut x = raw[ch * 4 + k];
            if info.transformed {
                // E1..E3 are sign-extended two's complement deltas from E0, wrapped at EPB bits
                // ("R1 = (R0 + R1) & ((1 << EPB) - 1)", DFS 20.2)
                x = sign_extend(x, info.delta_bits[ch]);
                x = (e0 + x) & ((1 << epb) - 1);
                if signed {
                    x = sign_extend(x, epb);
                }
            } else if signed {
                x = sign_extend(x, epb);
            }
            e[ch] = x;
        }
    }
    Some(BlockInfo {
        mode: mode_number,
        endpoint_bits: epb,
        transformed: info.transformed,
        partition: info.partition.then_some(((v >> 77) & 31) as u8),
        endpoints: ep,
    })
}

/// Decodes a BC6H block to sixteen RGB texels in raster order, each channel a half-float bit
/// pattern. `signed` selects SF16 instead of UF16.
pub fn decode_block_half(block: &[u8; 16], signed: bool) -> [[u16; 3]; 16] {
    let Some(info) = parse_block(block, signed) else {
        return [[0, 0, 0]; 16]; // reserved mode: (0, 0, 0, 1)
    };
    let v = u128::from_le_bytes(*block);
    let two_subsets = info.partition.is_some();
    let n_ep = if two_subsets { 4 } else { 2 };
    let epb = info.endpoint_bits;

    // Unquantize
    let mut unq = [[0i32; 3]; 4];
    for (u, e) in unq.iter_mut().zip(info.endpoints.iter()).take(n_ep) {
        for (x, &q) in u.iter_mut().zip(e.iter()) {
            *x = unquantize(q, signed, epb);
        }
    }

    // Partition and indices: two subsets have 3-bit indices from bit 82, one subset has 4-bit
    // indices from bit 65 (DFS 20.2, Tables 123 and 124)
    let (part, index_bits, index_pos, weights): (usize, u32, u32, &[i32]) = match info.partition {
        Some(p) => (p as usize, 3, 82, &WEIGHTS3),
        None => (0, 4, 65, &WEIGHTS4),
    };
    let anchor1 = if two_subsets {
        ANCHOR2[part] as usize
    } else {
        usize::MAX
    };

    let mut out = [[0u16; 3]; 16];
    let mut pos = index_pos;
    for (i, px) in out.iter_mut().enumerate() {
        // The anchor texel of each subset loses its top index bit (DFS 20.1: "the highest bit for
        // that index is guaranteed to be zero - and not storing that bit"); texel 0 anchors
        // subset 0, and ANCHOR2 gives the anchor of subset 1.
        let nbits = if i == 0 || i == anchor1 {
            index_bits - 1
        } else {
            index_bits
        };
        let idx = (v.checked_shr(pos).unwrap_or(0) & ((1u128 << nbits) - 1)) as usize;
        pos += nbits;
        let subset = if two_subsets {
            PARTITION2[part][i] as usize
        } else {
            0
        };
        let w = weights[idx];
        let (a, b) = (&unq[subset * 2], &unq[subset * 2 + 1]);
        for ch in 0..3 {
            // Equation 2: ((64 - w) * E0 + w * E1 + 32) >> 6, an arithmetic shift for negatives
            let interp = (a[ch] * (64 - w) + b[ch] * w + 32) >> 6;
            px[ch] = finish(interp, signed);
        }
    }
    out
}

/// Converts a half-float bit pattern to `f32` (exact; infinities and NaNs are kept).
pub fn half_to_f32(h: u16) -> f32 {
    let sign = ((h as u32) & 0x8000) << 16;
    let exp = ((h >> 10) & 0x1F) as u32;
    let man = (h & 0x3FF) as u32;
    let bits = if exp == 0 {
        if man == 0 {
            sign
        } else {
            // subnormal: man * 2^-24, renormalized
            let shift = man.leading_zeros() - 21; // leading zeros within the 10-bit field
            let man = (man << shift) & 0x3FF;
            let exp = 127 - 15 + 1 - shift;
            sign | (exp << 23) | (man << 13)
        }
    } else if exp == 31 {
        sign | 0x7F80_0000 | (man << 13)
    } else {
        sign | ((exp + 127 - 15) << 23) | (man << 13)
    };
    f32::from_bits(bits)
}

/// Decodes a BC6H block to `f32` RGBA texels (alpha 1.0), raster order.
pub fn decode_block_f32(block: &[u8; 16], signed: bool) -> [[f32; 4]; 16] {
    let h = decode_block_half(block, signed);
    let mut out = [[0.0, 0.0, 0.0, 1.0]; 16];
    for (o, p) in out.iter_mut().zip(h.iter()) {
        o[0] = half_to_f32(p[0]);
        o[1] = half_to_f32(p[1]);
        o[2] = half_to_f32(p[2]);
    }
    out
}

/// Converts a half float to 8 bits: clamped to [0, 1] (negative values, NaN and -infinity give 0,
/// +infinity gives 255), multiplied by 255 and **rounded down**.
///
/// BC6H defines no 8-bit form; this convenience truncates like `texture2ddecoder` (our previous
/// dependency) does, so that existing images do not change. Use the half or `f32` output for
/// anything that needs the HDR range or round-to-nearest.
pub fn half_to_unorm8(h: u16) -> u8 {
    if h >= 0x7C00 {
        // +infinity clamps to 1; NaN (0x7C01..=0x7FFF) and everything with the sign bit set give 0
        return if h == 0x7C00 { 255 } else { 0 };
    }
    if h >= 0x3C00 {
        return 255; // 1.0 and above
    }
    let exp = (h >> 10) as u32;
    let man = (h & 0x3FF) as u32;
    // value = (1024 + man) * 2^(exp - 25) for a normal number, man * 2^-24 for a subnormal one;
    // times 255 and rounded down (the product has at most 19 significant bits, so a float
    // multiplication would give the same result)
    if exp == 0 {
        ((man * 255) >> 24) as u8
    } else {
        (((1024 + man) * 255) >> (25 - exp)) as u8
    }
}

/// Decodes a BC6H block to RGBA8 texels (alpha 255): each channel is clamped to [0, 1], so
/// everything above 1.0 is lost. No tone mapping and no gamma are applied.
pub fn decode_block_rgba8(block: &[u8; 16], signed: bool) -> [[u8; 4]; 16] {
    let h = decode_block_half(block, signed);
    let mut out = [[0, 0, 0, 255]; 16];
    for (o, p) in out.iter_mut().zip(h.iter()) {
        o[0] = half_to_unorm8(p[0]);
        o[1] = half_to_unorm8(p[1]);
        o[2] = half_to_unorm8(p[2]);
    }
    out
}
