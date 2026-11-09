# Agent guide

bcn-decode decodes BC1, BC2, BC3, BC4, BC5, BC6H and BC7 textures for Rust graphics and game developers.
It supplies block and whole-surface decoders, including pitched and allocating APIs.

## Toolchain and CI

- Rust 1.88 minimum (`Cargo.toml` and the MSRV job); normal CI uses stable Rust.
- CI covers Linux, macOS and Windows, with rustfmt and Clippy.
- `std` is enabled by default; disabling it gives `no_std` plus `alloc`.
- From the root, the CI commands are:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --release
cargo test --release --no-default-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

`cargo test --release` also runs on Rust 1.88 and compiles the library for the tests.

## Code map

- `src/lib.rs`, `surface.rs`, `error.rs`: public API, surface decoding and errors.
- `src/s3tc.rs`, `rgtc.rs`, `bc6h.rs`, `bc7.rs`: per-format block decoders.
- `src/tables.rs` and `tests/common/spec_tables.rs`: generated specification tables.
- `tools/gen_tables.py`: generates and cross-checks those tables.
- `tests/`: synthetic, differential and malformed-input tests.
- `oracle/`, `tools/corpus/`, `docs/`: black-box comparisons and format evidence.

## Project rules

- Keep the specification section beside each decoding rule, as in the existing modules.
- Preserve `#![forbid(unsafe_code)]`, dependency-free decoding and malformed-input handling.
- The README records the BC6H provenance, including the earlier `wf-unpack` cross-check.
- `texture2ddecoder` is a separate black-box oracle, not a library dependency.
- The local corpus tool reads installed games without retaining their texture data.

## Public files

- Never commit game files, extracted assets, renders, decompiled code or secrets.
- The public-tree gate needs Python 3.9+ and Git; run it from the repository root:
  `python3 tools/check_public_tree.py` (checks Git's index, including staged new files).
- CI checks committed changes with `python3 tools/check_public_tree.py --range BASE..HEAD`.

## Contributing, license and AI

- Open an issue to discuss a problem or send a pull request with the change.
- Before a pull request, run the applicable CI commands above and the public-tree gate.
- License: MIT OR Apache-2.0, at your option; see `LICENSE-MIT` and `LICENSE-APACHE`.

The code was written with AI coding agents (Anthropic's Claude and OpenAI's GPT models) under the maintainer's direction.
AI-assisted contributions are welcome when their use is disclosed and the changes are reviewed.
