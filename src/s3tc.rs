//! BC1, BC2 and BC3 (the S3TC / DXT1, DXT3 and DXT5 formats).
//!
//! Sources: Khronos Data Format Specification 1.3.1 (DFS) section 18 "S3TC Compressed Texture
//! Image Formats", and Microsoft's "Block Compression (Direct3D 10)" (D3D10 BC) for BC1-BC3.
//!
//! # 8-bit rounding
//!
//! Both specifications define the texel values as real numbers (for example
//! `(2 * RGB0 + RGB1) / 3`) and leave the conversion to 8 bits to the implementation: Direct3D
//! only demands bit-exact decoding for BC6H and BC7. This module uses the convention that
//! software decoders commonly use and that `texture2ddecoder` (our previous dependency) also
//! follows, so that images do not change when switching: the 5:6:5 endpoints are widened to 8 bits
//! by replicating their top bits, the in-between colours are computed on those 8-bit values with
//! integer arithmetic, and the division rounds down. Against the exact real value of the
//! specification (scaled to 0..255) the result is at most 0.72 levels above and at most 1.37 below,
//! over all possible endpoint pairs (checked by the `stays_close_to_the_exact_values` test).

use crate::rgtc;

/// Widens an RGB 5:6:5 value to 8 bits per channel by replicating the top bits (DFS 18.1 gives
/// `R = color^15..11 / 31`, `G = color^10..5 / 63`, `B = color^4..0 / 31`; replication is the
/// integer form of that scaling and maps 31 to 255 and 63 to 255).
#[inline]
fn expand_565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 0x1F) as u8;
    let g = ((c >> 5) & 0x3F) as u8;
    let b = (c & 0x1F) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

/// The four colours a BC1 colour block can select from (DFS 18.1, Table 105).
///
/// `four_colors` forces the `color0 > color1` branch, which BC2 and BC3 do ("the two code bits
/// always use the non-transparent encodings", DFS 18.3 and 18.4). `punch_through` makes index 3 of
/// the three-colour branch transparent black (DFS 18.2, Table 106) instead of opaque black.
#[inline]
fn color_palette(block: &[u8; 8], four_colors: bool, punch_through: bool) -> [[u8; 4]; 4] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let e0 = expand_565(c0);
    let e1 = expand_565(c1);
    let mut p = [[0, 0, 0, 255]; 4];
    p[0] = [e0[0], e0[1], e0[2], 255];
    p[1] = [e1[0], e1[1], e1[2], 255];
    if c0 > c1 || four_colors {
        for ch in 0..3 {
            p[2][ch] = ((2 * e0[ch] as u16 + e1[ch] as u16) / 3) as u8;
            p[3][ch] = ((e0[ch] as u16 + 2 * e1[ch] as u16) / 3) as u8;
        }
    } else {
        for ch in 0..3 {
            p[2][ch] = ((e0[ch] as u16 + e1[ch] as u16) / 2) as u8;
        }
        // Index 3 is black; with alpha it is also transparent, and "the red, green, and blue
        // components of any texels with a final alpha of 0 should be encoded as zero" (DFS 18.2)
        p[3] = [0, 0, 0, if punch_through { 0 } else { 255 }];
    }
    p
}

/// Decodes a BC1 block (8 bytes) to 16 RGBA texels in raster order (row by row).
///
/// With `punch_through_alpha` false the format is "BC1 with no alpha" (DFS 18.1): index 3 of the
/// three-colour mode is opaque black and every alpha is 255. With it true the format is "BC1 with
/// alpha" (DFS 18.2): that texel is (0, 0, 0, 0).
pub fn decode_bc1_block(block: &[u8; 8], punch_through_alpha: bool) -> [[u8; 4]; 16] {
    let pal = color_palette(block, false, punch_through_alpha);
    // code(x, y) = bits[2 * (4y + x) + 1 .. 2 * (4y + x)] (DFS 18.1)
    let bits = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    let mut out = [[0u8; 4]; 16];
    for (i, px) in out.iter_mut().enumerate() {
        *px = pal[((bits >> (2 * i)) & 3) as usize];
    }
    out
}

/// Decodes a BC2 block (16 bytes: 64 bits of 4-bit alpha, then a BC1 colour block).
pub fn decode_bc2_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let (alpha, color) = block.split_at(8);
    let color: &[u8; 8] = color.try_into().unwrap_or(&[0; 8]);
    let pal = color_palette(color, true, false);
    let bits = u32::from_le_bytes([color[4], color[5], color[6], color[7]]);
    let alpha = u64::from_le_bytes([
        alpha[0], alpha[1], alpha[2], alpha[3], alpha[4], alpha[5], alpha[6], alpha[7],
    ]);
    let mut out = [[0u8; 4]; 16];
    for (i, px) in out.iter_mut().enumerate() {
        *px = pal[((bits >> (2 * i)) & 3) as usize];
        // alpha(x, y) = bits[4 * (4y + x) + 3 .. 4 * (4y + x)], value alpha / 15 (DFS 18.3).
        // 15 / 15 -> 255, so the 4-bit value times 17 is exact.
        px[3] = (((alpha >> (4 * i)) & 0xF) as u8) * 17;
    }
    out
}

