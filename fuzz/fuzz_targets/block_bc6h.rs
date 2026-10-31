#![no_main]
//! BC6H block decoder: half, f32 and RGBA8 outputs must agree with each other.

use bcn_decode::bc6h;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|b: [u8; 16]| {
    for signed in [false, true] {
        let info = bc6h::parse_block(&b, signed);
        let half = bc6h::decode_block_half(&b, signed);
        if info.is_none() {
            assert_eq!(half, [[0u16; 3]; 16]);
        }
        let f = bc6h::decode_block_f32(&b, signed);
        let u = bc6h::decode_block_rgba8(&b, signed);
        for i in 0..16 {
            assert_eq!(f[i][3], 1.0);
            assert_eq!(u[i][3], 255);
            for c in 0..3 {
                let x = bc6h::half_to_f32(half[i][c]);
                assert!(f[i][c].to_bits() == x.to_bits());
                // half_to_unorm8 documents "clamped to [0, 1], times 255, rounded down"
                let want = if x.is_nan() || x <= 0.0 {
                    0
                } else if x >= 1.0 {
                    255
                } else {
                    (x * 255.0) as u8
                };
                assert_eq!(u[i][c], want);
            }
        }
    }
});
