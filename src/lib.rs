//! Block-compression texture decoder for BC1, BC2, BC3, BC4, BC5, BC6H and BC7.
//!
//! Implemented from the public specifications: Microsoft's Direct3D 10 and 11 block compression
//! documentation and the Khronos Data Format Specification 1.3.1 (sections 18 S3TC, 19 RGTC, 20
//! BPTC). Every rule says in a comment which section it comes from. Provenance: the source of
//! `texture2ddecoder`, bcdec, DirectXTex and other decoders was not read. Part of wf-unpack's
//! `bc6h.rs` (its mode table, its unquantizing helpers and its notes on two `texture2ddecoder`
//! bugs; that file derived from `texture2ddecoder` and bcdec) was read first and later served as a
//! cross-check. `texture2ddecoder` was run as a black box (outputs only, plus the signatures in
//! its generated documentation) to compare results; see `docs/oracle-results.md`.
//!
//! * [`decode_rgba8`] decodes a whole surface to 8-bit RGBA; [`decode_rgba_f32`] to `f32`
//!   (BC6H keeps its full half-float range there); [`decode_bc6h_rgba_f16`] gives the raw
//!   half-float bit patterns. The `_pitched` variants take row pitches. Widths and heights that
//!   are not multiples of 4 are fine: the blocks at the right and bottom edges are cropped.
//! * Each format also has a block decoder in its module ([`s3tc`], [`rgtc`], [`bc6h`], [`bc7`])
//!   that returns the 16 texels of one block in raster order.
//! * Nothing here panics on malformed input and there is no `unsafe`. Every bit pattern of a
//!   block decodes to something (the specifications define the result of reserved encodings);
//!   only buffers that are too small are errors.
//!
//! # Output conventions
//!
//! | Format | RGBA8 texel | `f32` texel |
//! |---|---|---|
//! | [`Format::Bc1`] | opaque; index 3 of the three-colour mode is black | same |
//! | [`Format::Bc1Alpha`] | index 3 of the three-colour mode is (0, 0, 0, 0) | same |
//! | [`Format::Bc2`], [`Format::Bc3`] | RGBA | RGBA |
//! | [`Format::Bc4`], [`Format::Bc5`] | (R, 0, 0, 255), (R, G, 0, 255) | (R, 0, 0, 1), (R, G, 0, 1) |
//! | [`Format::Bc4Snorm`], [`Format::Bc5Snorm`] | `R8G8B8A8_SNORM` bytes (two's complement, alpha 127) | -1.0 to 1.0 |
//! | [`Format::Bc6hUf16`], [`Format::Bc6hSf16`] | clamped to [0, 1] (HDR above 1.0 is lost), alpha 255 | the exact half-float values, alpha 1.0 |
//! | [`Format::Bc7`] | RGBA | RGBA / 255 |
//!
//! BC1 to BC5 have no bit-exact 8-bit definition (the specifications give real values); see the
//! module documentation of [`s3tc`] and [`rgtc`] for the rounding used. BC6H and BC7 are bit-exact.
//!
//! # Example
//!
//! ```
//! use bcn_decode::{decode_rgba8, Format};
//!
//! // One BC1 block: color0 = pure red (0xF800), color1 = pure blue (0x001F), all indices 0
//! let block = [0x00, 0xF8, 0x1F, 0x00, 0, 0, 0, 0];
//! let mut rgba = [0u8; 4 * 4 * 4];
//! decode_rgba8(Format::Bc1, &block, 4, 4, &mut rgba).unwrap();
//! assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod bc6h;
pub mod bc7;
mod error;
pub mod rgtc;
pub mod s3tc;
mod surface;
#[rustfmt::skip]
mod tables;

pub use error::Error;
pub use surface::{
    decode_bc6h_rgba_f16, decode_bc6h_rgba_f16_pitched, decode_rgba8, decode_rgba8_pitched,
    decode_rgba8_to_vec, decode_rgba_f32, decode_rgba_f32_pitched, decode_rgba_f32_to_vec,
};

/// A block-compressed format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    /// BC1 / DXT1 without alpha ("BC1 with no alpha", DFS 18.1): every texel is opaque.
    Bc1,
    /// BC1 / DXT1 with 1-bit alpha ("BC1 with alpha", DFS 18.2).
    Bc1Alpha,
    /// BC2 / DXT3: explicit 4-bit alpha.
    Bc2,
    /// BC3 / DXT5: interpolated alpha.
    Bc3,
    /// BC4 unsigned (ATI1): one channel.
    Bc4,
    /// BC4 signed.
    Bc4Snorm,
    /// BC5 unsigned (ATI2): two channels.
    Bc5,
    /// BC5 signed.
    Bc5Snorm,
    /// BC6H unsigned half floats (`DXGI_FORMAT_BC6H_UF16`).
    Bc6hUf16,
    /// BC6H signed half floats (`DXGI_FORMAT_BC6H_SF16`).
    Bc6hSf16,
    /// BC7.
    Bc7,
}

impl Format {
    /// Bytes per 4x4 block: 8 for BC1 and BC4, 16 for the others.
    pub const fn block_bytes(self) -> usize {
        match self {
            Format::Bc1 | Format::Bc1Alpha | Format::Bc4 | Format::Bc4Snorm => 8,
            _ => 16,
        }
    }

    /// Size in bytes of a tightly packed surface of this format, `None` if it overflows.
    ///
    /// ```
    /// assert_eq!(bcn_decode::Format::Bc7.surface_bytes(5, 5), Some(64));
    /// ```
    pub const fn surface_bytes(self, width: usize, height: usize) -> Option<usize> {
        let bx = match width.checked_add(3) {
            Some(v) => v / 4,
            None => return None,
        };
        let by = match height.checked_add(3) {
            Some(v) => v / 4,
            None => return None,
        };
        match bx.checked_mul(by) {
            Some(n) => n.checked_mul(self.block_bytes()),
            None => None,
        }
    }
}
