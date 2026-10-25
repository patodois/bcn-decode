# Comparison with texture2ddecoder 0.1.2

`oracle/` runs `texture2ddecoder` as a black box next to this crate and compares the 8-bit RGBA
output block by block. The run of 25 October 2026 on local installs (Warframe, Steam build of
22 October 2026; War Thunder 2.59 for macOS), without keeping any data:

| Format | Warframe blocks | identical | War Thunder blocks | identical | Nocturne blocks | identical |
|---|---:|---:|---:|---:|---:|---:|
| BC1 (opaque, and with 1-bit alpha) | 2,650,278,296 | 100% | 2,674,826,412 | 100% | 122,027,785 | 100% |
| BC3 | 3,331,610,806 | 99.075% | 4,429,985,290 | 100% | 70,782,642 | 100% |
| BC4 | 292,371,345 | 100% | 82,015,617 | 100% | none | |
| BC5 | 1,330,253,494 | 100% | 70,091,242 | 100% | none | |
| BC6H UF16 | 194,248,613 | 99.848% | none in the game | | none | |
| BC7 | 184,988,873 | 100% | 867,006,177 | 100% | 5,313,600 | 100% |
| BC2 | none in the games (random blocks only) | | | | | |

Nocturne is Shin Megami Tensei III Nocturne HD Remaster (Steam, Windows build, Unity 2019.4 bundles):
every Texture2D of the base game and its DLC bundles (15,930 textures; DXT1 10,742, DXT5 3,455, BC7 41, plus uncompressed
RGB24, RGBA32 and Alpha8), counted over all mip levels like the other games. In its DXT5 data
16,440,167 blocks have `color0 <= color1`, and none of them differs: the oracle's three-colour mode only
shows for colour codes 2 and 3, which those blocks do not use.

Random blocks (50,000 per mode and format, fixed seed): BC1, BC1 with alpha, BC4, BC5 and BC7 are 100%
identical; BC2 and BC3 differ only for `color0 <= color1`; BC6H differs only in the classes below.

## Every difference, and the rule that decides it

1. **BC2 and BC3, `color0 <= color1`.** The Khronos Data Format Specification (18.3, 18.4) says the colour
   block of BC2 and BC3 is decoded "as though color0 > color1, regardless of the actual values". The oracle
   switches to the three-colour mode of BC1. In 409,600 random blocks of that kind (6,553,600 texels) 3,278,188 texels differ (half of them);
   every one has colour code 2 or 3, none has a different alpha, none has colour code 0 or 1. In
   Warframe 794,338,002 BC3 blocks have `color0 <= color1`, 30,822,303 of them (3.9%) decode differently;
   in War Thunder 214,703,599 have it and none differs (their code 2 and 3 are never used).
2. **BC6H, 16-bit mode (mode 15).** DFS 20.2: the delta transform wraps "at the number of endpoint bits", 16
   here, so the mask is `0xFFFF`. The oracle's mask is zero. 7,023,837 blocks in Warframe, 642 of them differ after
   clamping to 8 bits (the HDR values above 1.0 that differ are hidden by the clamp).
3. **BC6H unsigned, unquantizing.** DFS 20.2: `x == (1 << EPB) - 1` maps to 0xFFFF and nothing else is special.
   The oracle maps the half-way value `1 << (EPB - 1)` to 0xFFFF and treats the all-ones value like any other.
   Of the 523,017 Warframe blocks with such an endpoint 294,489 differ.
4. **BC6H signed.** DFS 20.2: endpoints are two's complement "oriented properly to interpolate as a two's
   complement value". The oracle differs for every block whose two endpoints of a subset have different signs
   (99.8% of such random blocks differ; with the same signs, 100% agree).

The rest of BC6H is identical: all 186,701,759 Warframe blocks without any of the cases 2 and 3 agree,
and so do all random SF16 blocks without case 2 or 4 (413,984 of 700,000). The Warframe numbers are for UF16,
the only BC6H variant in the game; SF16 is checked with random blocks, the table-driven reference in
`tests/bc6h_spec.rs`, and wf-unpack's own `bc6h.rs` (no difference on 2,000,000 random blocks of each).

## Nocturne, old versus new, pixel by pixel

nocturne-unpack on its `main` branch (texture2ddecoder) and on its `bcn` branch (bcn-decode), on the
installed game: all 15,930 Texture2D (3.75 billion pixels, every format the game uses) decode to the
same RGBA pixels with both, 0 differences, peak RSS 1 GB. `check`, `check --kind fields`, `census
--decode`, `check-bridge --kind demons --textures`, `check-bridge --kind fields`, and
`check-conformance` (`--kind character --compare-load` and `--kind level`) print the same results on both
branches (only timings and the name of the bundle that reached the peak memory differ), and the game-data
tests pass on both (49 unit tests on `bcn`, 48 on `main`: one test was added).

## 8-bit conventions

BC1 to BC5 have no bit-exact 8-bit definition in the specifications. This crate uses the convention of the
oracle (replicated endpoints, integer arithmetic, rounding down), which is why they agree at 100%. The BC6H
8-bit convenience rounds down like the oracle. See the module documentation.

## Throughput

Single thread, Apple silicon, release build, best of 5, shared machine (load 3 to 8, so +-15%); output
RGBA8 for both, `texture2ddecoder` writes `u32`. Real blocks from the local corpus; the ratio is ours / oracle.

| Format | ours (Mpx/s) | texture2ddecoder (Mpx/s) | ratio |
|---|---:|---:|---:|
| BC1 | 630 to 870 | 510 to 700 | 1.0 to 1.25 |
| BC3 | 640 | 610 to 680 | 0.95 to 1.05 |
| BC4 | 770 | 850 to 890 | 0.9 |
| BC5 | 530 | 570 to 600 | 0.9 |
| BC6H UF16 | 107 | 92 | 1.2 |
| BC7 | 136 to 146 | 68 to 76 | 2.0 |
