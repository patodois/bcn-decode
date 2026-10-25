//! BC4 and BC5 (the RGTC formats; ATI1 and ATI2 in DDS files).
//!
//! Sources: Khronos Data Format Specification 1.3.1 (DFS) section 19 "RGTC Compressed Texture
//! Image Formats" (19.1 BC4 unsigned, 19.2 BC4 signed, 19.3 BC5 unsigned, 19.4 BC5 signed), and
//! Microsoft's "Block Compression (Direct3D 10)" for BC4/BC5.
//!
//! A BC4 block is 8 bytes: `red0`, `red1`, then sixteen 3-bit codes (48 bits, little-endian, texel
//! (x, y) at bits `3 * (4y + x)`). A BC5 block is two BC4 blocks: red, then green.
//!
//! # 8-bit rounding
//!
//! The specification gives real values such as `(6 * RED0 + RED1) / 7`. The unsigned 8-bit form
//! here computes `(6 * red0 + red1) / 7` with integer division, which rounds down (the convention
//! `texture2ddecoder` also uses; the exact value is at most 1 level above the result). The
//! signed 8-bit form truncates towards zero the same way. [`decode_unorm_block_f32`] and
//! [`decode_snorm_block_f32`] give the values of the specification without any 8-bit rounding.

/// The three index bits of texel `i` (DFS 19.1: `code(x, y) = bits[3 * (4y + x) + 2 .. 3 * (4y + x)]`).
#[inline]
fn code(bits: u64, i: usize) -> usize {
    ((bits >> (3 * i)) & 7) as usize
}

#[inline]
fn index_bits(block: &[u8; 8]) -> u64 {
    u64::from_le_bytes(*block) >> 16
}

/// Unsigned 8-bit palette (DFS 19.1, Table 108).
#[inline]
fn unorm_palette(r0: u8, r1: u8) -> [u8; 8] {
    let (a, b) = (r0 as u16, r1 as u16);
    let mut p = [r0, r1, 0, 0, 0, 0, 0, 0];
    if r0 > r1 {
        // six interpolated values: (6 r0 + r1) / 7 ... (r0 + 6 r1) / 7
        for i in 2..8u16 {
            p[i as usize] = (((8 - i) * a + (i - 1) * b) / 7) as u8;
        }
    } else {
        // four interpolated values, then REDmin = 0 and REDmax = 255
        for i in 2..6u16 {
            p[i as usize] = (((6 - i) * a + (i - 1) * b) / 5) as u8;
        }
        p[6] = 0;
        p[7] = 255;
    }
    p
}

/// Decodes one unsigned BC4 block to sixteen 8-bit values in raster order.
pub fn decode_unorm_block(block: &[u8; 8]) -> [u8; 16] {
    let pal = unorm_palette(block[0], block[1]);
    let bits = index_bits(block);
    let mut out = [0u8; 16];
    for (i, v) in out.iter_mut().enumerate() {
        *v = pal[code(bits, i)];
    }
    out
}

/// Signed palette (DFS 19.2). `red0` and `red1` are two's complement; -128 stands for -1.0 like
/// -127, so it is clamped to -127 in the values (but not in the `red0 > red1` comparison, which
/// uses the raw bytes; the specification calls the case `red0 = -127`, `red1 = -128` undefined).
#[inline]
fn snorm_palette(r0: i8, r1: i8) -> [i8; 8] {
    let (a, b) = (r0.max(-127) as i16, r1.max(-127) as i16);
    let mut p = [a as i8, b as i8, 0, 0, 0, 0, 0, 0];
    if r0 > r1 {
        for i in 2..8i16 {
            p[i as usize] = (((8 - i) * a + (i - 1) * b) / 7) as i8;
        }
    } else {
        for i in 2..6i16 {
            p[i as usize] = (((6 - i) * a + (i - 1) * b) / 5) as i8;
        }
        p[6] = -127; // REDmin = -1.0
        p[7] = 127; // REDmax = 1.0
    }
    p
}

/// Decodes one signed BC4 block to sixteen values in -127..=127 (1.0 is 127, -1.0 is -127).
pub fn decode_snorm_block(block: &[u8; 8]) -> [i8; 16] {
    let pal = snorm_palette(block[0] as i8, block[1] as i8);
    let bits = index_bits(block);
    let mut out = [0i8; 16];
    for (i, v) in out.iter_mut().enumerate() {
        *v = pal[code(bits, i)];
    }
    out
}

