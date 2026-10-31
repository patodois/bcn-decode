#![no_main]
//! decode_bc6h_rgba_f16 and decode_bc6h_rgba_f16_pitched, for every Format (non-BC6H must be an error).

use bcn_decode::{
    decode_bc6h_rgba_f16, decode_bc6h_rgba_f16_pitched, decode_rgba_f32_to_vec, Error, Format,
};
use bcn_decode_fuzz::SurfaceInput;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: SurfaceInput| {
    let f = input.format();
    let (w, h) = (input.width.get(), input.height.get());
    let src = input.source();
    let tight = SurfaceInput::tight_src_pitch(f, w);
    let is_bc6h = matches!(f, Format::Bc6hUf16 | Format::Bc6hSf16);

    let n = input.dst_elems(w.saturating_mul(4));
    let mut dst = vec![0xBEEFu16; n];
    let r = decode_bc6h_rgba_f16(f, &src, w, h, &mut dst);
    if !is_bc6h {
        // The tight variant derives the pitches first, so a width that overflows is reported as
        // SizeOverflow before the format is looked at (see CLOUD-REPORT.md); both are errors.
        assert!(matches!(r, Err(Error::UnsupportedOutput | Error::SizeOverflow)));
    }
    if r.is_err() {
        assert!(dst.iter().all(|&x| x == 0xBEEF), "written despite an error");
    } else if is_bc6h && w > 0 && h > 0 {
        // agrees with the f32 path (half floats are exactly representable as f32)
        let v = decode_rgba_f32_to_vec(f, &src, w, h).unwrap();
        for (a, b) in dst.iter().zip(&v) {
            let want = bcn_decode::bc6h::half_to_f32(*a);
            assert!(want.to_bits() == b.to_bits() || (want.is_nan() && b.is_nan()));
        }
    }

    let sp = input.src_pitch.resolve(tight);
    let dp = input.dst_pitch.resolve(w.checked_mul(4));
    let n = input.dst_elems(dp);
    let mut dst2 = vec![0xBEEFu16; n];
    let r2 = decode_bc6h_rgba_f16_pitched(f, &src, sp, w, h, &mut dst2, dp);
    if !is_bc6h {
        assert_eq!(r2, Err(Error::UnsupportedOutput));
    }
    if r2.is_err() {
        assert!(dst2.iter().all(|&x| x == 0xBEEF), "written despite an error");
    } else if w > 0 && h > 1 {
        let row = w * 4;
        for y in 0..h - 1 {
            assert!(dst2[y * dp + row..(y + 1) * dp].iter().all(|&x| x == 0xBEEF));
        }
    }
});
