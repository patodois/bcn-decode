//! BC6H tests built from the specification (Khronos DFS 1.3.1 section 20.2, Tables 114, 119, 121
//! and 122; Microsoft's "BC6H Format").
//!
//! The reference decoder here reads the fields with the *vector notation of Table 122* ("R09..0",
//! "G24", "R010..11" ...) and applies the pseudocode of section 20.2, so it shares nothing with the
//! run tables that `src/bc6h.rs` uses (generated from the bit-by-bit Tables 123/124).

#![allow(clippy::needless_range_loop, clippy::precedence)]

mod common;
#[path = "../src/tables.rs"]
#[allow(dead_code)]
#[rustfmt::skip]
mod tables;

use bcn_decode::bc6h;
use common::spec_tables::BC6H_DESCRIPTIONS;
use common::{get, put, Rng};

/// Table 121: (mode, transformed, partitioned, endpoint bits, delta bits R/G/B)
const TABLE_121: [(u8, bool, bool, u32, [u32; 3]); 14] = [
    (0, true, true, 10, [5, 5, 5]),
    (1, true, true, 7, [6, 6, 6]),
    (2, true, true, 11, [5, 4, 4]),
    (6, true, true, 11, [4, 5, 4]),
    (10, true, true, 11, [4, 4, 5]),
    (14, true, true, 9, [5, 5, 5]),
    (18, true, true, 8, [6, 5, 5]),
    (22, true, true, 8, [5, 6, 5]),
    (26, true, true, 8, [5, 5, 6]),
    (30, false, true, 6, [0, 0, 0]),
    (3, false, false, 10, [0, 0, 0]),
    (7, true, false, 11, [9, 9, 9]),
    (11, true, false, 12, [8, 8, 8]),
    (15, true, false, 16, [4, 4, 4]),
];

