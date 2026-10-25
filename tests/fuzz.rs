//! Random and structured blocks must never panic, and the results must obey the invariants of
//! each format.

#![allow(clippy::needless_range_loop, clippy::precedence)]

mod common;

use bcn_decode::{bc6h, bc7, decode_rgba8_to_vec, decode_rgba_f32_to_vec, rgtc, s3tc, Format};
use common::Rng;

#[test]
fn random_blocks_never_panic_in_any_decoder() {
    let mut rng = Rng(0xF022);
    for _ in 0..300_000 {
        let b = rng.block16();
        let b8: [u8; 8] = b[..8].try_into().unwrap();
        let _ = s3tc::decode_bc1_block(&b8, false);
        let _ = s3tc::decode_bc1_block(&b8, true);
        let _ = s3tc::decode_bc2_block(&b);
        let _ = s3tc::decode_bc3_block(&b);
        let _ = rgtc::decode_bc4_block(&b8);
        let _ = rgtc::decode_bc4_snorm_block(&b8);
        let _ = rgtc::decode_bc5_block(&b);
        let _ = rgtc::decode_bc5_snorm_block(&b);
        let _ = rgtc::decode_bc4_block_f32(&b8);
        let _ = rgtc::decode_bc5_snorm_block_f32(&b);
        let _ = bc7::decode_block(&b);
        for signed in [false, true] {
            let _ = bc6h::decode_block_half(&b, signed);
            let _ = bc6h::decode_block_f32(&b, signed);
            let _ = bc6h::decode_block_rgba8(&b, signed);
            let _ = bc6h::parse_block(&b, signed);
        }
    }
}

#[test]
fn special_blocks_never_panic() {
    let specials: Vec<[u8; 16]> = vec![
        [0; 16], [0xFF; 16], [0x55; 16], [0xAA; 16], [0x80; 16], [0x01; 16], [0x7F; 16],
    ];
    for b in specials {
        let b8: [u8; 8] = b[..8].try_into().unwrap();
        let _ = s3tc::decode_bc1_block(&b8, true);
        let _ = s3tc::decode_bc2_block(&b);
        let _ = s3tc::decode_bc3_block(&b);
        let _ = rgtc::decode_bc4_snorm_block(&b8);
        let _ = rgtc::decode_bc5_snorm_block_f32(&b);
        let _ = bc7::decode_block(&b);
        for signed in [false, true] {
            let _ = bc6h::decode_block_half(&b, signed);
        }
    }
}

#[test]
fn every_first_byte_of_every_bc7_and_bc6h_block() {
    // The mode lives in the first byte: all 256 values with random bytes after them
    let mut rng = Rng(0xF1257);
    for first in 0..=255u8 {
        for _ in 0..2000 {
            let mut b = rng.block16();
            b[0] = first;
            let out = bc7::decode_block(&b);
            if first == 0 {
                assert_eq!(out, [[0; 4]; 16]);
            }
            for signed in [false, true] {
                let _ = bc6h::decode_block_half(&b, signed);
            }
        }
    }
}

#[test]
fn bc1_to_bc3_invariants() {
    let mut rng = Rng(0xC0);
    for _ in 0..200_000 {
        let b = rng.block16();
        let b8: [u8; 8] = b[..8].try_into().unwrap();
        // BC1 without alpha is always opaque
        assert!(s3tc::decode_bc1_block(&b8, false)
            .iter()
            .all(|p| p[3] == 255));
        // with alpha: alpha is 0 or 255 and the colour of a transparent texel is black
        for p in s3tc::decode_bc1_block(&b8, true) {
            assert!(p[3] == 0 || p[3] == 255);
            if p[3] == 0 {
                assert_eq!(p, [0, 0, 0, 0]);
            }
        }
        // the colours of BC1 with and without alpha agree where the alpha is opaque
        let (a, o) = (
            s3tc::decode_bc1_block(&b8, true),
            s3tc::decode_bc1_block(&b8, false),
        );
        for (x, y) in a.iter().zip(o.iter()) {
            if x[3] == 255 {
                assert_eq!(x, y);
            }
        }
        // BC2 alpha is a multiple of 17
        assert!(s3tc::decode_bc2_block(&b).iter().all(|p| p[3] % 17 == 0));
    }
}

#[test]
fn bc6h_outputs_are_well_formed() {
    let mut rng = Rng(0xB6B6);
    for _ in 0..200_000 {
        let b = rng.block16();
        // unsigned: finite, non-negative half floats (sign bit clear, exponent below 31)
        for p in bc6h::decode_block_half(&b, false) {
            for h in p {
                assert!(h <= 0x7BFF, "{h:#06x}");
            }
        }
        // signed: never NaN; only -infinity (0xFC00) can be an exceptional value
        for p in bc6h::decode_block_half(&b, true) {
            for h in p {
                let exp = (h >> 10) & 31;
                assert!(exp < 31 || h == 0xFC00, "{h:#06x}");
            }
        }
        for t in bc6h::decode_block_f32(&b, true) {
            assert!(t.iter().all(|v| !v.is_nan()));
            assert_eq!(t[3], 1.0);
        }
    }
}

#[test]
fn bc4_bc5_extremes() {
    let mut rng = Rng(0xB4);
    for _ in 0..100_000 {
        let b = rng.block16();
        let b8: [u8; 8] = b[..8].try_into().unwrap();
        let u = rgtc::decode_unorm_block(&b8);
        let f = rgtc::decode_unorm_block_f32(&b8);
        for i in 0..16 {
            assert!((0.0..=1.0).contains(&f[i]));
            // every 8-bit value lies between the two end values or is a constant 0/255
            assert!(u[i] as f32 / 255.0 <= f[i] + 1e-6);
        }
        let s = rgtc::decode_snorm_block(&b8);
        assert!(s.iter().all(|&v| v >= -127));
        let sf = rgtc::decode_snorm_block_f32(&b8);
        assert!(sf.iter().all(|v| (-1.0..=1.0).contains(v)));
    }
}

#[test]
fn random_surfaces_of_random_sizes_never_panic() {
    let mut rng = Rng(0x5F);
    let formats = [
        Format::Bc1,
        Format::Bc1Alpha,
        Format::Bc2,
        Format::Bc3,
        Format::Bc4,
        Format::Bc4Snorm,
        Format::Bc5,
        Format::Bc5Snorm,
        Format::Bc6hUf16,
        Format::Bc6hSf16,
        Format::Bc7,
    ];
    for _ in 0..2000 {
        let format = formats[rng.below(formats.len() as u64) as usize];
        let (w, h) = (rng.below(40) as usize, rng.below(40) as usize);
        let need = format.surface_bytes(w, h).unwrap();
        // sometimes too short on purpose
        let len = if rng.below(4) == 0 {
            need.saturating_sub(1 + rng.below(20) as usize)
        } else {
            need
        };
        let mut src = vec![0u8; len];
        rng.fill(&mut src);
        let r = decode_rgba8_to_vec(format, &src, w, h);
        assert_eq!(r.is_ok(), len >= need, "{format:?} {w}x{h}");
        let r = decode_rgba_f32_to_vec(format, &src, w, h);
        assert_eq!(r.is_ok(), len >= need);
    }
}
