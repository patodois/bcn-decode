//! BC7 tests built from the tables of the specification (Khronos DFS 1.3.1 section 20.1).
//!
//! `common/spec_tables.rs` is generated from Tables 109, 111, 112 and 113. The reference decoder
//! below reads the endpoints bit by bit from where Tables 112/113 say they come from and the
//! indices from the offsets of Table 111, so it shares no code and no layout logic with
//! `src/bc7.rs`, which walks the block front to back.

#![allow(clippy::needless_range_loop, clippy::precedence)]

mod common;
#[path = "../src/tables.rs"]
#[allow(dead_code)]
#[rustfmt::skip]
mod tables;

use bcn_decode::bc7;
use common::spec_tables::{BC7_ENDPOINTS, BC7_LAYOUT, BC7_PARAMS};
use common::{get, put, Rng};

const W2: [u32; 4] = [0, 21, 43, 64];
const W3: [u32; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const W4: [u32; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

fn weight(bits: u32, idx: u32) -> u32 {
    match bits {
        2 => W2[idx as usize],
        3 => W3[idx as usize],
        _ => W4[idx as usize],
    }
}

fn field(mode: usize, name: &str) -> Option<(u32, u32)> {
    BC7_LAYOUT[mode]
        .iter()
        .find(|f| f.name == name)
        .map(|f| (f.lo as u32, f.hi as u32))
}

/// DFS Equation 2.
fn lerp(e0: u32, e1: u32, w: u32) -> u32 {
    ((64 - w) * e0 + w * e1 + 32) >> 6
}

/// Reference decoder driven by the specification's tables.
fn reference(block: &[u8; 16]) -> [[u8; 4]; 16] {
    let v = u128::from_le_bytes(*block);
    if block[0] == 0 {
        return [[0; 4]; 16];
    }
    let mode = block[0].trailing_zeros() as usize;
    let [ns, _pb, _rb, _isb, _cb, _ab, _epb, _spb, ib, ib2] = BC7_PARAMS[mode].map(|x| x as u32);

    // Endpoints from Tables 112/113
    let mut ep = vec![[0u32; 4]; 2 * ns as usize];
    for (e, srcs) in BC7_ENDPOINTS[mode].iter().enumerate() {
        for c in 0..4 {
            let mut val = 0u32;
            for k in 0..8 {
                let s = srcs[c * 8 + k];
                let bit = if s == 255 {
                    1
                } else {
                    get(v, s as u32, 1) as u32
                };
                val = (val << 1) | bit;
            }
            ep[e][c] = val;
        }
    }

    let part = field(mode, "PB").map_or(0, |(lo, hi)| get(v, lo, hi - lo + 1) as usize);
    let rot = field(mode, "RB").map_or(0, |(lo, hi)| get(v, lo, hi - lo + 1));
    let isb = field(mode, "ISB").map_or(0, |(lo, _)| get(v, lo, 1));

    let subset = |i: usize| -> usize {
        match ns {
            1 => 0,
            2 => tables::PARTITION2[part][i] as usize,
            _ => tables::PARTITION3[part][i] as usize,
        }
    };
    let anchors: Vec<usize> = match ns {
        1 => vec![0],
        2 => vec![0, tables::ANCHOR2[part] as usize],
        _ => vec![
            0,
            tables::ANCHOR3_1[part] as usize,
            tables::ANCHOR3_2[part] as usize,
        ],
    };
    // Table 111: the indices start at the field "IB" ("IB2" for the secondary set). Each texel has
    // `bits` bits, one fewer for the anchor of its subset, so texel i starts after `bits * i`
    // bits minus one per anchor texel before it.
    let index = |set: &str, i: usize, bits: u32, anchors: &[usize]| -> u32 {
        let (lo, _) = field(mode, set).unwrap();
        let before = anchors.iter().filter(|&&a| a < i).count() as u32;
        let width = if anchors.contains(&i) { bits - 1 } else { bits };
        get(v, lo + bits * i as u32 - before, width) as u32
    };

    let mut out = [[0u8; 4]; 16];
    for (i, px) in out.iter_mut().enumerate() {
        let s = subset(i);
        let (e0, e1) = (ep[2 * s], ep[2 * s + 1]);
        let primary = index("IB", i, ib, &anchors);
        let (cidx, cbits, aidx, abits) = if ib2 == 0 {
            (primary, ib, primary, ib)
        } else {
            let secondary = index("IB2", i, ib2, &[0]);
            if isb == 1 {
                (secondary, ib2, primary, ib)
            } else {
                (primary, ib, secondary, ib2)
            }
        };
        let mut c = [
            lerp(e0[0], e1[0], weight(cbits, cidx)),
            lerp(e0[1], e1[1], weight(cbits, cidx)),
            lerp(e0[2], e1[2], weight(cbits, cidx)),
            lerp(e0[3], e1[3], weight(abits, aidx)),
        ];
        if rot != 0 {
            c.swap(3, rot as usize - 1);
        }
        *px = [c[0] as u8, c[1] as u8, c[2] as u8, c[3] as u8];
    }
    out
}

#[test]
fn table_109_and_111_agree_on_field_sizes() {
    // The bits of the layout must add up to 128 and the field widths follow Table 109
    for mode in 0..8 {
        let [ns, pb, rb, isb, cb, ab, epb, spb, ib, ib2] = BC7_PARAMS[mode].map(|x| x as u32);
        let total: u32 = BC7_LAYOUT[mode]
            .iter()
            .map(|f| f.hi as u32 - f.lo as u32 + 1)
            .sum();
        assert_eq!(total, 128, "mode {mode}");
        let width = |name: &str| field(mode, name).map_or(0, |(lo, hi)| hi - lo + 1);
        assert_eq!(width("M"), mode as u32 + 1);
        assert_eq!(width("PB"), pb);
        assert_eq!(width("RB"), rb);
        assert_eq!(width("ISB"), isb);
        for ch in ["R", "G", "B"] {
            for e in 0..2 * ns {
                assert_eq!(width(&format!("{ch}{e}")), cb, "mode {mode} {ch}{e}");
            }
        }
        for e in 0..2 * ns {
            assert_eq!(width(&format!("A{e}")), ab, "mode {mode} A{e}");
            assert_eq!(width(&format!("EPB{e}")), epb, "mode {mode} EPB{e}");
        }
        for s in 0..ns {
            assert_eq!(width(&format!("SPB{s}")), spb, "mode {mode} SPB{s}");
        }
        // Index bits: IB bits per texel minus one per subset (the anchors)
        assert_eq!(width("IB"), 16 * ib - ns, "mode {mode}");
        assert_eq!(
            width("IB2"),
            if ib2 > 0 { 16 * ib2 - 1 } else { 0 },
            "mode {mode}"
        );
    }
}

#[test]
fn matches_the_table_driven_reference_for_every_mode() {
    let mut rng = Rng(0xBC7);
    for mode in 0..8u32 {
        for _ in 0..60_000 {
            let mut b = rng.block16();
            // force the mode: `mode` zero bits and a one
            b[0] = (b[0] & !((2u16 << mode) as u8).wrapping_sub(1)) | (1 << mode);
            assert_eq!(
                bc7::decode_block(&b),
                reference(&b),
                "mode {mode} block {b:02x?}"
            );
        }
    }
}

#[test]
fn matches_the_reference_for_every_partition_of_every_partitioned_mode() {
    // Modes 0, 1, 2, 3 and 7 have partition bits; try every partition number with random rest
    let mut rng = Rng(0xA11CE);
    for mode in [0usize, 1, 2, 3, 7] {
        let (lo, hi) = field(mode, "PB").unwrap();
        for part in 0..(1u32 << (hi - lo + 1)) {
            for _ in 0..200 {
                let mut v = u128::from_le_bytes(rng.block16());
                put(&mut v, 0, mode as u32 + 1, 1 << mode);
                put(&mut v, lo, hi - lo + 1, part as u128);
                let b = v.to_le_bytes();
                assert_eq!(
                    bc7::decode_block(&b),
                    reference(&b),
                    "mode {mode} partition {part}"
                );
            }
        }
    }
}

#[test]
fn matches_the_reference_for_every_rotation_and_index_selection() {
    let mut rng = Rng(0x207);
    for mode in [4usize, 5] {
        let (rlo, _) = field(mode, "RB").unwrap();
        let isb = field(mode, "ISB");
        for rot in 0..4u128 {
            for sel in 0..2u128 {
                if mode == 5 && sel == 1 {
                    continue; // mode 5 has no index selection bit
                }
                for _ in 0..500 {
                    let mut v = u128::from_le_bytes(rng.block16());
                    put(&mut v, 0, mode as u32 + 1, 1 << mode);
                    put(&mut v, rlo, 2, rot);
                    if let Some((l, _)) = isb {
                        put(&mut v, l, 1, sel);
                    }
                    let b = v.to_le_bytes();
                    assert_eq!(
                        bc7::decode_block(&b),
                        reference(&b),
                        "mode {mode} rot {rot} sel {sel}"
                    );
                }
            }
        }
    }
}

#[test]
fn reserved_mode_decodes_to_zero() {
    // DFS 20.1: "hardware decoders processing a texel block with a low byte of 0 should return 0
    // for all channels of all texels"
    let mut rng = Rng(8);
    for _ in 0..1000 {
        let mut b = rng.block16();
        b[0] = 0;
        assert_eq!(bc7::decode_block(&b), [[0; 4]; 16]);
    }
}

// ---- Hand-built blocks. Bit positions are the ones printed in Table 111 --------------------

/// Block with just the mode bit set.
fn blank(mode: u32) -> u128 {
    1u128 << mode
}

/// Index of texel `i` stored in a field starting at `lo` with `bits` bits per index, where the
/// anchors (texels in `anchors`) have one bit less. All earlier texels must be set in order.
fn set_index(v: &mut u128, lo: u32, bits: u32, anchors: &[usize], i: usize, value: u32) {
    let before = anchors.iter().filter(|&&a| a < i).count() as u32;
    let width = if anchors.contains(&i) { bits - 1 } else { bits };
    put(v, lo + bits * i as u32 - before, width, value as u128);
}

#[test]
fn mode_6_one_subset_four_bit_indices_and_unique_pbits() {
    // Table 111, mode 6: M 0..6 = 0000001, R0 7..13, R1 14..20, G0 21..27, G1 28..34,
    // B0 35..41, B1 42..48, A0 49..55, A1 56..62, EPB0 63, EPB1 64, IB 65..127.
    let mut v = blank(6);
    // Endpoint 0: R=127 G=64 B=0 A=127, P-bit 1 -> 8-bit (255, 129, 1, 255)
    put(&mut v, 7, 7, 127);
    put(&mut v, 21, 7, 64);
    put(&mut v, 35, 7, 0);
    put(&mut v, 49, 7, 127);
    // Endpoint 1: R=0 G=0 B=127 A=0, P-bit 0 -> (0, 0, 254, 0)
    put(&mut v, 14, 7, 0);
    put(&mut v, 28, 7, 0);
    put(&mut v, 42, 7, 127);
    put(&mut v, 56, 7, 0);
    put(&mut v, 63, 1, 1);
    put(&mut v, 64, 1, 0);
    // Texel i has index i; texel 0 is the anchor (3 bits)
    for i in 0..16 {
        set_index(&mut v, 65, 4, &[0], i, i as u32);
    }
    let out = bc7::decode_block(&v.to_le_bytes());
    // Equation 2 with the 4-bit weights of Table 119 (texel 1: 4, texel 8: 34, texel 15: 64):
    assert_eq!(out[0], [255, 129, 1, 255]);
    assert_eq!(out[1], [239, 121, 17, 239]); // (60 * 255 + 32) >> 6 = 239, (60 * 129 + 32) >> 6 = 121, ...
    assert_eq!(out[8], [120, 60, 135, 120]);
    assert_eq!(out[15], [0, 0, 254, 0]);
}

#[test]
fn mode_0_three_subsets_with_unique_pbits() {
    // Table 111, mode 0: M 0 = 1, PB 1..4, R0..R5 at 5 + 4k, G at 29 + 4k, B at 53 + 4k,
    // EPB0..EPB5 77..82, IB 83..127. Three solid-colour subsets, partition 0 of Table 115:
    //   0 0 1 1 / 0 0 1 1 / 0 2 2 1 / 2 2 2 2
    let mut v = blank(0);
    put(&mut v, 1, 4, 0); // partition 0
                          // colours as 4-bit values plus P-bit: (v << 1 | p) is the 5-bit value, expanded to 8 bits
    let subsets = [([15u32, 0, 8], 1u32), ([0, 15, 4], 0), ([7, 7, 7], 1)];
    for (s, (rgb, p)) in subsets.iter().enumerate() {
        for e in 0..2 {
            for (c, &x) in rgb.iter().enumerate() {
                put(
                    &mut v,
                    5 + 24 * c as u32 + 4 * (2 * s as u32 + e),
                    4,
                    x as u128,
                );
            }
            put(&mut v, 77 + 2 * s as u32 + e, 1, *p as u128);
        }
    }
    let out = bc7::decode_block(&v.to_le_bytes());
    let five = |x: u32, p: u32| {
        let t = (x << 1) | p;
        ((t << 3) | (t >> 2)) as u8
    };
    // 4-bit value x with P-bit p is the 5-bit value 2x + p, widened by (t << 3) | (t >> 2):
    //   (15, p1) = 31 -> 255        (0, p1) = 1 -> 8        (8, p1) = 17 -> 136 | 4 = 140
    //   (8, p0) = 16 -> 128 | 4 = 132
    assert_eq!(five(15, 1), 255);
    assert_eq!(five(0, 1), 8);
    assert_eq!(five(8, 1), 140);
    assert_eq!(five(8, 0), 132);
    let col = |s: usize| {
        let (rgb, p) = subsets[s];
        [five(rgb[0], p), five(rgb[1], p), five(rgb[2], p), 255]
    };
    let map = [0, 0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 1, 2, 2, 2, 2];
    for i in 0..16 {
        assert_eq!(out[i], col(map[i]), "texel {i}");
    }
    assert_eq!(col(0), [255, 8, 140, 255]);
    // subset 1 has P-bit 0: (0, 15, 4) -> (0, 30 -> 240 | 7, 8 -> 64 | 2) = (0, 247, 66)
    assert_eq!(col(1), [0, 247, 66, 255]);
    // subset 2 has P-bit 1: (7, 7, 7) -> 15 -> 120 | 3 = 123
    assert_eq!(col(2), [123, 123, 123, 255]);
}

#[test]
fn mode_1_two_subsets_with_a_shared_pbit() {
    // Table 111, mode 1: M 0..1 = 01, PB 2..7, R0..R3 at 8 + 6k, G0..G3 at 32 + 6k,
    // B0..B3 at 56 + 6k, SPB0 80, SPB1 81, IB 82..127. Partition 0: two columns of each.
    let mut v = blank(1);
    put(&mut v, 2, 6, 0);
    // 6-bit x with the shared P-bit p is the 7-bit t = 2x + p, widened by (t << 1) | (t >> 6):
    // subset 0: R=63 G=0 B=32, P-bit 1 -> t = (127, 1, 65) -> (254 | 1, 2 | 0, 130 | 1) = (255, 2, 131)
    // subset 1: R=21 G=42 B=63, P-bit 0 -> t = (42, 84, 126) -> (84 | 0, 168 | 1, 252 | 1) = (84, 169, 253)
    let cols = [([63u32, 0, 32], 1u32), ([21, 42, 63], 0)];
    for (s, (rgb, p)) in cols.iter().enumerate() {
        for e in 0..2u32 {
            for (c, &x) in rgb.iter().enumerate() {
                put(
                    &mut v,
                    8 + 24 * c as u32 + 6 * (2 * s as u32 + e),
                    6,
                    x as u128,
                );
            }
        }
        put(&mut v, 80 + s as u32, 1, *p as u128);
    }
    let out = bc7::decode_block(&v.to_le_bytes());
    let expect = |rgb: [u32; 3], p: u32| {
        let f = |x: u32| {
            let t = (x << 1) | p; // 7 bits
            ((t << 1) | (t >> 6)) as u8
        };
        [f(rgb[0]), f(rgb[1]), f(rgb[2]), 255]
    };
    assert_eq!(expect([63, 0, 32], 1), [255, 2, 131, 255]);
    assert_eq!(expect([21, 42, 63], 0), [84, 169, 253, 255]);
    let map = [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1];
    for i in 0..16 {
        assert_eq!(out[i], expect(cols[map[i]].0, cols[map[i]].1), "texel {i}");
    }
}

#[test]
fn mode_2_three_subsets_without_pbits() {
    // Table 111, mode 2: M 0..2 = 001, PB 3..8, R0..R5 at 9 + 5k, G0..G5 at 39 + 5k, B at 69 + 5k,
    // IB 99..127. Partition 0 of Table 115 again.
    let mut v = blank(2);
    put(&mut v, 3, 6, 0);
    let cols = [[31u32, 0, 16], [0, 31, 8], [20, 20, 20]];
    for (s, rgb) in cols.iter().enumerate() {
        for e in 0..2u32 {
            for (c, &x) in rgb.iter().enumerate() {
                put(
                    &mut v,
                    9 + 30 * c as u32 + 5 * (2 * s as u32 + e),
                    5,
                    x as u128,
                );
            }
        }
    }
    let out = bc7::decode_block(&v.to_le_bytes());
    let expand = |x: u32| ((x << 3) | (x >> 2)) as u8; // 5 bits -> 8 bits
    assert_eq!([expand(31), expand(0), expand(16)], [255, 0, 132]);
    let map = [0, 0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 1, 2, 2, 2, 2];
    for i in 0..16 {
        let c = cols[map[i]];
        assert_eq!(
            out[i],
            [expand(c[0]), expand(c[1]), expand(c[2]), 255],
            "texel {i}"
        );
    }
}

#[test]
fn mode_3_two_subsets_and_an_anchor_that_is_not_texel_15() {
    // Table 111, mode 3: M 0..3 = 0001, PB 4..9, R0..R3 at 10 + 7k, G at 38 + 7k, B at 66 + 7k,
    // EPB0..EPB3 94..97, IB 98..127. Partition 17: the anchor of subset 1 is texel 2 (Table 118),
    // so texel 2's index has one bit and texels 3.. are stored one bit earlier than they would be.
    let mut v = blank(3);
    put(&mut v, 4, 6, 17);
    // subset 0: E0 black, E1 white; subset 1: E0 white, E1 black (7 bits + P-bit)
    let ends = [[0u32; 3], [127, 127, 127], [127, 127, 127], [0, 0, 0]];
    for (e, rgb) in ends.iter().enumerate() {
        for (c, &x) in rgb.iter().enumerate() {
            put(&mut v, 10 + 28 * c as u32 + 7 * e as u32, 7, x as u128);
        }
        put(
            &mut v,
            94 + e as u32,
            1,
            if e == 1 || e == 2 { 1 } else { 0 },
        );
    }
    let part = &tables::PARTITION2[17];
    assert_eq!(tables::ANCHOR2[17], 2);
    let anchors = [0usize, 2];
    // every texel selects the far endpoint (index 3 = weight 64) except the anchors, which have a
    // single bit (their top bit is implied zero): index 1 = weight 21
    for i in 0..16 {
        let idx = if anchors.contains(&i) { 1 } else { 3 };
        set_index(&mut v, 98, 2, &anchors, i, idx);
    }
    let out = bc7::decode_block(&v.to_le_bytes());
    for i in 0..16 {
        let s = part[i];
        // subset 0: black -> white. subset 1: white -> black
        let (e0, e1) = if s == 0 { (0u32, 255u32) } else { (255, 0) };
        let w = if anchors.contains(&i) { 21 } else { 64 };
        let g = lerp(e0, e1, w) as u8;
        assert_eq!(out[i], [g, g, g, 255], "texel {i}");
    }
    // texels 0 and 2 are the anchors: weight 21 of the way from the start of their subset
    assert_eq!(out[0][0], lerp(0, 255, 21) as u8);
    assert_eq!(out[2][0], lerp(255, 0, 21) as u8);
}

#[test]
fn mode_4_rotation_and_index_selection() {
    // Table 111, mode 4: M 0..4 = 00001, RB 5..6, ISB 7, R0 8..12, R1 13..17, G0 18..22, G1 23..27,
    // B0 28..32, B1 33..37, A0 38..43, A1 44..49, IB 50..80 (2 bits, anchor texel 0 has 1),
    // IB2 81..127 (3 bits, anchor texel 0 has 2).
    let build = |rot: u128, isb: u128| {
        let mut v = blank(4);
        put(&mut v, 5, 2, rot);
        put(&mut v, 7, 1, isb);
        // colour: black -> white (5-bit 0 and 31); alpha: 0 -> 63 (6 bits)
        for (lo, x) in [
            (8, 0u128),
            (13, 31),
            (18, 0),
            (23, 31),
            (28, 0),
            (33, 31),
            (38, 0),
            (44, 63),
        ] {
            let n = if lo >= 38 { 6 } else { 5 };
            put(&mut v, lo, n, x);
        }
        for i in 0..16 {
            set_index(&mut v, 50, 2, &[0], i, (i % 4) as u32);
            set_index(&mut v, 81, 3, &[0], i, (i % 8) as u32);
        }
        bc7::decode_block(&v.to_le_bytes())
    };
    // texel i: 2-bit weight W2[i % 4] and 3-bit weight W3[i % 8]; both ends are 0 and 255
    let l2 = |i: usize| lerp(0, 255, W2[i % 4]) as u8;
    let l3 = |i: usize| lerp(0, 255, W3[i % 8]) as u8;
    assert_eq!([l2(1), l2(2), l2(3)], [84, 171, 255]); // by hand: (21 * 255 + 32) >> 6 = 84, ...
    assert_eq!(
        [l3(1), l3(2), l3(3), l3(4), l3(5), l3(6), l3(7)],
        [36, 72, 108, 147, 183, 219, 255]
    );
    // ISB = 0: colour from the 2-bit indices, alpha from the 3-bit ones
    let out = build(0, 0);
    for i in 0..8 {
        assert_eq!(
            out[i],
            [l2(i), l2(i), l2(i), l3(i)],
            "no rotation, isb 0, texel {i}"
        );
    }
    // ISB = 1: colour from the 3-bit indices, alpha from the 2-bit ones
    let out = build(0, 1);
    for i in 0..8 {
        assert_eq!(
            out[i],
            [l3(i), l3(i), l3(i), l2(i)],
            "no rotation, isb 1, texel {i}"
        );
    }
    // Rotation 1, 2, 3 swap alpha with R, G, B (Table 120). The three colour channels are equal,
    // so the swapped channel shows the alpha value.
    for (rot, ch) in [(1u128, 0usize), (2, 1), (3, 2)] {
        let out = build(rot, 0);
        for i in 0..8 {
            let mut expect = [l2(i), l2(i), l2(i), l3(i)];
            expect.swap(3, ch);
            assert_eq!(out[i], expect, "rotation {rot}, texel {i}");
        }
    }
}

#[test]
fn mode_5_rotation_with_separate_alpha_indices() {
    // Table 111, mode 5: M 0..5 = 000001, RB 6..7, R0 8..14, R1 15..21, G0 22..28, G1 29..35,
    // B0 36..42, B1 43..49, A0 50..57, A1 58..65, IB 66..96, IB2 97..127 (both 2 bits).
    let build = |rot: u128| {
        let mut v = blank(5);
        put(&mut v, 6, 2, rot);
        // colour endpoints: (127,0,64) -> (0,127,64) (7 bits); alpha 255 -> 0
        for (lo, x) in [
            (8, 127u128),
            (15, 0),
            (22, 0),
            (29, 127),
            (36, 64),
            (43, 64),
        ] {
            put(&mut v, lo, 7, x);
        }
        put(&mut v, 50, 8, 255);
        put(&mut v, 58, 8, 0);
        for i in 0..16 {
            set_index(&mut v, 66, 2, &[0], i, (i % 4) as u32);
            set_index(&mut v, 97, 2, &[0], i, ((i + 1) % 4) as u32); // alpha index differs
        }
        bc7::decode_block(&v.to_le_bytes())
    };
    let c7 = |x: u32| (x << 1) | (x >> 6); // 7 -> 8 bits
    let (r0, g1, b) = (c7(127), c7(127), c7(64));
    assert_eq!((r0, b), (255, 129));
    let px = |i: usize| -> [u8; 4] {
        let wc = W2[i % 4];
        let wa = W2[(i + 1) % 4];
        [
            lerp(r0, 0, wc) as u8,
            lerp(0, g1, wc) as u8,
            lerp(b, b, wc) as u8,
            lerp(255, 0, wa) as u8,
        ]
    };
    for rot in 0..4u128 {
        let out = build(rot);
        for i in 0..16 {
            let mut e = px(i);
            if rot > 0 {
                e.swap(3, rot as usize - 1);
            }
            assert_eq!(out[i], e, "rotation {rot} texel {i}");
        }
    }
}

#[test]
fn mode_7_two_subsets_with_alpha_and_pbits() {
    // Table 111, mode 7: M 0..7 = 00000001, PB 8..13, R0..R3 at 14 + 5k, G0..G3 at 34 + 5k,
    // B at 54 + 5k, A0..A3 at 74 + 5k, EPB0..EPB3 94..97, IB 98..127. Partition 0.
    let mut v = blank(7);
    put(&mut v, 8, 6, 0);
    // subset 0: RGBA 5-bit (31, 0, 16, 8), P-bit 1 -> 6 bits (63, 1, 33, 17)
    // subset 1: (0, 31, 10, 31), P-bit 0           -> (0, 62, 20, 62)
    let cols = [([31u32, 0, 16, 8], 1u32), ([0, 31, 10, 31], 0)];
    for (s, (rgba, p)) in cols.iter().enumerate() {
        for e in 0..2u32 {
            for (c, &x) in rgba.iter().enumerate() {
                put(
                    &mut v,
                    14 + 20 * c as u32 + 5 * (2 * s as u32 + e),
                    5,
                    x as u128,
                );
            }
            put(&mut v, 94 + 2 * s as u32 + e, 1, *p as u128);
        }
    }
    let out = bc7::decode_block(&v.to_le_bytes());
    let six = |x: u32, p: u32| {
        let t = (x << 1) | p;
        ((t << 2) | (t >> 4)) as u8
    };
    let col = |s: usize| {
        let (c, p) = cols[s];
        [six(c[0], p), six(c[1], p), six(c[2], p), six(c[3], p)]
    };
    // 5-bit x with P-bit 1 is the 6-bit t = 2x + 1, widened by (t << 2) | (t >> 4):
    // 63 -> 252 | 3 = 255, 1 -> 4 | 0 = 4, 33 -> 132 | 2 = 134, 17 -> 68 | 1 = 69
    assert_eq!(col(0), [255, 4, 134, 69]);
    let map = [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1];
    for i in 0..16 {
        assert_eq!(out[i], col(map[i]), "texel {i}");
    }
}