const W3: [i64; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const W4: [i64; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

/// One element of Table 122: `<name><endpoint?><a>[..<b>]`
struct Item {
    name: String,
    ep: usize,
    a: u32,
    b: u32,
}

fn parse_description(s: &str) -> Vec<Item> {
    s.split(',')
        .map(|item| {
            let item = item.trim();
            let (name, rest) = if let Some(r) = item.strip_prefix("PB") {
                ("PB".to_string(), r)
            } else if let Some(r) = item.strip_prefix('M') {
                ("M".to_string(), r)
            } else {
                (item[..1].to_string(), &item[1..])
            };
            // For R, G and B the first digit is the endpoint number
            let (ep, rest) = if name == "M" || name == "PB" {
                (0, rest)
            } else {
                ((rest.as_bytes()[0] - b'0') as usize, &rest[1..])
            };
            let (a, b) = match rest.split_once("..") {
                Some((a, b)) => (a.parse().unwrap(), b.parse().unwrap()),
                None => (rest.parse().unwrap(), rest.parse().unwrap()),
            };
            Item { name, ep, a, b }
        })
        .collect()
}

/// A decoded view of the fields of a block through Table 122: `raw[ch][ep]` is the value stored
/// for R/G/B endpoint 0..3, `partition` the PB field.
struct Fields {
    raw: [[i64; 4]; 3],
    partition: usize,
}

/// Reads the fields of `v` for the mode `mode` (the low bits of v already hold it).
fn read_fields(v: u128, desc: &[Item]) -> Fields {
    let mut f = Fields {
        raw: [[0; 4]; 3],
        partition: 0,
    };
    let mut pos = 0u32;
    for it in desc {
        let n = it.a.abs_diff(it.b) + 1;
        for k in 0..n {
            let bit = get(v, pos + k, 1) as i64;
            // The first block bit goes to bit b of the variable; the next ones towards a
            // ("If a < b, then the bits are reversed")
            let target = if it.a >= it.b { it.b + k } else { it.b - k };
            match it.name.as_str() {
                "M" => {}
                "PB" => f.partition |= (bit as usize) << target,
                c => f.raw["RGB".find(c).unwrap()][it.ep] |= bit << target,
            }
        }
        pos += n;
    }
    f
}

/// Writes logical field values into a block (the inverse of `read_fields`); the mode bits are left
/// as they are.
fn write_fields(v: &mut u128, desc: &[Item], f: &Fields) {
    let mut pos = 0u32;
    for it in desc {
        let n = it.a.abs_diff(it.b) + 1;
        if it.name != "M" {
            for k in 0..n {
                let target = if it.a >= it.b { it.b + k } else { it.b - k };
                let bit = match it.name.as_str() {
                    "PB" => ((f.partition >> target) & 1) as u128,
                    c => ((f.raw["RGB".find(c).unwrap()][it.ep] >> target) & 1) as u128,
                };
                put(v, pos + k, 1, bit);
            }
        }
        pos += n;
    }
}

fn sign_extend(x: i64, bits: u32) -> i64 {
    let s = 64 - bits;
    (x << s) >> s
}

/// Section 20.2 unquantization of one endpoint value.
fn unquantize(x: i64, signed: bool, epb: u32) -> i64 {
    if !signed {
        if epb >= 15 {
            x
        } else if x == 0 {
            0
        } else if x == (1 << epb) - 1 {
            0xFFFF
        } else {
            ((x << 15) + 0x4000) >> (epb - 1)
        }
    } else if epb >= 16 {
        x
    } else {
        let (s, m) = if x < 0 { (true, -x) } else { (false, x) };
        let unq = if m == 0 {
            0
        } else if m >= (1 << (epb - 1)) - 1 {
            0x7FFF
        } else {
            ((m << 15) + 0x4000) >> (epb - 1)
        };
        if s {
            -unq
        } else {
            unq
        }
    }
}

/// Microsoft's `finish_unquantize` (BC6H Format): the magnitude is scaled, then the sign is applied
/// if the scaled value is negative. (DFS 20.2 tests the sign of the unscaled value, which differs
/// only for -1: negative zero there.)
fn finish(i: i64, signed: bool) -> u16 {
    if !signed {
        ((i * 31) >> 6) as u16
    } else {
        let scaled = if i < 0 {
            -(((-i) * 31) >> 5)
        } else {
            (i * 31) >> 5
        };
        if scaled < 0 {
            (0x8000 | -scaled) as u16
        } else {
            scaled as u16
        }
    }
}

/// The reference decoder.
fn reference(block: &[u8; 16], signed: bool) -> [[u16; 3]; 16] {
    let v = u128::from_le_bytes(*block);
    // "If the low two bits are less than two, that is the mode number, otherwise the low five bits
    // is the mode number"
    let low2 = (v & 3) as u8;
    let mode = if low2 < 2 { low2 } else { (v & 31) as u8 };
    let Some(&(_, transformed, partitioned, epb, delta)) = TABLE_121.iter().find(|t| t.0 == mode)
    else {
        return [[0; 3]; 16]; // reserved
    };
    let desc = parse_description(BC6H_DESCRIPTIONS.iter().find(|d| d.0 == mode).unwrap().1);
    let f = read_fields(v, &desc);

    let n_ep = if partitioned { 4 } else { 2 };
    let mut ep = [[0i64; 3]; 4];
    for ch in 0..3 {
        let e0 = if signed {
            sign_extend(f.raw[ch][0], epb)
        } else {
            f.raw[ch][0]
        };
        ep[0][ch] = e0;
        for k in 1..n_ep {
            let mut e = f.raw[ch][k];
            if signed || transformed {
                e = sign_extend(e, if transformed { delta[ch] } else { epb });
            }
            if transformed {
                e = (e0 + e) & ((1 << epb) - 1);
                if signed {
                    e = sign_extend(e, epb);
                }
            }
            ep[k][ch] = e;
        }
    }
    let unq: Vec<[i64; 3]> = ep[..n_ep]
        .iter()
        .map(|e| e.map(|x| unquantize(x, signed, epb)))
        .collect();

    // Indices: one subset has 4-bit indices from bit 65, two subsets 3-bit indices from bit 82
    // (Tables 123 and 124); the anchor texels have one bit less
    let (start, bits, weights): (u32, u32, &[i64]) = if partitioned {
        (82, 3, &W3)
    } else {
        (65, 4, &W4)
    };
    let anchors: Vec<usize> = if partitioned {
        vec![0, tables::ANCHOR2[f.partition] as usize]
    } else {
        vec![0]
    };
    let mut out = [[0u16; 3]; 16];
    for (i, px) in out.iter_mut().enumerate() {
        let before = anchors.iter().filter(|&&a| a < i).count() as u32;
        let width = if anchors.contains(&i) { bits - 1 } else { bits };
        let idx = get(v, start + bits * i as u32 - before, width) as usize;
        let s = if partitioned {
            tables::PARTITION2[f.partition][i] as usize
        } else {
            0
        };
        let w = weights[idx];
        for ch in 0..3 {
            let interp = (unq[2 * s][ch] * (64 - w) + unq[2 * s + 1][ch] * w + 32) >> 6;
            px[ch] = finish(interp, signed);
        }
    }
    out
}

#[test]
fn matches_the_reference_for_every_mode() {
    let mut rng = Rng(0xB6);
    for signed in [false, true] {
        for &(mode, ..) in &TABLE_121 {
            for _ in 0..40_000 {
                let mut b = rng.block16();
                if mode < 2 {
                    b[0] = (b[0] & !3) | mode;
                } else {
                    b[0] = (b[0] & !31) | mode;
                }
                assert_eq!(
                    bc6h::decode_block_half(&b, signed),
                    reference(&b, signed),
                    "mode {mode} signed {signed} block {b:02x?}"
                );
            }
        }
    }
}

#[test]
fn matches_the_reference_for_all_32_block_modes_and_arbitrary_blocks() {
    // Including the four reserved modes, which the reference returns as zero
    let mut rng = Rng(0xB7);
    for signed in [false, true] {
        for _ in 0..200_000 {
            let b = rng.block16();
            assert_eq!(
                bc6h::decode_block_half(&b, signed),
                reference(&b, signed),
                "{b:02x?}"
            );
        }
    }
}

#[test]
fn matches_the_reference_for_every_partition() {
    let mut rng = Rng(0xB8);
    for signed in [false, true] {
        for part in 0..32u128 {
            for _ in 0..300 {
                let mut v = u128::from_le_bytes(rng.block16());
                put(&mut v, 0, 5, 30); // mode 30: six-bit endpoints, no transform, partitioned
                put(&mut v, 77, 5, part);
                let b = v.to_le_bytes();
                assert_eq!(
                    bc6h::decode_block_half(&b, signed),
                    reference(&b, signed),
                    "partition {part}"
                );
            }
        }
    }
}

#[test]
fn the_parsed_header_agrees_with_the_notation() {
    let mut rng = Rng(0xB9);
    for signed in [false, true] {
        for _ in 0..50_000 {
            let b = rng.block16();
            let v = u128::from_le_bytes(b);
            let low2 = (v & 3) as u8;
            let mode = if low2 < 2 { low2 } else { (v & 31) as u8 };
            let info = bc6h::parse_block(&b, signed);
            let Some(&(_, transformed, partitioned, epb, delta)) =
                TABLE_121.iter().find(|t| t.0 == mode)
            else {
                assert!(info.is_none());
                continue;
            };
            let info = info.unwrap();
            assert_eq!(info.mode, mode);
            assert_eq!(info.endpoint_bits, epb);
            assert_eq!(info.transformed, transformed);
            let desc = parse_description(BC6H_DESCRIPTIONS.iter().find(|d| d.0 == mode).unwrap().1);
            let f = read_fields(v, &desc);
            assert_eq!(
                info.partition.map(|p| p as usize),
                partitioned.then_some(f.partition)
            );
            // endpoint 0 without the transform
            for ch in 0..3 {
                let e0 = if signed {
                    sign_extend(f.raw[ch][0], epb)
                } else {
                    f.raw[ch][0]
                };
                assert_eq!(info.endpoints[0][ch] as i64, e0);
            }
            let _ = delta;
        }
    }
}

// ---- Hand-built blocks ------------------------------------------------------------------------

/// A block of the given mode whose bits are all zero otherwise.
fn block_of(mode: u8) -> u128 {
    mode as u128
}

/// Sets every field of `mode` from `raw[ch][ep]` and the partition.
fn build(mode: u8, raw: [[i64; 4]; 3], partition: usize) -> u128 {
    let desc = parse_description(BC6H_DESCRIPTIONS.iter().find(|d| d.0 == mode).unwrap().1);
    let mut v = block_of(mode);
    write_fields(&mut v, &desc, &Fields { raw, partition });
    v
}

#[test]
fn mode_3_unquantizes_the_ends_but_not_the_half_way_value() {
    // Mode 3 ("00011"): ten-bit R0 G0 B0 R1 G1 B1 at bits 5.., indices from bit 65. Index 0 on
    // every texel gives endpoint 0 (the anchor stores 3 bits of zero).
    // R0 = 1023 is the all-ones value: unquantized to 0xFFFF, then (0xFFFF * 31) >> 6 = 0x7BFF
    //   (65504.0, the largest half float).
    // G0 = 512 is only the half-way value: ((512 << 15) + 0x4000) >> 9 = 32800 -> 0x3E0F.
    // B0 = 1 -> ((1 << 15) + 0x4000) >> 9 = 96 -> (96 * 31) >> 6 = 46 = 0x2E.
    let v = build(3, [[1023, 0, 0, 0], [512, 0, 0, 0], [1, 0, 0, 0]], 0);
    let out = bc6h::decode_block_half(&v.to_le_bytes(), false);
    for px in out {
        assert_eq!(px, [0x7BFF, 0x3E0F, 0x002E]);
    }
    assert_eq!(bc6h::half_to_f32(0x7BFF), 65504.0);
    // 0x3E0F: exponent 15 (2^0), mantissa 0x20F = 527 -> 1 + 527 / 1024
    assert_eq!(bc6h::half_to_f32(0x3E0F), 1.0 + 527.0 / 1024.0);
}

#[test]
fn mode_3_signed_interpolates_negative_and_positive_endpoints_as_twos_complement() {
    // E0 = -256 (ten-bit two's complement 0x300), E1 = 255, one subset (4-bit indices).
    // Unquantized (EPB 10): -((256 << 15) + 0x4000) >> 9 = -16416 and 16352.
    // Index 8 has weight 34: (-16416 * 30 + 16352 * 34 + 32) >> 6 = 992 -> (992 * 31) >> 5 = 961.
    // Index 4 has weight 17: (-16416 * 47 + 16352 * 17 + 32) >> 6 = -7712 (the shift rounds down)
    //   -> ((7712 * 31) >> 5) | 0x8000 = 0x9D2F.
    let mut v = build(3, [[0x300, 0x0FF, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]], 0);
    // texel i has index i, texel 0 is the anchor (3 bits)
    let mut pos = 65;
    for i in 0..16u128 {
        let n = if i == 0 { 3 } else { 4 };
        put(&mut v, pos, n, i & ((1 << n) - 1));
        pos += n;
    }
    let out = bc6h::decode_block_half(&v.to_le_bytes(), true);
    assert_eq!(out[0][0], 0xBE1F); // -16416 -> -((16416 * 31) >> 5) = -15903 = 0x8000 | 0x3E1F
    assert_eq!(out[4][0], 0x9D2F);
    assert_eq!(out[8][0], 961);
    assert_eq!(out[15][0], 0x3DE1); // 16352 -> (16352 * 31) >> 5 = 15841
                                    // G and B are zero everywhere
    assert!(out.iter().all(|p| p[1] == 0 && p[2] == 0));
}

#[test]
fn mode_15_keeps_all_sixteen_bits_of_the_wrapped_endpoint() {
    // Mode 15 (16-bit endpoints, 4-bit deltas). R0 = 0xFFFF and delta -1 (0b1111): the sum is
    // 0xFFFE; a mask of 1u16 << 16 would turn it into zero.
    let v = build(15, [[0xFFFF, 0xF, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]], 0);
    let b = v.to_le_bytes();
    let info = bc6h::parse_block(&b, false).unwrap();
    assert_eq!(info.endpoints[0][0], 0xFFFF);
    assert_eq!(info.endpoints[1][0], 0xFFFE);
    // all indices are zero, so every texel is endpoint 0: 0xFFFF -> 0x7BFF
    let out = bc6h::decode_block_half(&b, false);
    assert!(out.iter().all(|p| p[0] == (0xFFFFu32 * 31 >> 6) as u16));
    // set every non-anchor index to 15 (weight 64): endpoint 1, 0xFFFE -> (0xFFFE * 31) >> 6
    let mut v = v;
    let mut pos = 65;
    for i in 0..16 {
        let n = if i == 0 { 3 } else { 4 };
        put(&mut v, pos, n, if i == 0 { 0 } else { 15 });
        pos += n;
    }
    let out = bc6h::decode_block_half(&v.to_le_bytes(), false);
    assert_eq!(out[1][0], (0xFFFEu32 * 31 >> 6) as u16);
    assert_eq!(out[1][0], 0x7BFF); // (65534 * 31) >> 6 = 31743; zero would mean the mask bug
                                   // delta -8 (0b1000) from 0xFFFF is 0xFFF7: (65527 * 31) >> 6 = 31739 = 0x7BFB
    let mut v = build(15, [[0xFFFF, 0x8, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]], 0);
    let mut pos = 65;
    for i in 0..16 {
        let n = if i == 0 { 3 } else { 4 };
        put(&mut v, pos, n, if i == 0 { 0 } else { 15 });
        pos += n;
    }
    let b = v.to_le_bytes();
    assert_eq!(
        bc6h::parse_block(&b, false).unwrap().endpoints[1][0],
        0xFFF7
    );
    assert_eq!(bc6h::decode_block_half(&b, false)[1][0], 0x7BFB);
}

#[test]
fn signed_negative_infinity_is_reachable_and_clamps_to_zero_in_8_bit() {
    // Signed mode 15: R0 = 0x8000 (-32768, the 16-bit endpoints are not unquantized), index 0:
    // |i| = 0x8000 -> (0x8000 * 31) >> 5 = 0x7C00, sign set -> 0xFC00 = -infinity (DFS 20.2).
    let v = build(15, [[0x8000, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]], 0);
    let b = v.to_le_bytes();
    let out = bc6h::decode_block_half(&b, true);
    assert_eq!(out[0][0], 0xFC00);
    assert_eq!(bc6h::half_to_f32(0xFC00), f32::NEG_INFINITY);
    assert_eq!(bc6h::decode_block_f32(&b, true)[0][0], f32::NEG_INFINITY);
    assert_eq!(bc6h::decode_block_rgba8(&b, true)[0], [0, 0, 0, 255]);
}

#[test]
fn two_subsets_and_an_anchor_that_is_not_texel_15() {
    // Mode 30 (six-bit endpoints, no transform, 2 subsets). Partition 17: the anchor of subset 1 is
    // texel 2 (Table 118). Subset 0 runs E0 = 0 -> E1 = 63 (0xFFFF), subset 1 runs E2 = 63 -> E3 = 0.
    // Non-anchor texels use index 7 (weight 64), the two anchors store two bits "11" = 3 (weight 27).
    let raw = [[0, 63, 63, 0], [0, 63, 63, 0], [0, 63, 63, 0]];
    let mut v = build(30, raw, 17);
    assert_eq!(tables::ANCHOR2[17], 2);
    let anchors = [0usize, 2];
    let mut pos = 82;
    for i in 0..16 {
        let (n, idx) = if anchors.contains(&i) { (2, 3) } else { (3, 7) };
        put(&mut v, pos, n, idx);
        pos += n;
    }
    assert_eq!(pos, 128);
    let out = bc6h::decode_block_half(&v.to_le_bytes(), false);
    for i in 0..16 {
        let subset = tables::PARTITION2[17][i];
        let expect = match (subset, anchors.contains(&i)) {
            (0, false) => 0x7BFF, // E1
            (_, false) => 0x0000, // E3 (subset 1)
            // weight 27 from E0 = 0 to E1 = 0xFFFF: (65535 * 27 + 32) >> 6 = 27648 -> 0x3450
            (0, true) => 0x3450,
            // weight 27 from E2 = 0xFFFF to E3 = 0: (65535 * 37 + 32) >> 6 = 37887 -> 0x47AF
            _ => 0x47AF,
        };
        assert_eq!(out[i], [expect; 3], "texel {i}");
    }
}

#[test]
fn reserved_modes_decode_to_zero_with_alpha_one() {
    // DFS 20.2: modes 19, 23, 27 and 31 are reserved; the result is (0.0, 0.0, 0.0, 1.0)
    for mode in [19u8, 23, 27, 31] {
        for signed in [false, true] {
            let mut b = [0xA5u8; 16];
            b[0] = (b[0] & !31) | mode;
            assert!(bc6h::parse_block(&b, signed).is_none());
            assert_eq!(bc6h::decode_block_half(&b, signed), [[0; 3]; 16]);
            assert_eq!(
                bc6h::decode_block_f32(&b, signed),
                [[0.0, 0.0, 0.0, 1.0]; 16]
            );
        }
    }
}

#[test]
fn all_fourteen_modes_read_their_endpoints_from_the_documented_bits() {
    // For each mode, give every field a distinct recognisable value, decode the header and check
    // that every endpoint comes back (E0 as stored, E1..E3 after the delta transform).
    for signed in [false, true] {
        for &(mode, transformed, partitioned, epb, delta) in &TABLE_121 {
            let n_ep = if partitioned { 4 } else { 2 };
            let mut raw = [[0i64; 4]; 3];
            for ch in 0..3 {
                for k in 0..n_ep {
                    let bits = if k == 0 || !transformed {
                        epb
                    } else {
                        delta[ch]
                    };
                    // a distinct, non-trivial pattern that uses the top bit as well
                    raw[ch][k] = ((0x2B + 37 * ch as i64 + 11 * k as i64) | (1 << (bits - 1)))
                        & ((1 << bits) - 1);
                }
            }
            let v = build(mode, raw, 0b10110);
            let info = bc6h::parse_block(&v.to_le_bytes(), signed).unwrap();
            assert_eq!(info.mode, mode);
            for ch in 0..3 {
                let e0 = if signed {
                    sign_extend(raw[ch][0], epb)
                } else {
                    raw[ch][0]
                };
                assert_eq!(info.endpoints[0][ch] as i64, e0, "mode {mode} E0 ch {ch}");
                for k in 1..n_ep {
                    let mut e = raw[ch][k];
                    if transformed {
                        e = (e0 + sign_extend(e, delta[ch])) & ((1 << epb) - 1);
                        if signed {
                            e = sign_extend(e, epb);
                        }
                    } else if signed {
                        e = sign_extend(e, epb);
                    }
                    assert_eq!(
                        info.endpoints[k][ch] as i64, e,
                        "mode {mode} E{k} ch {ch} signed {signed}"
                    );
                }
            }
            if partitioned {
                assert_eq!(info.partition, Some(0b10110), "mode {mode}");
            }
        }
    }
}

#[test]
fn half_to_f32_matches_the_definition_for_every_bit_pattern() {
    for h in 0..=u16::MAX {
        let sign = if h & 0x8000 != 0 { -1.0f64 } else { 1.0 };
        let exp = ((h >> 10) & 31) as i32;
        let man = (h & 0x3FF) as f64;
        let expect: f64 = match exp {
            0 => sign * man * 2f64.powi(-24),
            31 => {
                if man == 0.0 {
                    sign * f64::INFINITY
                } else {
                    f64::NAN
                }
            }
            e => sign * (1.0 + man / 1024.0) * 2f64.powi(e - 15),
        };
        let got = bc6h::half_to_f32(h) as f64;
        if expect.is_nan() {
            assert!(got.is_nan(), "{h:#06x}");
        } else {
            assert_eq!(got, expect, "{h:#06x}");
            assert!(got.to_bits() == expect.to_bits(), "{h:#06x} sign of zero");
        }
    }
}

#[test]
fn rgba8_convenience_clamps_and_truncates() {
    // 1.0 -> 255, 0.5 -> 127 (truncated), 2.0 -> 255, negative -> 0, -inf -> 0, NaN -> 0
    assert_eq!(bc6h::half_to_unorm8(0x3C00), 255);
    assert_eq!(bc6h::half_to_unorm8(0x3800), 127);
    assert_eq!(bc6h::half_to_unorm8(0x4000), 255);
    assert_eq!(bc6h::half_to_unorm8(0xBC00), 0);
    assert_eq!(bc6h::half_to_unorm8(0xFC00), 0);
    assert_eq!(bc6h::half_to_unorm8(0x7E00), 0);
    assert_eq!(bc6h::half_to_unorm8(0x7C00), 255);
    assert_eq!(bc6h::half_to_unorm8(0x0000), 0);
}

#[test]
fn half_to_unorm8_equals_the_float_definition_for_every_bit_pattern() {
    // floor(clamp(x, 0, 1) * 255) with x the half converted to f32 (the product is exact in f32)
    for h in 0..=u16::MAX {
        let x = bc6h::half_to_f32(h);
        let expect = if x.is_nan() || x <= 0.0 {
            0
        } else if x >= 1.0 {
            255
        } else {
            (x * 255.0) as u8
        };
        assert_eq!(bc6h::half_to_unorm8(h), expect, "{h:#06x}");
    }
}

#[test]
fn signed_minus_one_scales_to_positive_zero_like_direct3d() {
    // Mode 15 (16-bit endpoints, no unquantizing) with R0 = 0xFFFF = -1 and zero deltas: every
    // texel interpolates to -1, ((-1 * (64 - w) - w + 32) >> 6) = -1. Microsoft's finish_unquantize
    // scales the magnitude, (1 * 31) >> 5 = 0, and only then looks at the sign: +0.0 (0x0000). The
    // DFS 20.2 text would give 0x8000 (-0.0); the choice follows Direct3D (see `bc6h::finish`).
    let v = build(
        15,
        [[0xFFFF, 0, 0, 0], [0xFFFF, 0, 0, 0], [0xFFFF, 0, 0, 0]],
        0,
    );
    let b = v.to_le_bytes();
    assert_eq!(
        bc6h::parse_block(&b, true).unwrap().endpoints[0],
        [-1, -1, -1]
    );
    let out = bc6h::decode_block_half(&b, true);
    assert!(out.iter().all(|p| *p == [0, 0, 0]), "{out:?}");
    let f = bc6h::decode_block_f32(&b, true);
    assert!(f.iter().all(|t| t[0].is_sign_positive() && t[0] == 0.0));
    // -2 scales to (2 * 31) >> 5 = 1: the smallest negative subnormal 0x8001
    let v = build(15, [[0xFFFE, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]], 0);
    let out = bc6h::decode_block_half(&v.to_le_bytes(), true);
    assert_eq!(out[0][0], 0x8001);
}