/// Decodes a BC3 block (16 bytes: a BC4-style alpha block, then a BC1 colour block).
pub fn decode_bc3_block(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let (alpha, color) = block.split_at(8);
    let color: &[u8; 8] = color.try_into().unwrap_or(&[0; 8]);
    let alpha: &[u8; 8] = alpha.try_into().unwrap_or(&[0; 8]);
    let pal = color_palette(color, true, false);
    let bits = u32::from_le_bytes([color[4], color[5], color[6], color[7]]);
    let a = rgtc::decode_unorm_block(alpha);
    let mut out = [[0u8; 4]; 16];
    for (i, px) in out.iter_mut().enumerate() {
        *px = pal[((bits >> (2 * i)) & 3) as usize];
        px[3] = a[i];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// Texel codes 0, 1, 2, 3 repeated on every row: bits = 0b11_10_01_00 per row.
    const ROW_CODES: [u8; 4] = [0xE4, 0xE4, 0xE4, 0xE4];

    fn bc1_block(c0: u16, c1: u16, codes: [u8; 4]) -> [u8; 8] {
        let (a, b) = (c0.to_le_bytes(), c1.to_le_bytes());
        [
            a[0], a[1], b[0], b[1], codes[0], codes[1], codes[2], codes[3],
        ]
    }

    #[test]
    fn widening_565_replicates_the_top_bits() {
        assert_eq!(expand_565(0x0000), [0, 0, 0]);
        assert_eq!(expand_565(0xFFFF), [255, 255, 255]);
        // R = 16 -> 128 | 4 = 132, G = 32 -> 128 | 2 = 130, B = 1 -> 8, R = 30 -> 240 | 7 = 247
        assert_eq!(expand_565((16 << 11) | (32 << 5) | 1), [132, 130, 8]);
        assert_eq!(expand_565(30 << 11)[0], 247);
        // DFS 18.1 scales by 1/31 and 1/63: replication stays within 0.7 levels of 255 * v / max
        for v in 0..32u32 {
            let e = expand_565((v << 11) as u16)[0] as f64;
            assert!((e - 255.0 * v as f64 / 31.0).abs() < 0.7, "r5 {v}");
        }
        for v in 0..64u32 {
            let e = expand_565((v << 5) as u16)[1] as f64;
            assert!((e - 255.0 * v as f64 / 63.0).abs() < 0.72, "g6 {v}");
        }
    }

    #[test]
    fn bc1_four_colour_block() {
        // color0 = red 0xF800 > color1 = blue 0x001F: RGB0, RGB1, (2 RGB0 + RGB1) / 3, (RGB0 + 2 RGB1) / 3
        let out = decode_bc1_block(&bc1_block(0xF800, 0x001F, ROW_CODES), false);
        assert_eq!(
            &out[..4],
            &[
                [255, 0, 0, 255],
                [0, 0, 255, 255],
                [170, 0, 85, 255],
                [85, 0, 170, 255]
            ]
        );
        // The alpha variant only changes the three-colour case
        assert_eq!(
            decode_bc1_block(&bc1_block(0xF800, 0x001F, ROW_CODES), true),
            out
        );
        // The same codes on all four rows
        assert_eq!(out[4..8], out[..4]);
        assert_eq!(out[12..16], out[..4]);
    }

    #[test]
    fn bc1_three_colour_block_without_and_with_alpha() {
        // color0 = blue <= color1 = red: RGB0, RGB1, (RGB0 + RGB1) / 2, black
        let blk = bc1_block(0x001F, 0xF800, ROW_CODES);
        let opaque = decode_bc1_block(&blk, false);
        assert_eq!(
            &opaque[..4],
            &[
                [0, 0, 255, 255],
                [255, 0, 0, 255],
                [127, 0, 127, 255],
                [0, 0, 0, 255]
            ]
        );
        let alpha = decode_bc1_block(&blk, true);
        assert_eq!(
            &alpha[..4],
            &[
                [0, 0, 255, 255],
                [255, 0, 0, 255],
                [127, 0, 127, 255],
                [0, 0, 0, 0]
            ]
        );
    }

    #[test]
    fn bc1_equal_endpoints_use_the_three_colour_mode() {
        // "color0 <= color1" selects the three-colour mode, so equality does too: code 3 is black
        let out = decode_bc1_block(&bc1_block(0x8410, 0x8410, ROW_CODES), true);
        assert_eq!(out[0], [132, 130, 132, 255]);
        assert_eq!(out[2], [132, 130, 132, 255]);
        assert_eq!(out[3], [0, 0, 0, 0]);
    }

    #[test]
    fn bc1_texel_order_is_row_major_with_the_low_bits_first() {
        // code(x, y) = bits[2 * (4y + x) + 1 .. 2 * (4y + x)]: set texel (x = 1, y = 2) to code 1
        let bits: u32 = 1 << (2 * (4 * 2 + 1));
        let b = bits.to_le_bytes();
        let out = decode_bc1_block(&bc1_block(0xF800, 0x001F, b), false);
        for (i, px) in out.iter().enumerate() {
            let expect = if i == 9 {
                [0, 0, 255, 255]
            } else {
                [255, 0, 0, 255]
            };
            assert_eq!(*px, expect, "texel {i}");
        }
    }

    #[test]
    fn bc2_has_explicit_four_bit_alpha_and_always_four_colours() {
        // alpha nibbles 0, 1, ..., 15 (low nibble of byte 0 is texel 0); colour block with
        // color0 = blue <= color1 = red, which would be the three-colour mode in BC1
        let mut blk = [0u8; 16];
        for (i, b) in blk.iter_mut().take(8).enumerate() {
            *b = (2 * i as u8) | ((2 * i as u8 + 1) << 4);
        }
        blk[8..].copy_from_slice(&bc1_block(0x001F, 0xF800, ROW_CODES));
        let out = decode_bc2_block(&blk);
        for (i, px) in out.iter().enumerate() {
            assert_eq!(px[3], (i * 17) as u8, "alpha {i}"); // n / 15 * 255 = 17 n
        }
        // DFS 18.3: treated as color0 > color1, so index 2 is (2 RGB0 + RGB1) / 3 and index 3 is
        // (RGB0 + 2 RGB1) / 3, not black
        assert_eq!([out[2][0], out[2][1], out[2][2]], [85, 0, 170]);
        assert_eq!([out[3][0], out[3][1], out[3][2]], [170, 0, 85]);
    }

    #[test]
    fn bc3_alpha_with_six_interpolated_values() {
        // alpha0 = 255 > alpha1 = 0: 255, 0, (6 a0 + a1) / 7 ... (a0 + 6 a1) / 7 rounded down
        let mut blk = [0u8; 16];
        blk[0] = 255;
        blk[1] = 0;
        // codes 0..7 for the first 8 texels, 3 bits each, then 7..0 for the others
        let mut bits = 0u64;
        for i in 0..16u64 {
            let code = if i < 8 { i } else { 15 - i };
            bits |= code << (3 * i);
        }
        blk[2..8].copy_from_slice(&bits.to_le_bytes()[..6]);
        blk[8..].copy_from_slice(&bc1_block(0xFFFF, 0xFFFF, [0; 4]));
        let out = decode_bc3_block(&blk);
        let alphas: Vec<u8> = out.iter().map(|p| p[3]).collect();
        assert_eq!(alphas[..8], [255, 0, 218, 182, 145, 109, 72, 36]);
        // texels 8..16 have the codes 7, 6, ..., 0
        assert_eq!(alphas[8..], [36, 72, 109, 145, 182, 218, 0, 255]);
        assert!(out.iter().all(|p| p[..3] == [255, 255, 255]));
    }

    #[test]
    fn bc3_alpha_with_four_interpolated_values_and_the_extremes() {
        // alpha0 = 0 <= alpha1 = 255: 0, 255, (4 a0 + a1) / 5, (3 a0 + 2 a1) / 5, (2 a0 + 3 a1) / 5,
        // (a0 + 4 a1) / 5, then 0 and 255
        let mut blk = [0u8; 16];
        blk[0] = 0;
        blk[1] = 255;
        let mut bits = 0u64;
        for i in 0..8u64 {
            bits |= i << (3 * i);
        }
        blk[2..8].copy_from_slice(&bits.to_le_bytes()[..6]);
        let out = decode_bc3_block(&blk);
        let alphas: Vec<u8> = out.iter().take(8).map(|p| p[3]).collect();
        assert_eq!(alphas, [0, 255, 51, 102, 153, 204, 0, 255]);
        // equal alphas are also the four-value mode
        blk[0] = 100;
        blk[1] = 100;
        let out = decode_bc3_block(&blk);
        let alphas: Vec<u8> = out.iter().take(8).map(|p| p[3]).collect();
        assert_eq!(alphas, [100, 100, 100, 100, 100, 100, 0, 255]);
    }

    #[test]
    fn stays_close_to_the_exact_values() {
        // The specification's values are real numbers; compare with them over every pair of
        // endpoints: the 8-bit result is at most 0.72 above and 1.37 below the exact value.
        for (bits, max) in [(5u32, 31u32), (6, 63)] {
            for a in 0..=max {
                for b in 0..=max {
                    let widen = |v: u32| (v << (8 - bits)) | (v >> (2 * bits - 8));
                    let (ea, eb) = (widen(a), widen(b));
                    for (num, den, wa, wb) in [
                        (2 * ea + eb, 3, 2, 1),
                        (ea + 2 * eb, 3, 1, 2),
                        (ea + eb, 2, 1, 1),
                    ] {
                        let ours = (num / den) as f64;
                        let exact = 255.0 * (wa * a + wb * b) as f64 / (den * max) as f64;
                        let d = exact - ours;
                        assert!((-0.72..=1.37).contains(&d), "{bits}-bit {a} {b}: {d}");
                    }
                }
            }
        }
    }
}