/// The weights of Table 108 as numerators over `den`: value = (w0 * RED0 + w1 * RED1) / den.
#[inline]
fn weights(a_gt_b: bool, idx: usize) -> (u32, u32, u32) {
    match (a_gt_b, idx) {
        (_, 0) => (1, 0, 1),
        (_, 1) => (0, 1, 1),
        (true, i) => ((8 - i) as u32, (i - 1) as u32, 7),
        (false, i @ 2..=5) => ((6 - i) as u32, (i - 1) as u32, 5),
        (false, _) => (0, 0, 1), // 6 and 7 are the constants, handled by the caller
    }
}

/// Decodes one unsigned BC4 block to the exact values of DFS 19.1 as `f32` in 0.0..=1.0
/// (each is one correctly rounded division of an integer by an integer).
pub fn decode_unorm_block_f32(block: &[u8; 8]) -> [f32; 16] {
    let (r0, r1) = (block[0], block[1]);
    let gt = r0 > r1;
    let bits = index_bits(block);
    let mut out = [0f32; 16];
    for (i, v) in out.iter_mut().enumerate() {
        let c = code(bits, i);
        *v = if !gt && c >= 6 {
            if c == 6 {
                0.0
            } else {
                1.0
            }
        } else {
            let (w0, w1, den) = weights(gt, c);
            (w0 * r0 as u32 + w1 * r1 as u32) as f32 / (den * 255) as f32
        };
    }
    out
}

/// Decodes one signed BC4 block to the exact values of DFS 19.2 as `f32` in -1.0..=1.0.
pub fn decode_snorm_block_f32(block: &[u8; 8]) -> [f32; 16] {
    let (r0, r1) = (block[0] as i8, block[1] as i8);
    let (a, b) = (r0.max(-127) as i32, r1.max(-127) as i32);
    let gt = r0 > r1;
    let bits = index_bits(block);
    let mut out = [0f32; 16];
    for (i, v) in out.iter_mut().enumerate() {
        let c = code(bits, i);
        *v = if !gt && c >= 6 {
            if c == 6 {
                -1.0
            } else {
                1.0
            }
        } else {
            let (w0, w1, den) = weights(gt, c);
            (w0 as i32 * a + w1 as i32 * b) as f32 / (den * 127) as f32
        };
    }
    out
}

/// BC4 unsigned to RGBA8 texels `(R, 0, 0, 255)` (DFS 19.1: "the resulting RGBA value for the texel
/// is (R, 0, 0, 1)").
pub fn decode_bc4_block(block: &[u8; 8]) -> [[u8; 4]; 16] {
    let pal = unorm_palette(block[0], block[1]);
    let bits = index_bits(block);
    let mut out = [[0, 0, 0, 255]; 16];
    for (i, px) in out.iter_mut().enumerate() {
        px[0] = pal[code(bits, i)];
    }
    out
}

/// BC4 signed to texels of an `R8G8B8A8_SNORM` image: `(R, 0, 0, 127)` as two's complement bytes.
pub fn decode_bc4_snorm_block(block: &[u8; 8]) -> [[u8; 4]; 16] {
    let r = decode_snorm_block(block);
    let mut out = [[0, 0, 0, 127]; 16];
    for (px, &v) in out.iter_mut().zip(r.iter()) {
        px[0] = v as u8;
    }
    out
}

/// BC5 unsigned to RGBA8 texels `(R, G, 0, 255)` (DFS 19.3).
pub fn decode_bc5_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let (r, g) = split(block);
    let (pr, pg) = (unorm_palette(r[0], r[1]), unorm_palette(g[0], g[1]));
    let (br, bg) = (index_bits(r), index_bits(g));
    let mut out = [[0, 0, 0, 255]; 16];
    for (i, px) in out.iter_mut().enumerate() {
        px[0] = pr[code(br, i)];
        px[1] = pg[code(bg, i)];
    }
    out
}

/// BC5 signed to texels of an `R8G8B8A8_SNORM` image: `(R, G, 0, 127)` as two's complement bytes.
pub fn decode_bc5_snorm_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let (r, g) = split(block);
    let r = decode_snorm_block(r);
    let g = decode_snorm_block(g);
    let mut out = [[0, 0, 0, 127]; 16];
    for i in 0..16 {
        out[i][0] = r[i] as u8;
        out[i][1] = g[i] as u8;
    }
    out
}

#[inline]
fn split(block: &[u8; 16]) -> (&[u8; 8], &[u8; 8]) {
    let (a, b) = block.split_at(8);
    (
        a.try_into().unwrap_or(&[0; 8]),
        b.try_into().unwrap_or(&[0; 8]),
    )
}

