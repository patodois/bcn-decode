#![no_main]
//! BC4 and BC5 block decoders (unsigned and signed, 8-bit and f32 outputs).

use bcn_decode::{decode_rgba8, decode_rgba_f32, rgtc, Format};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|b: [u8; 16]| {
    let b8: [u8; 8] = b[..8].try_into().unwrap();
    let _ = rgtc::decode_unorm_block(&b8);
    let _ = rgtc::decode_snorm_block(&b8);
    let _ = rgtc::decode_unorm_block_f32(&b8);
    let _ = rgtc::decode_snorm_block_f32(&b8);

    let flat = |t: [[u8; 4]; 16]| t.as_flattened().to_vec();
    let mut out = [0u8; 64];
    let mut outf = [0f32; 64];
    for (fmt, src, want) in [
        (Format::Bc4, &b[..8], flat(rgtc::decode_bc4_block(&b8))),
        (Format::Bc4Snorm, &b[..8], flat(rgtc::decode_bc4_snorm_block(&b8))),
        (Format::Bc5, &b[..], flat(rgtc::decode_bc5_block(&b))),
        (Format::Bc5Snorm, &b[..], flat(rgtc::decode_bc5_snorm_block(&b))),
    ] {
        decode_rgba8(fmt, src, 4, 4, &mut out).unwrap();
        assert_eq!(&out[..], &want[..]);
    }
    for (fmt, src, want) in [
        (Format::Bc4, &b[..8], rgtc::decode_bc4_block_f32(&b8)),
        (Format::Bc4Snorm, &b[..8], rgtc::decode_bc4_snorm_block_f32(&b8)),
        (Format::Bc5, &b[..], rgtc::decode_bc5_block_f32(&b)),
        (Format::Bc5Snorm, &b[..], rgtc::decode_bc5_snorm_block_f32(&b)),
    ] {
        decode_rgba_f32(fmt, src, 4, 4, &mut outf).unwrap();
        for (x, y) in outf.iter().zip(want.as_flattened()) {
            assert_eq!(x.to_bits(), y.to_bits());
        }
    }
});
