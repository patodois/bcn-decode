#![no_main]
//! BC1, BC2, BC3 block decoders; also checks that a 4x4 surface equals the block decoder.

use bcn_decode::{decode_rgba8, s3tc, Format};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|b: [u8; 16]| {
    let b8: [u8; 8] = b[..8].try_into().unwrap();
    let flat = |t: [[u8; 4]; 16]| t.as_flattened().to_vec();
    let mut out = [0u8; 64];
    for (fmt, want) in [
        (Format::Bc1, flat(s3tc::decode_bc1_block(&b8, false))),
        (Format::Bc1Alpha, flat(s3tc::decode_bc1_block(&b8, true))),
    ] {
        decode_rgba8(fmt, &b8, 4, 4, &mut out).unwrap();
        assert_eq!(&out[..], &want[..]);
    }
    for (fmt, want) in [
        (Format::Bc2, flat(s3tc::decode_bc2_block(&b))),
        (Format::Bc3, flat(s3tc::decode_bc3_block(&b))),
    ] {
        decode_rgba8(fmt, &b, 4, 4, &mut out).unwrap();
        assert_eq!(&out[..], &want[..]);
    }
});
