use core::fmt;

/// Why a surface could not be decoded.
///
/// The decoders never fail on the *content* of a block: every bit pattern decodes to something
/// (the specifications define what reserved encodings return). Only the buffers handed in can be
/// wrong, and those are all checked before any pixel is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A size computed from the width, height or pitches does not fit in `usize`.
    SizeOverflow,
    /// The source pitch (bytes from one row of blocks to the next) is smaller than a row of
    /// blocks.
    SourcePitchTooSmall {
        /// Bytes needed for one row of blocks.
        needed: usize,
        /// The pitch given.
        got: usize,
    },
    /// The source slice does not hold all the blocks of the surface.
    SourceTooShort {
        /// Bytes needed.
        needed: usize,
        /// Bytes given.
        got: usize,
    },
    /// The destination pitch (elements from one row of pixels to the next) is smaller than one
    /// row of pixels.
    DestinationPitchTooSmall {
        /// Elements needed for one row (width times 4).
        needed: usize,
        /// The pitch given.
        got: usize,
    },
    /// The destination slice cannot hold the decoded surface.
    DestinationTooShort {
        /// Elements needed.
        needed: usize,
        /// Elements given.
        got: usize,
    },
    /// The format cannot produce the requested output type (for example half floats from BC7).
    UnsupportedOutput,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Error::SizeOverflow => f.write_str("surface size overflows usize"),
            Error::SourcePitchTooSmall { needed, got } => {
                write!(
                    f,
                    "source pitch {got} is smaller than a row of blocks ({needed} bytes)"
                )
            }
            Error::SourceTooShort { needed, got } => {
                write!(f, "source holds {got} bytes but the surface needs {needed}")
            }
            Error::DestinationPitchTooSmall { needed, got } => {
                write!(
                    f,
                    "destination pitch {got} is smaller than a row of pixels ({needed} elements)"
                )
            }
            Error::DestinationTooShort { needed, got } => {
                write!(
                    f,
                    "destination holds {got} elements but the surface needs {needed}"
                )
            }
            Error::UnsupportedOutput => {
                f.write_str("this format cannot produce the requested output type")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}
