//! Whole-surface behaviour: cropping, pitches, errors, output types.

#![allow(clippy::needless_range_loop, clippy::precedence)]

mod common;

use bcn_decode::{
    decode_bc6h_rgba_f16, decode_bc6h_rgba_f16_pitched, decode_rgba8, decode_rgba8_pitched,
    decode_rgba8_to_vec, decode_rgba_f32, decode_rgba_f32_pitched, decode_rgba_f32_to_vec, Error,
    Format,
};
use common::Rng;

const ALL: [Format; 11] = [
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

/// A BC1 block that decodes to one solid colour (565 value `c`, all indices 0).
fn solid_bc1(c: u16) -> [u8; 8] {
    let b = c.to_le_bytes();
    [b[0], b[1], b[0], b[1], 0, 0, 0, 0]
}

#[test]
fn block_and_surface_sizes() {
    assert_eq!(Format::Bc1.block_bytes(), 8);
    assert_eq!(Format::Bc4.block_bytes(), 8);
    assert_eq!(Format::Bc4Snorm.block_bytes(), 8);
    assert_eq!(Format::Bc2.block_bytes(), 16);
    assert_eq!(Format::Bc7.block_bytes(), 16);
    assert_eq!(Format::Bc1.surface_bytes(4, 4), Some(8));
    assert_eq!(Format::Bc1.surface_bytes(5, 5), Some(32));
    assert_eq!(Format::Bc7.surface_bytes(1, 1), Some(16));
    assert_eq!(Format::Bc7.surface_bytes(0, 10), Some(0));
    assert_eq!(Format::Bc7.surface_bytes(usize::MAX, 4), None);
    assert_eq!(
        Format::Bc7.surface_bytes(usize::MAX / 2, usize::MAX / 2),
        None
    );
}

#[test]
fn partial_edge_blocks_are_cropped() {
    // 5 x 6 texels = 2 x 2 blocks; every block a different solid colour
    let colours: [u16; 4] = [0xF800, 0x07E0, 0x001F, 0xFFFF];
    let mut src = Vec::new();
    for c in colours {
        src.extend_from_slice(&solid_bc1(c));
    }
    let mut out = vec![0xEEu8; 5 * 6 * 4];
    decode_rgba8(Format::Bc1, &src, 5, 6, &mut out).unwrap();
    let rgb = |c: u16| -> [u8; 4] {
        match c {
            0xF800 => [255, 0, 0, 255],
            0x07E0 => [0, 255, 0, 255],
            0x001F => [0, 0, 255, 255],
            _ => [255, 255, 255, 255],
        }
    };
    for y in 0..6 {
        for x in 0..5 {
            let c = colours[(y / 4) * 2 + x / 4];
            let at = (y * 5 + x) * 4;
            assert_eq!(out[at..at + 4], rgb(c), "({x}, {y})");
        }
    }
}

#[test]
fn width_or_height_below_four() {
    for (w, h) in [(1usize, 1usize), (2, 3), (3, 1), (1, 7), (4, 1)] {
        let bx = w.div_ceil(4);
        let by = h.div_ceil(4);
        let src: Vec<u8> = (0..bx * by).flat_map(|_| solid_bc1(0xF800)).collect();
        let out = decode_rgba8_to_vec(Format::Bc1, &src, w, h).unwrap();
        assert_eq!(out.len(), w * h * 4);
        assert!(out.chunks(4).all(|p| p == [255, 0, 0, 255]), "{w}x{h}");
    }
}

#[test]
fn cropping_is_the_same_as_decoding_the_padded_surface() {
    let mut rng = Rng(77);
    for &format in &ALL {
        for _ in 0..40 {
            let (w, h) = (1 + rng.below(23) as usize, 1 + rng.below(23) as usize);
            let (pw, ph) = (w.div_ceil(4) * 4, h.div_ceil(4) * 4);
            let mut src = vec![0u8; format.surface_bytes(w, h).unwrap()];
            rng.fill(&mut src);
            let small = decode_rgba8_to_vec(format, &src, w, h).unwrap();
            let big = decode_rgba8_to_vec(format, &src, pw, ph).unwrap();
            for y in 0..h {
                assert_eq!(
                    small[y * w * 4..(y + 1) * w * 4],
                    big[y * pw * 4..y * pw * 4 + w * 4],
                    "{format:?} {w}x{h} row {y}"
                );
            }
        }
    }
}

#[test]
fn pitches_are_respected_and_padding_is_untouched() {
    let mut rng = Rng(5);
    for &format in &ALL {
        let (w, h) = (11usize, 9usize);
        let bb = format.block_bytes();
        let row = w.div_ceil(4) * bb;
        let mut packed = vec![0u8; row * h.div_ceil(4)];
        rng.fill(&mut packed);
        let expect = decode_rgba8_to_vec(format, &packed, w, h).unwrap();

        // source rows 7 bytes apart more than needed, filled with junk
        let src_pitch = row + 7;
        let mut src = vec![0xA5u8; src_pitch * h.div_ceil(4)];
        for (r, chunk) in packed.chunks(row).enumerate() {
            src[r * src_pitch..r * src_pitch + row].copy_from_slice(chunk);
        }
        // destination rows 13 bytes apart more than needed
        let dst_pitch = w * 4 + 13;
        let mut dst = vec![0xEEu8; dst_pitch * h];
        decode_rgba8_pitched(format, &src, src_pitch, w, h, &mut dst, dst_pitch).unwrap();
        for y in 0..h {
            assert_eq!(
                dst[y * dst_pitch..y * dst_pitch + w * 4],
                expect[y * w * 4..(y + 1) * w * 4],
                "{format:?} row {y}"
            );
            if y + 1 < h {
                assert!(
                    dst[y * dst_pitch + w * 4..(y + 1) * dst_pitch]
                        .iter()
                        .all(|&b| b == 0xEE),
                    "{format:?} padding row {y}"
                );
            }
        }

        // the same for f32
        let expect = decode_rgba_f32_to_vec(format, &packed, w, h).unwrap();
        let dst_pitch = w * 4 + 5;
        let mut dst = vec![-7.0f32; dst_pitch * h];
        decode_rgba_f32_pitched(format, &src, src_pitch, w, h, &mut dst, dst_pitch).unwrap();
        for y in 0..h {
            for x in 0..w * 4 {
                let (a, b) = (dst[y * dst_pitch + x], expect[y * w * 4 + x]);
                assert!(a == b || (a.is_nan() && b.is_nan()), "{format:?}");
            }
            if y + 1 < h {
                assert!(dst[y * dst_pitch + w * 4..(y + 1) * dst_pitch]
                    .iter()
                    .all(|&v| v == -7.0));
            }
        }
    }
}

#[test]
fn zero_sized_surfaces_decode_nothing() {
    let mut out = [0u8; 0];
    for &format in &ALL {
        decode_rgba8(format, &[], 0, 0, &mut out).unwrap();
        decode_rgba8(format, &[], 0, 5, &mut out).unwrap();
        decode_rgba8(format, &[], 5, 0, &mut out).unwrap();
        assert!(decode_rgba8_to_vec(format, &[], 0, 0).unwrap().is_empty());
    }
}

#[test]
fn buffer_errors_are_reported_before_anything_is_written() {
    let src = [0u8; 8 * 4];
    let mut dst = vec![0x55u8; 8 * 8 * 4];
    // 8 x 8 BC1 needs 4 blocks = 32 bytes
    assert_eq!(
        decode_rgba8(Format::Bc1, &src[..31], 8, 8, &mut dst),
        Err(Error::SourceTooShort {
            needed: 32,
            got: 31
        })
    );
    assert_eq!(
        decode_rgba8(Format::Bc1, &src, 8, 8, &mut dst[..255]),
        Err(Error::DestinationTooShort {
            needed: 256,
            got: 255
        })
    );
    assert_eq!(
        decode_rgba8_pitched(Format::Bc1, &src, 15, 8, 8, &mut dst, 32),
        Err(Error::SourcePitchTooSmall {
            needed: 16,
            got: 15
        })
    );
    assert_eq!(
        decode_rgba8_pitched(Format::Bc1, &src, 16, 8, 8, &mut dst, 31),
        Err(Error::DestinationPitchTooSmall {
            needed: 32,
            got: 31
        })
    );
    assert!(
        dst.iter().all(|&b| b == 0x55),
        "nothing may be written on error"
    );
    // the Display text exists
    assert!(Error::SizeOverflow.to_string().contains("overflow"));
}

#[cfg(feature = "std")]
#[test]
fn the_error_type_implements_std_error() {
    let e: &dyn std::error::Error = &Error::UnsupportedOutput;
    assert!(!e.to_string().is_empty());
}

#[test]
fn absurd_sizes_are_errors_not_panics() {
    let mut dst = [0u8; 16];
    for &format in &ALL {
        for (w, h) in [
            (usize::MAX, 1),
            (1, usize::MAX),
            (usize::MAX, usize::MAX),
            (usize::MAX - 2, 4),
            (1 << 62, 1 << 62),
        ] {
            assert!(
                decode_rgba8(format, &[0; 16], w, h, &mut dst).is_err(),
                "{format:?} {w}x{h}"
            );
            assert!(
                decode_rgba8_to_vec(format, &[0; 16], w, h).is_err(),
                "{format:?} {w}x{h}"
            );
            assert!(decode_rgba_f32_to_vec(format, &[0; 16], w, h).is_err());
        }
        // a wide image with a pitch that overflows when multiplied by the number of rows
        assert!(decode_rgba8_pitched(format, &[0; 16], usize::MAX, 4, 8, &mut dst, 16).is_err());
        assert!(decode_rgba8_pitched(format, &[0; 16], 16, 4, 8, &mut dst, usize::MAX).is_err());
    }
}

#[test]
fn a_huge_size_with_a_tiny_source_does_not_allocate() {
    // Must fail on the source check before trying to allocate 2^46 bytes
    let r = decode_rgba8_to_vec(Format::Bc7, &[0; 16], 1 << 22, 1 << 22);
    assert!(matches!(r, Err(Error::SourceTooShort { .. })), "{r:?}");
}

#[test]
fn eight_bit_and_float_outputs_agree() {
    let mut rng = Rng(9);
    let (w, h) = (13usize, 10usize);
    for &format in &ALL {
        let mut src = vec![0u8; format.surface_bytes(w, h).unwrap()];
        rng.fill(&mut src);
        let a = decode_rgba8_to_vec(format, &src, w, h).unwrap();
        let f = decode_rgba_f32_to_vec(format, &src, w, h).unwrap();
        for (i, (&b, &v)) in a.iter().zip(f.iter()).enumerate() {
            match format {
                Format::Bc4Snorm | Format::Bc5Snorm => {
                    // R8G8B8A8_SNORM bytes: two's complement, 127 is 1.0; truncation towards zero
                    assert!(
                        (v * 127.0 - (b as i8) as f32).abs() < 1.0 + 1e-3,
                        "{format:?} {i}"
                    );
                }
                Format::Bc6hUf16 | Format::Bc6hSf16 => {
                    // clamped and truncated: floor(clamp(v, 0, 1) * 255); alpha 1.0 -> 255
                    let expect = if i % 4 == 3 {
                        255
                    } else {
                        (v.clamp(0.0, 1.0) * 255.0) as u8
                    };
                    let expect = if v.is_nan() { 0 } else { expect };
                    assert_eq!(b, expect, "{format:?} {i} v {v}");
                }
                Format::Bc4 | Format::Bc5 => {
                    assert!((v * 255.0 - b as f32).abs() < 1.0 + 1e-3, "{format:?} {i}");
                }
                _ => assert_eq!(b, (v * 255.0).round() as u8, "{format:?} {i}"),
            }
        }
    }
}

#[test]
fn bc6h_half_float_output() {
    let mut rng = Rng(10);
    let (w, h) = (9usize, 7usize);
    for format in [Format::Bc6hUf16, Format::Bc6hSf16] {
        let mut src = vec![0u8; format.surface_bytes(w, h).unwrap()];
        rng.fill(&mut src);
        let mut half = vec![0u16; w * h * 4];
        decode_bc6h_rgba_f16(format, &src, w, h, &mut half).unwrap();
        let f = decode_rgba_f32_to_vec(format, &src, w, h).unwrap();
        for (i, (&hh, &ff)) in half.iter().zip(f.iter()).enumerate() {
            let back = bcn_decode::bc6h::half_to_f32(hh);
            assert!(back == ff || (back.is_nan() && ff.is_nan()), "{i}");
            if i % 4 == 3 {
                assert_eq!(hh, 0x3C00);
            }
        }
        // pitched
        let mut padded = vec![0xBEEFu16; (w * 4 + 3) * h];
        decode_bc6h_rgba_f16_pitched(
            format,
            &src,
            w.div_ceil(4) * 16,
            w,
            h,
            &mut padded,
            w * 4 + 3,
        )
        .unwrap();
        for y in 0..h {
            assert_eq!(
                padded[y * (w * 4 + 3)..y * (w * 4 + 3) + w * 4],
                half[y * w * 4..(y + 1) * w * 4]
            );
        }
    }
    // other formats cannot produce half floats
    let mut out = [0u16; 64];
    assert_eq!(
        decode_bc6h_rgba_f16(Format::Bc7, &[0; 16], 4, 4, &mut out),
        Err(Error::UnsupportedOutput)
    );
    assert_eq!(
        decode_bc6h_rgba_f16(Format::Bc1, &[0; 8], 4, 4, &mut out),
        Err(Error::UnsupportedOutput)
    );
}

#[test]
fn format_specific_output_conventions() {
    let mut out = [0u8; 64];
    // BC4: (R, 0, 0, 255); codes all 0 -> red0
    let blk = [200, 13, 0, 0, 0, 0, 0, 0];
    decode_rgba8(Format::Bc4, &blk, 4, 4, &mut out).unwrap();
    assert_eq!(out[..4], [200, 0, 0, 255]);
    // BC5: (R, G, 0, 255)
    let blk = [200, 13, 0, 0, 0, 0, 0, 0, 90, 7, 0, 0, 0, 0, 0, 0];
    decode_rgba8(Format::Bc5, &blk, 4, 4, &mut out).unwrap();
    assert_eq!(out[..4], [200, 90, 0, 255]);
    // snorm: bytes of an R8G8B8A8_SNORM image, alpha 127
    let blk = [
        (-100i8) as u8,
        5,
        0,
        0,
        0,
        0,
        0,
        0,
        100,
        5,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    decode_rgba8(Format::Bc5Snorm, &blk, 4, 4, &mut out).unwrap();
    assert_eq!(out[..4], [(-100i8) as u8, 100, 0, 127]);
    // BC1 versus BC1 with alpha: only the three-colour mode's code 3 differs
    let blk = [0x1F, 0x00, 0x00, 0xF8, 0xFF, 0, 0, 0]; // blue <= red: three colours; texel 0..3 code 3
    decode_rgba8(Format::Bc1, &blk, 4, 4, &mut out).unwrap();
    assert_eq!(out[..4], [0, 0, 0, 255]);
    decode_rgba8(Format::Bc1Alpha, &blk, 4, 4, &mut out).unwrap();
    assert_eq!(out[..4], [0, 0, 0, 0]);
    // f32 of BC4 snorm: -1.0 .. 1.0
    let mut f = [0f32; 64];
    let blk = [(-127i8) as u8, 127, 0, 0, 0, 0, 0, 0];
    decode_rgba_f32(Format::Bc4Snorm, &blk, 4, 4, &mut f).unwrap();
    assert_eq!(f[..4], [-1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn widths_whose_row_overflows_are_errors_even_with_zero_height() {
    // width * 4 overflows for width >= 2^62: the to_vec helpers used to multiply unchecked when the
    // height was 0 (a panic with overflow checks on)
    for &format in &ALL {
        for width in [1usize << 62, (1 << 62) + 1, usize::MAX / 2, usize::MAX] {
            for height in [0usize, 1] {
                assert_eq!(
                    decode_rgba8_to_vec(format, &[], width, 0),
                    Err(Error::SizeOverflow),
                    "{format:?} {width}"
                );
                assert!(decode_rgba8_to_vec(format, &[], width, height).is_err());
                assert!(
                    decode_rgba_f32_to_vec(format, &[], width, 0).is_err(),
                    "{format:?} {width}"
                );
                assert!(decode_rgba_f32_to_vec(format, &[], width, height).is_err());
                let mut out = [0u8; 4];
                assert!(decode_rgba8(format, &[], width, 0, &mut out).is_err());
            }
        }
        // a wide but representable row: nothing overflows: zero height decodes nothing
        assert_eq!(decode_rgba8_to_vec(format, &[], 1 << 50, 0), Ok(Vec::new()));
    }
}
