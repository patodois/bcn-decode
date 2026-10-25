//! Whole-surface decoding: walks the blocks, handles pitches and cropped edge blocks.

use alloc::vec::Vec;

use crate::{bc6h, bc7, rgtc, s3tc, Error, Format};

/// Sizes of a surface, validated against the buffers.
struct Geometry {
    blocks_x: usize,
    blocks_y: usize,
}

/// Checks all the buffer sizes. Nothing is written before this has succeeded.
fn check(
    format: Format,
    src_len: usize,
    src_pitch: usize,
    width: usize,
    height: usize,
    dst_len: usize,
    dst_pitch: usize,
) -> Result<Geometry, Error> {
    let blocks_x = width.checked_add(3).ok_or(Error::SizeOverflow)? / 4;
    let blocks_y = height.checked_add(3).ok_or(Error::SizeOverflow)? / 4;
    if blocks_x == 0 || blocks_y == 0 {
        return Ok(Geometry {
            blocks_x: 0,
            blocks_y: 0,
        });
    }
    let row_bytes = blocks_x
        .checked_mul(format.block_bytes())
        .ok_or(Error::SizeOverflow)?;
    if src_pitch < row_bytes {
        return Err(Error::SourcePitchTooSmall {
            needed: row_bytes,
            got: src_pitch,
        });
    }
    let src_needed = (blocks_y - 1)
        .checked_mul(src_pitch)
        .and_then(|n| n.checked_add(row_bytes))
        .ok_or(Error::SizeOverflow)?;
    if src_len < src_needed {
        return Err(Error::SourceTooShort {
            needed: src_needed,
            got: src_len,
        });
    }
    let row_elems = width.checked_mul(4).ok_or(Error::SizeOverflow)?;
    if dst_pitch < row_elems {
        return Err(Error::DestinationPitchTooSmall {
            needed: row_elems,
            got: dst_pitch,
        });
    }
    let dst_needed = (height - 1)
        .checked_mul(dst_pitch)
        .and_then(|n| n.checked_add(row_elems))
        .ok_or(Error::SizeOverflow)?;
    if dst_len < dst_needed {
        return Err(Error::DestinationTooShort {
            needed: dst_needed,
            got: dst_len,
        });
    }
    Ok(Geometry { blocks_x, blocks_y })
}

/// Decodes every block with `decode` and stores the texels that lie inside the image.
#[allow(clippy::too_many_arguments)]
fn run<T: Copy, const N: usize>(
    geo: &Geometry,
    src: &[u8],
    src_pitch: usize,
    width: usize,
    height: usize,
    dst: &mut [T],
    dst_pitch: usize,
    decode: impl Fn(&[u8; N]) -> [[T; 4]; 16],
) {
    for by in 0..geo.blocks_y {
        let start = by * src_pitch;
        let Some(row) = src.get(start..start + geo.blocks_x * N) else {
            return;
        };
        let y0 = by * 4;
        let rows = (height - y0).min(4);
        for (bx, block) in row.as_chunks::<N>().0.iter().enumerate() {
            let texels = decode(block);
            let x0 = bx * 4;
            let cols = (width - x0).min(4);
            let flat = texels.as_flattened();
            for ty in 0..rows {
                let at = (y0 + ty) * dst_pitch + x0 * 4;
                let Some(line) = dst.get_mut(at..at + cols * 4) else {
                    return;
                };
                // Blocks inside the image copy a whole row of four texels at once
                line.copy_from_slice(&flat[ty * 16..ty * 16 + cols * 4]);
            }
        }
    }
}

fn to_f32(t: [[u8; 4]; 16]) -> [[f32; 4]; 16] {
    let mut out = [[0f32; 4]; 16];
    for (o, p) in out.iter_mut().zip(t.iter()) {
        for c in 0..4 {
            o[c] = p[c] as f32 / 255.0;
        }
    }
    out
}

