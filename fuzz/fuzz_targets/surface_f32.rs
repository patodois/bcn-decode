#![no_main]
//! decode_rgba_f32, decode_rgba_f32_pitched, decode_rgba_f32_to_vec.

use bcn_decode::{decode_rgba_f32, decode_rgba_f32_pitched, decode_rgba_f32_to_vec};
use bcn_decode_fuzz::SurfaceInput;
use libfuzzer_sys::fuzz_target;

fn same(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
}

fuzz_target!(|input: SurfaceInput| {
    let f = input.format();
    let (w, h) = (input.width.get(), input.height.get());
    let src = input.source();
    let tight = SurfaceInput::tight_src_pitch(f, w);

    let v = decode_rgba_f32_to_vec(f, &src, w, h);
    if let Ok(v) = &v {
        // one block (at least 8 bytes) gives at most 16 texels of 4 floats
        assert!(v.len() <= src.len().saturating_mul(8));
    }

    let n = input.dst_elems(w.saturating_mul(4));
    let mut dst = vec![f32::from_bits(0x7FC0_1234); n];
    let r = decode_rgba_f32(f, &src, w, h, &mut dst);
    if r.is_err() {
        assert!(dst.iter().all(|x| x.to_bits() == 0x7FC0_1234), "written despite an error");
    }
    if let Ok(v) = &v {
        let mut exact = vec![0f32; v.len()];
        decode_rgba_f32(f, &src, w, h, &mut exact).unwrap();
        assert!(same(&exact, v));
    }

    let sp = input.src_pitch.resolve(tight);
    let dp = input.dst_pitch.resolve(w.checked_mul(4));
    let n = input.dst_elems(dp);
    let mut dst2 = vec![f32::from_bits(0x7FC0_1234); n];
    let r2 = decode_rgba_f32_pitched(f, &src, sp, w, h, &mut dst2, dp);
    if r2.is_err() {
        assert!(dst2.iter().all(|x| x.to_bits() == 0x7FC0_1234), "written despite an error");
    } else if h > 0 && w > 0 {
        if let Ok(v) = &v {
            if tight == Some(sp) {
                let row = w * 4;
                for y in 0..h {
                    assert!(same(&dst2[y * dp..y * dp + row], &v[y * row..(y + 1) * row]));
                }
            }
        }
    }
});
