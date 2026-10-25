# bcn-decode

A decoder for block-compressed textures: BC1, BC2, BC3, BC4, BC5, BC6H and BC7 (the formats of
DDS files, Direct3D 10 and 11, and `GL_EXT_texture_compression_s3tc`, `_rgtc` and `_bptc`).

It is implemented from the public specifications (Microsoft's Direct3D block compression
documentation and the Khronos Data Format Specification), with the section of the specification
named in a comment next to each rule. Provenance, exactly: the source code of `texture2ddecoder`,
bcdec, DirectXTex and other decoders was not read. Part of wf-unpack's `bc6h.rs` was read (its mode
table, its unquantizing helpers and its notes on two `texture2ddecoder` bugs; that file was derived from
`texture2ddecoder` and bcdec) before the BC6H decoder was written, and it served as a cross-check
afterwards (the two agree on millions of blocks). `texture2ddecoder` was only used as a black box:
its outputs were compared with ours, and its function signatures were read from its generated
documentation. The causes given below for its differences come from those comparisons and from the
specifications. It has no
dependencies, no `unsafe` (`#![forbid(unsafe_code)]`), is `no_std` + `alloc` with the `std` feature off,
and does not panic on malformed input: every bit pattern of a block decodes to something, and only
buffers that are too small are errors.

```rust
use bcn_decode::{decode_rgba8_to_vec, Format};

let rgba = decode_rgba8_to_vec(Format::Bc7, &blocks, width, height)?; // width * height * 4 bytes
```

## What it does

| Format | `Format` | 8-bit output | `f32` output |
|---|---|---|---|
| BC1 / DXT1 | `Bc1` (opaque), `Bc1Alpha` (1-bit alpha) | RGBA | RGBA |
| BC2 / DXT3 | `Bc2` | RGBA | RGBA |
| BC3 / DXT5 | `Bc3` | RGBA | RGBA |
| BC4 | `Bc4`, `Bc4Snorm` | (R, 0, 0, 255); snorm: `R8G8B8A8_SNORM` bytes | (R, 0, 0, 1) |
| BC5 | `Bc5`, `Bc5Snorm` | (R, G, 0, 255); snorm: `R8G8B8A8_SNORM` bytes | (R, G, 0, 1) |
| BC6H | `Bc6hUf16`, `Bc6hSf16` | clamped to [0, 1] | the half-float values, alpha 1.0 |
| BC7 | `Bc7` | RGBA | RGBA / 255 |

* `decode_rgba8`, `decode_rgba_f32` and `decode_bc6h_rgba_f16` (raw half-float bits) decode a
  whole surface into a slice; the `_pitched` variants take the byte pitch of the source rows of
  blocks and the pitch of the destination rows; the `_to_vec` variants allocate.
* Widths and heights that are not multiples of 4 work: the edge blocks are cropped.
* Every format has a block decoder in its module (`s3tc`, `rgtc`, `bc6h`, `bc7`) that returns the 16
  texels of one block.
* `bc6h::parse_block` returns the mode, partition and endpoints of a BC6H block.

## Where the specifications leave a choice

* **Signed BC6H, interpolated -1.** The DFS 20.2 text sets the sign bit from the unscaled value, which
  makes -1 negative zero (0x8000); Microsoft's `finish_unquantize` scales the magnitude first and sets the sign
  only if the scaled value is negative, giving +0 (0x0000). Direct3D requires bit-exact hardware, so this
  crate follows Direct3D.

* **BC1 to BC5, 8-bit output.** The specifications define real values such as
  `(2 * RGB0 + RGB1) / 3` and leave the conversion to 8 bits to the implementation (Direct3D only
  requires bit-exact decoding for BC6H and BC7). Endpoints are widened by replicating their top
  bits, the in-between colours are computed on the 8-bit values and the division rounds down. This is
  the common software convention and the one `texture2ddecoder` uses, so images do not change when
  switching. The result is within 1.4 levels of the exact value (`s3tc` and `rgtc` module docs);
  the `f32` output of BC4 and BC5 is the exact value.
* **BC6H, 8-bit output.** BC6H has no 8-bit form; the convenience clamps to [0, 1], multiplies by
  255 and rounds down, like `texture2ddecoder`. Use the half or `f32` output for the HDR range.
* **BC2 and BC3 always use the four-colour mode** for their colour block, whatever the order of the
  two endpoints (Khronos DFS 18.3 and 18.4). `texture2ddecoder` switches to the three-colour mode
  when `color0 <= color1`, which a GPU does not do for these formats.

## Differences from `texture2ddecoder` 0.1.2

Measured with the black-box oracle in `oracle/` (see below); every difference is listed with the
rule of the specification that decides it.

* BC2 and BC3 four-colour mode (above).
* BC6H, the 16-bit mode (mode 15): the wrap-around of the delta transform must mask with `0xFFFF`
  (DFS 20.2: "wrapped at the number of endpoint bits"); `texture2ddecoder` masks with zero.
* BC6H unquantizing: only the all-ones endpoint maps to the maximum (`x == (1 << EPB) - 1`, DFS 20.2);
  `texture2ddecoder` saturates the half-way value `1 << (EPB - 1)` instead.
* Signed BC6H: endpoints are two's complement and are interpolated as such (DFS 20.2: "oriented
  properly to interpolate as a two's complement value"); `texture2ddecoder` goes wrong whenever the
  two endpoints of a subset have different signs.

## Tests

* Unit tests per format with blocks built by hand from the tables of the specifications, one for each
  BC7 mode and BC6H mode, the edge cases (anchor texels that are not texel 15, reserved modes, mode 15 wrap,
  negative infinity, three-colour BC1, `-128` in signed BC4) and the properties that the 8-bit output
  stays within its stated distance of the exact values.
* Table-driven differential tests: reference decoders that read their fields from the bit positions
  of the specification's tables (BC7 Tables 109 to 113, BC6H Table 122) agree with the decoders on
  millions of random blocks.
* Fuzz-style tests: random and structured blocks and surfaces of random sizes and pitches.
* `oracle/`: a separate program (its own `Cargo.lock`; `texture2ddecoder` is not a dependency of the
  crate) that compares the crate with `texture2ddecoder` as a black box on random blocks.
* `tools/corpus/`: a local tool that walks the textures of your own Warframe and War Thunder installs and
  compares the two decoders on every block, without keeping any game data.

`tools/gen_tables.py` generates `src/tables.rs` (partition patterns, anchor indices, BC6H bit
layouts) and `tests/common/spec_tables.rs` from the Khronos specification and cross-checks the
tables against each other.

## License

Licensed under either of MIT or Apache-2.0, at your option.
