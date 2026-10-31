//! Shared input model of the fuzz targets (nightly, cargo-fuzz only; not part of the published crate).

use arbitrary::Arbitrary;
use bcn_decode::Format;

pub const FORMATS: [Format; 11] = [
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

/// A dimension or pitch: mostly small, but also huge and next to `usize::MAX`.
#[derive(Arbitrary, Debug, Clone, Copy)]
pub enum Dim {
    Small(u8),
    Mid(u16),
    Any(u64),
    /// `usize::MAX - n`
    Top(u8),
    /// `1 << n`, one more or one less
    Pow2(u8, i8),
}

impl Dim {
    pub fn get(self) -> usize {
        match self {
            Dim::Small(n) => (n % 70) as usize,
            Dim::Mid(n) => n as usize,
            Dim::Any(n) => n as usize,
            Dim::Top(n) => usize::MAX - n as usize,
            Dim::Pow2(s, d) => (1usize << (s as u32 % usize::BITS)).wrapping_add_signed(d as isize),
        }
    }
}

/// How the pitches of a `_pitched` call are chosen.
#[derive(Arbitrary, Debug, Clone, Copy)]
pub enum Pitch {
    /// The tightly packed pitch.
    Tight,
    /// Tight pitch plus a small padding.
    Padded(u8),
    /// Anything.
    Free(Dim),
}

impl Pitch {
    pub fn resolve(self, tight: Option<usize>) -> usize {
        match self {
            Pitch::Tight => tight.unwrap_or(0),
            Pitch::Padded(p) => tight.unwrap_or(0).saturating_add(p as usize),
            Pitch::Free(d) => d.get(),
        }
    }
}

/// Everything a surface call needs.
#[derive(Arbitrary, Debug)]
pub struct SurfaceInput {
    pub format: u8,
    pub width: Dim,
    pub height: Dim,
    pub src_pitch: Pitch,
    pub dst_pitch: Pitch,
    /// Destination length: exact when the surface is small, otherwise one of a few values.
    pub dst_len: Dim,
    pub exact_dst: bool,
    /// Repeat `data` until the source is exactly as long as the surface needs (when that is small).
    pub tile_src: bool,
    pub data: Vec<u8>,
}

/// The harness never allocates more than this many elements for a destination it chooses.
pub const MAX_DST: usize = 1 << 16;

impl SurfaceInput {
    pub fn format(&self) -> Format {
        FORMATS[self.format as usize % FORMATS.len()]
    }

    /// The source slice, tiled up to what the surface needs if requested and affordable.
    pub fn source(&self) -> Vec<u8> {
        let mut src = self.data.clone();
        if self.tile_src && !src.is_empty() {
            let pitch = self
                .src_pitch
                .resolve(Self::tight_src_pitch(self.format(), self.width.get()));
            let by = self.height.get().div_ceil(4);
            if let Some(need) = by.checked_mul(pitch) {
                if need <= (1 << 16) {
                    let mut i = 0;
                    while src.len() < need {
                        src.push(self.data[i % self.data.len()]);
                        i += 1;
                    }
                }
            }
        }
        src
    }

    pub fn tight_src_pitch(f: Format, width: usize) -> Option<usize> {
        width.div_ceil(4).checked_mul(f.block_bytes())
    }

    /// Length of the destination slice the harness hands over.
    pub fn dst_elems(&self, pitch: usize) -> usize {
        let h = self.height.get();
        let exact = if self.exact_dst && h > 0 {
            pitch
                .checked_mul(h - 1)
                .and_then(|n| n.checked_add(self.width.get().checked_mul(4)?))
        } else {
            None
        };
        match exact {
            Some(n) if n <= MAX_DST => n,
            _ => self.dst_len.get().min(MAX_DST),
        }
    }
}
