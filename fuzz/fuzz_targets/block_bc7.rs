#![no_main]
//! BC7 block decoder; also checks that a 4x4 surface equals the block decoder.

use bcn_decode::{bc7, decode_rgba8, Format};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|b: [u8; 16]| {
    let t = bc7::decode_block(&b);
    let mut out = [0u8; 64];
    decode_rgba8(Format::Bc7, &b, 4, 4, &mut out).unwrap();
    assert_eq!(&out[..], t.as_flattened());
});
