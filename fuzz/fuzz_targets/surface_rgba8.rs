#![no_main]
//! decode_rgba8, decode_rgba8_pitched, decode_rgba8_to_vec with arbitrary sizes, pitches and bytes.

use bcn_decode::{decode_rgba8, decode_rgba8_pitched, decode_rgba8_to_vec};
use bcn_decode_fuzz::SurfaceInput;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: SurfaceInput| {
    let f = input.format();
    let (w, h) = (input.width.get(), input.height.get());
    let src = input.source();
    let tight = SurfaceInput::tight_src_pitch(f, w);

    // Allocating variant: the result must be justified by the source
    let v = decode_rgba8_to_vec(f, &src, w, h);
    if let Ok(v) = &v {
        // one block (at least 8 bytes) gives at most 16 texels of 4 bytes
        assert!(v.len() <= src.len().saturating_mul(8));
    }

    // Tightly packed, into a caller-chosen buffer filled with a sentinel
    let n = input.dst_elems(w.saturating_mul(4));
    let mut dst = vec![0xA5u8; n];
    let r = decode_rgba8(f, &src, w, h, &mut dst);
    if r.is_err() {
        assert!(dst.iter().all(|&b| b == 0xA5), "written despite an error");
    }
    if let Ok(v) = &v {
        // to_vec succeeded: the tight call into an exactly sized buffer agrees
        let mut exact = vec![0u8; v.len()];
        decode_rgba8(f, &src, w, h, &mut exact).unwrap();
        assert_eq!(&exact, v);
        if r.is_ok() && dst.len() == v.len() {
            assert_eq!(&dst, v);
        }
    }

    // Explicit pitches
    let sp = input.src_pitch.resolve(tight);
    let dp = input.dst_pitch.resolve(w.checked_mul(4));
    let n = input.dst_elems(dp);
    let mut dst2 = vec![0xA5u8; n];
    let r2 = decode_rgba8_pitched(f, &src, sp, w, h, &mut dst2, dp);
    if r2.is_err() {
        assert!(dst2.iter().all(|&b| b == 0xA5), "written despite an error");
    } else if h > 0 && w > 0 {
        // padding between rows is left alone
        let row = w * 4;
        for y in 0..h.saturating_sub(1) {
            assert!(dst2[y * dp + row..(y + 1) * dp].iter().all(|&b| b == 0xA5));
        }
        if let Ok(v) = &v {
            if tight == Some(sp) {
                for y in 0..h {
                    assert_eq!(&dst2[y * dp..y * dp + row], &v[y * row..(y + 1) * row]);
                }
            }
        }
    }
});