/// BC4 unsigned to `f32` texels `(R, 0, 0, 1)`.
pub fn decode_bc4_block_f32(block: &[u8; 8]) -> [[f32; 4]; 16] {
    let r = decode_unorm_block_f32(block);
    let mut out = [[0.0, 0.0, 0.0, 1.0]; 16];
    for (px, &v) in out.iter_mut().zip(r.iter()) {
        px[0] = v;
    }
    out
}

/// BC4 signed to `f32` texels `(R, 0, 0, 1)`.
pub fn decode_bc4_snorm_block_f32(block: &[u8; 8]) -> [[f32; 4]; 16] {
    let r = decode_snorm_block_f32(block);
    let mut out = [[0.0, 0.0, 0.0, 1.0]; 16];
    for (px, &v) in out.iter_mut().zip(r.iter()) {
        px[0] = v;
    }
    out
}

/// BC5 unsigned to `f32` texels `(R, G, 0, 1)`.
pub fn decode_bc5_block_f32(block: &[u8; 16]) -> [[f32; 4]; 16] {
    let (r, g) = split(block);
    let r = decode_unorm_block_f32(r);
    let g = decode_unorm_block_f32(g);
    let mut out = [[0.0, 0.0, 0.0, 1.0]; 16];
    for i in 0..16 {
        out[i][0] = r[i];
        out[i][1] = g[i];
    }
    out
}

