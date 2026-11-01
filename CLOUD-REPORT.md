# CLOUD-REPORT: coverage-guided fuzzing of `bcn-decode`

## What was done
* Added `fuzz/` (cargo-fuzz / libFuzzer, nightly). It has its own empty `[workspace]`, so the stable
  build, `cargo test`, clippy and CI never touch it; `/fuzz` is also in the crate's `exclude` list.
* Seven targets:
  * `surface_rgba8`: `decode_rgba8`, `decode_rgba8_pitched`, `decode_rgba8_to_vec`
  * `surface_f32`: `decode_rgba_f32`, `_pitched`, `_to_vec`
  * `surface_bc6h_f16`: `decode_bc6h_rgba_f16`, `_pitched` (for every `Format`)
  * `block_s3tc`, `block_rgtc`, `block_bc6h`, `block_bc7`: every public block decoder (fixed-size arrays)
* Surface targets fuzz format, width, height, source/destination pitch, destination length and the
  bytes. Sizes are small, 16-bit, arbitrary 64-bit, `usize::MAX - n`, and `2^n +- 1`. Source can be
  tiled to the size the surface needs so the decode path runs, not only the error path.
* Properties checked besides "no panic / overflow / OOM / hang" (builds have debug assertions and
  overflow checks on):
  * nothing is written to the destination when an error is returned (documented in `decode_rgba8_pitched`);
  * padding between rows is untouched;
  * `_to_vec` output is bounded by the source length;
  * `_to_vec`, tight and pitched-with-tight-pitch calls agree;
  * a 4x4 surface equals the block decoder;
  * the BC6H half / f32 / RGBA8 outputs agree with each other (`half_to_unorm8` against its documented
    "clamp to [0,1], x255, round down").
* Seed corpora are hand-made bit patterns from `fuzz/` only (no game data): every BC7 mode byte, every
  BC6H mode (incl. reserved 19/23/27/31), BC1 4-/3-colour, BC4 8-/6-value, all-0/all-1 blocks, and
  surface headers for each format and sizes 0, 1, 4, 5, 8, huge, near `usize::MAX`. The committed
  corpora are the seeds plus what the fuzzers kept, reduced with `cargo fuzz cmin`.

## Fuzzing time (wall clock, one core each, run 4 at a time)
| Target | Time | Executions | Final coverage |
|---|---|---|---|
| surface_rgba8 | 31 min | 5.9 M | 1061 |
| surface_f32 | 31 min | 3.5 M | 1605 |
| surface_bc6h_f16 | 31 min | 4.0 M | 673 |
| block_s3tc | 31 min | 131.6 M | 202 |
| block_rgtc | 31 min | 36.8 M | 1044 |
| block_bc6h | 31 min | 67.4 M | 351 |
| block_bc7 | 31 min | 147.6 M | 190 |

Coverage had stopped growing on the block targets well before the end. The surface targets were
still slowly growing, but no new failure appeared.

## Bugs found in the library
**None.** No panic, arithmetic overflow, out-of-memory, excessive allocation or hang was found in any
target, so no library code was changed, no regression test was added, and there are no minimised crash
inputs to add to the corpus. (A fix without a failing input would have been invented.)

## Harness finding (not a library crash)
The first smoke run flagged `decode_bc6h_rgba_f16` with a non-BC6H format and a width that overflows
`usize`: it returns `Error::SizeOverflow`, while the docs of `decode_bc6h_rgba_f16_pitched` say
`Error::UnsupportedOutput` "for any other format". The tight variant computes the pitches before the
pitched variant checks the format. Both are errors, nothing is written, nothing panics. I left it
unchanged (changing which error is returned is an API decision for the owner) and the harness accepts
either error for the tight variant, and requires `UnsupportedOutput` for the pitched one.
Similarly `decode_rgba8(width = usize::MAX/2, height = 0)` returns `SizeOverflow` although the surface
is empty; harmless, not changed.

## Left out / uncertain
* Branch name: the task asked for `cloud/fuzz-bcn-decode`; the owner confirmed from the local session that
  `claude/fuzz-bcn-decode-1yc90l` (the only branch this session may push) replaces it.
* 30 minutes per target is the minimum asked; since nothing was found I did not go longer. The surface
  targets run at only ~2-3k exec/s (they build buffers), so they have had far fewer executions than the
  block targets. A longer run of those is the most useful follow-up.
* Destination buffers the harness hands in are capped at 64Ki elements, so very large *valid* surfaces
  are not decoded (their size arithmetic is still exercised through the error path and `_to_vec`).
* The tests in `tests/` and CI were not changed. Not run in the cloud: macOS/Windows CI, MSRV 1.88
  (only the installed stable 1.97 was available); nothing in the diff touches `src/`.
* Not covered: the `oracle/` and `tools/corpus/` helper crates, and the `no_std` build under fuzzing
  (it was built and tested with `--no-default-features`).

## How to run
`cargo install cargo-fuzz`, then in `fuzz/`: `cargo +nightly fuzz run surface_rgba8 -- -max_len=4096`.