/// Decodes a surface of `width` x `height` texels to 8-bit RGBA with explicit row pitches.
///
/// `src_pitch` is the number of bytes from one row of blocks to the next (at least
/// `ceil(width / 4) * format.block_bytes()`); `dst_pitch` is the number of bytes from one row of
/// pixels to the next (at least `width * 4`). Zero width or height decodes nothing. See the
/// crate documentation for what each format produces.
///
/// # Errors
///
/// A pitch too small for the width, a source or destination slice too short, or a size that
/// overflows. Nothing is written in those cases.
pub fn decode_rgba8_pitched(
    format: Format,
    src: &[u8],
    src_pitch: usize,
    width: usize,
    height: usize,
    dst: &mut [u8],
    dst_pitch: usize,
) -> Result<(), Error> {
    let g = check(
        format,
        src.len(),
        src_pitch,
        width,
        height,
        dst.len(),
        dst_pitch,
    )?;
    macro_rules! go {
        ($n:literal, $f:expr) => {
            run::<u8, $n>(&g, src, src_pitch, width, height, dst, dst_pitch, $f)
        };
    }
    match format {
        Format::Bc1 => go!(8, |b| s3tc::decode_bc1_block(b, false)),
        Format::Bc1Alpha => go!(8, |b| s3tc::decode_bc1_block(b, true)),
        Format::Bc2 => go!(16, s3tc::decode_bc2_block),
        Format::Bc3 => go!(16, s3tc::decode_bc3_block),
        Format::Bc4 => go!(8, rgtc::decode_bc4_block),
        Format::Bc4Snorm => go!(8, rgtc::decode_bc4_snorm_block),
        Format::Bc5 => go!(16, rgtc::decode_bc5_block),
        Format::Bc5Snorm => go!(16, rgtc::decode_bc5_snorm_block),
        Format::Bc6hUf16 => go!(16, |b| bc6h::decode_block_rgba8(b, false)),
        Format::Bc6hSf16 => go!(16, |b| bc6h::decode_block_rgba8(b, true)),
        Format::Bc7 => go!(16, bc7::decode_block),
    }
    Ok(())
}

/// Decodes a tightly packed surface to 8-bit RGBA (`width * 4` bytes per row).
///
/// # Errors
///
/// See [`decode_rgba8_pitched`].
pub fn decode_rgba8(
    format: Format,
    src: &[u8],
    width: usize,
    height: usize,
    dst: &mut [u8],
) -> Result<(), Error> {
    let src_pitch = width
        .div_ceil(4)
        .checked_mul(format.block_bytes())
        .ok_or(Error::SizeOverflow)?;
    let dst_pitch = width.checked_mul(4).ok_or(Error::SizeOverflow)?;
    decode_rgba8_pitched(format, src, src_pitch, width, height, dst, dst_pitch)
}

/// Decodes a tightly packed surface to a new `Vec` of 8-bit RGBA.
///
/// # Errors
///
/// See [`decode_rgba8_pitched`]; the source is checked before anything is allocated.
pub fn decode_rgba8_to_vec(
    format: Format,
    src: &[u8],
    width: usize,
    height: usize,
) -> Result<Vec<u8>, Error> {
    let src_pitch = width
        .div_ceil(4)
        .checked_mul(format.block_bytes())
        .ok_or(Error::SizeOverflow)?;
    let len = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .ok_or(Error::SizeOverflow)?;
    // Validate the source before allocating, so that a huge size cannot make us allocate
    check(format, src.len(), src_pitch, width, height, len, width * 4)?;
    let mut out = alloc::vec![0u8; len];
    decode_rgba8(format, src, width, height, &mut out)?;
    Ok(out)
}

/// Decodes a surface to `f32` RGBA with explicit row pitches. Pitches are in **bytes** for
/// `src_pitch` and in **`f32` elements** for `dst_pitch` (at least `width * 4`).
///
/// # Errors
///
/// See [`decode_rgba8_pitched`].
pub fn decode_rgba_f32_pitched(
    format: Format,
    src: &[u8],
    src_pitch: usize,
    width: usize,
    height: usize,
    dst: &mut [f32],
    dst_pitch: usize,
) -> Result<(), Error> {
    let g = check(
        format,
        src.len(),
        src_pitch,
        width,
        height,
        dst.len(),
        dst_pitch,
    )?;
    macro_rules! go {
        ($n:literal, $f:expr) => {
            run::<f32, $n>(&g, src, src_pitch, width, height, dst, dst_pitch, $f)
        };
    }
    match format {
        Format::Bc1 => go!(8, |b| to_f32(s3tc::decode_bc1_block(b, false))),
        Format::Bc1Alpha => go!(8, |b| to_f32(s3tc::decode_bc1_block(b, true))),
        Format::Bc2 => go!(16, |b| to_f32(s3tc::decode_bc2_block(b))),
        Format::Bc3 => go!(16, |b| to_f32(s3tc::decode_bc3_block(b))),
        Format::Bc4 => go!(8, rgtc::decode_bc4_block_f32),
        Format::Bc4Snorm => go!(8, rgtc::decode_bc4_snorm_block_f32),
        Format::Bc5 => go!(16, rgtc::decode_bc5_block_f32),
        Format::Bc5Snorm => go!(16, rgtc::decode_bc5_snorm_block_f32),
        Format::Bc6hUf16 => go!(16, |b| bc6h::decode_block_f32(b, false)),
        Format::Bc6hSf16 => go!(16, |b| bc6h::decode_block_f32(b, true)),
        Format::Bc7 => go!(16, |b| to_f32(bc7::decode_block(b))),
    }
    Ok(())
}