/// BC5 signed to `f32` texels `(R, G, 0, 1)`.
pub fn decode_bc5_snorm_block_f32(block: &[u8; 16]) -> [[f32; 4]; 16] {
    let (r, g) = split(block);
    let r = decode_snorm_block_f32(r);
    let g = decode_snorm_block_f32(g);
    let mut out = [[0.0, 0.0, 0.0, 1.0]; 16];
    for i in 0..16 {
        out[i][0] = r[i];
        out[i][1] = g[i];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(r0: u8, r1: u8, codes: [u8; 16]) -> [u8; 8] {
        let mut bits = 0u64;
        for (i, &c) in codes.iter().enumerate() {
            bits |= (c as u64 & 7) << (3 * i);
        }
        let b = bits.to_le_bytes();
        [r0, r1, b[0], b[1], b[2], b[3], b[4], b[5]]
    }

    const SEQ: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 7, 6, 5, 4, 3, 2, 1, 0];

    #[test]
    fn unsigned_six_interpolated_values() {
        // DFS 19.1 Table 108: red0 > red1: (6 R0 + R1) / 7 ... (R0 + 6 R1) / 7
        let out = decode_unorm_block(&block(255, 0, SEQ));
        assert_eq!(out[..8], [255, 0, 218, 182, 145, 109, 72, 36]);
        let out = decode_unorm_block(&block(200, 13, SEQ));
        // (6 * 200 + 13) / 7 = 173, (5 * 200 + 26) / 7 = 146, (4 * 200 + 39) / 7 = 119, ...
        assert_eq!(out[..8], [200, 13, 173, 146, 119, 93, 66, 39]);
        // the second half of the codes runs backwards
        assert_eq!(out[8..], [39, 66, 93, 119, 146, 173, 13, 200]);
    }

    #[test]
    fn unsigned_four_interpolated_values_with_zero_and_one() {
        // red0 <= red1: (4 R0 + R1) / 5, (3 R0 + 2 R1) / 5, (2 R0 + 3 R1) / 5, (R0 + 4 R1) / 5, 0, 255
        let out = decode_unorm_block(&block(13, 200, SEQ));
        assert_eq!(out[..8], [13, 200, 50, 87, 125, 162, 0, 255]);
        // equal values are the four-value mode too
        let out = decode_unorm_block(&block(77, 77, SEQ));
        assert_eq!(out[..8], [77, 77, 77, 77, 77, 77, 0, 255]);
    }

    #[test]
    fn signed_six_interpolated_values() {
        // red0 > red1: (6 R0 + R1) / 7 ..., values divided by 127; here R0 = 127, R1 = -127.
        // Integer division truncates towards zero: (6 * 127 - 127) / 7 = 90 (90.7), 54 (54.4),
        // 18 (18.1), -18, -54, -90
        let out = decode_snorm_block(&block(127, (-127i8) as u8, SEQ));
        assert_eq!(out[..8], [127, -127, 90, 54, 18, -18, -54, -90]);
    }

    #[test]
    fn signed_four_interpolated_values_with_minus_one_and_one() {
        // red0 <= red1: (4 R0 + R1) / 5 ..., then -1.0 and 1.0 (-127 and 127)
        let out = decode_snorm_block(&block((-127i8) as u8, 127, SEQ));
        assert_eq!(out[..8], [-127, 127, -76, -25, 25, 76, -127, 127]);
    }

    #[test]
    fn signed_minus_128_means_minus_one() {
        // RED = -1.0 for red = -128 (DFS 19.2), the same as -127. The comparison red0 > red1 uses the
        // raw values: red0 = -128 <= red1 = -127 selects the four-value mode.
        let out = decode_snorm_block(&block((-128i8) as u8, (-127i8) as u8, SEQ));
        assert_eq!(out[..2], [-127, -127]);
        assert_eq!(out[6..8], [-127, 127]);
        // red0 = -127 > red1 = -128 is the case DFS 19.2 calls undefined; we compare the raw bytes,
        // so it selects the six-value mode and both ends read as -1.0
        let out = decode_snorm_block(&block((-127i8) as u8, (-128i8) as u8, SEQ));
        assert_eq!(out[..8], [-127, -127, -127, -127, -127, -127, -127, -127]);
    }

    #[test]
    fn f32_values_are_the_exact_fractions() {
        let out = decode_unorm_block_f32(&block(255, 0, SEQ));
        assert_eq!(out[2], 6.0 / 7.0);
        assert_eq!(out[7], 1.0 / 7.0);
        let out = decode_unorm_block_f32(&block(13, 200, SEQ));
        assert_eq!(out[2], (4.0 * 13.0 + 200.0) / (5.0 * 255.0));
        assert_eq!(out[6], 0.0);
        assert_eq!(out[7], 1.0);
        let out = decode_snorm_block_f32(&block(127, (-127i8) as u8, SEQ));
        assert_eq!(out[0], 1.0);
        assert_eq!(out[1], -1.0);
        assert_eq!(out[2], (6.0 * 127.0 - 127.0) / (7.0 * 127.0));
        let out = decode_snorm_block_f32(&block((-127i8) as u8, 127, SEQ));
        assert_eq!(out[6], -1.0);
        assert_eq!(out[7], 1.0);
    }

    #[test]
    fn eight_bit_values_stay_within_one_level_of_the_exact_ones() {
        // unsigned: the exact value is at least the 8-bit one and less than one level above it
        for r0 in (0..=255u8).step_by(5) {
            for r1 in (0..=255u8).step_by(3) {
                let b = block(r0, r1, SEQ);
                let (o, f) = (decode_unorm_block(&b), decode_unorm_block_f32(&b));
                for i in 0..16 {
                    let d = f[i] as f64 * 255.0 - o[i] as f64;
                    assert!((-1e-3..0.8572).contains(&d), "{r0} {r1} texel {i}: {d}");
                }
            }
        }
        for r0 in (-128..=127i16).step_by(5) {
            for r1 in (-128..=127i16).step_by(3) {
                let b = block(r0 as u8, r1 as u8, SEQ);
                let (o, f) = (decode_snorm_block(&b), decode_snorm_block_f32(&b));
                for i in 0..16 {
                    let d = f[i] as f64 * 127.0 - o[i] as f64;
                    assert!(d.abs() < 1.0, "{r0} {r1} texel {i}: {d}");
                }
            }
        }
    }

    #[test]
    fn bc4_and_bc5_texels() {
        let b4 = block(255, 0, SEQ);
        let t = decode_bc4_block(&b4);
        assert_eq!(t[2], [218, 0, 0, 255]);
        let t = decode_bc4_snorm_block(&block(127, (-127i8) as u8, SEQ));
        assert_eq!(t[2], [90, 0, 0, 127]);
        assert_eq!(t[1], [(-127i8) as u8, 0, 0, 127]);
        // BC5: red block first, green block second
        let mut b5 = [0u8; 16];
        b5[..8].copy_from_slice(&block(255, 0, SEQ));
        b5[8..].copy_from_slice(&block(13, 200, SEQ));
        let t = decode_bc5_block(&b5);
        assert_eq!(t[2], [218, 50, 0, 255]);
        assert_eq!(t[6], [72, 0, 0, 255]);
        let t = decode_bc5_snorm_block(&b5);
        assert_eq!(t[0][3], 127);
        let f = decode_bc5_block_f32(&b5);
        assert_eq!(f[2][3], 1.0);
        assert_eq!(f[2][2], 0.0);
    }
}