/// Decodes a tightly packed surface to `f32` RGBA (`width * 4` elements per row).
///
/// # Errors
///
/// See [`decode_rgba8_pitched`].
pub fn decode_rgba_f32(
    format: Format,
    src: &[u8],
    width: usize,
    height: usize,
    dst: &mut [f32],
) -> Result<(), Error> {
    let src_pitch = width
        .div_ceil(4)
        .checked_mul(format.block_bytes())
        .ok_or(Error::SizeOverflow)?;
    let dst_pitch = width.checked_mul(4).ok_or(Error::SizeOverflow)?;
    decode_rgba_f32_pitched(format, src, src_pitch, width, height, dst, dst_pitch)
}

/// Decodes a tightly packed surface to a new `Vec` of `f32` RGBA.
///
/// # Errors
///
/// See [`decode_rgba8_to_vec`].
pub fn decode_rgba_f32_to_vec(
    format: Format,
    src: &[u8],
    width: usize,
    height: usize,
) -> Result<Vec<f32>, Error> {
    let src_pitch = width
        .div_ceil(4)
        .checked_mul(format.block_bytes())
        .ok_or(Error::SizeOverflow)?;
    let len = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .ok_or(Error::SizeOverflow)?;
    check(format, src.len(), src_pitch, width, height, len, width * 4)?;
    let mut out = alloc::vec![0f32; len];
    decode_rgba_f32(format, src, width, height, &mut out)?;
    Ok(out)
}

/// Decodes a BC6H surface to the raw half-float bit patterns (`u16`, RGBA with alpha `0x3C00`
/// = 1.0), with explicit pitches (`src_pitch` in bytes, `dst_pitch` in `u16` elements).
///
/// `format` must be [`Format::Bc6hUf16`] or [`Format::Bc6hSf16`].
///
/// # Errors
///
/// [`Error::UnsupportedOutput`] for any other format, otherwise as [`decode_rgba8_pitched`].
pub fn decode_bc6h_rgba_f16_pitched(
    format: Format,
    src: &[u8],
    src_pitch: usize,
    width: usize,
    height: usize,
    dst: &mut [u16],
    dst_pitch: usize,
) -> Result<(), Error> {
    let signed = match format {
        Format::Bc6hUf16 => false,
        Format::Bc6hSf16 => true,
        _ => return Err(Error::UnsupportedOutput),
    };
    let g = check(
        format,
        src.len(),
        src_pitch,
        width,
        height,
        dst.len(),
        dst_pitch,
    )?;
    run::<u16, 16>(&g, src, src_pitch, width, height, dst, dst_pitch, |b| {
        let h = bc6h::decode_block_half(b, signed);
        let mut out = [[0, 0, 0, bc6h::HALF_ONE]; 16];
        for (o, p) in out.iter_mut().zip(h.iter()) {
            o[..3].copy_from_slice(p);
        }
        out
    });
    Ok(())
}

/// Decodes a tightly packed BC6H surface to half-float bit patterns (`width * 4` elements per row).
///
/// # Errors
///
/// See [`decode_bc6h_rgba_f16_pitched`].
pub fn decode_bc6h_rgba_f16(
    format: Format,
    src: &[u8],
    width: usize,
    height: usize,
    dst: &mut [u16],
) -> Result<(), Error> {
    let src_pitch = width
        .div_ceil(4)
        .checked_mul(format.block_bytes())
        .ok_or(Error::SizeOverflow)?;
    let dst_pitch = width.checked_mul(4).ok_or(Error::SizeOverflow)?;
    decode_bc6h_rgba_f16_pitched(format, src, src_pitch, width, height, dst, dst_pitch)
}
