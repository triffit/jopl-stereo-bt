# CLAUDE.md — jopl-stereo-bt

Product proposal: see `PRODUCT.md`. Licence: MIT (`LICENSE`).

## Layout

- Cargo workspace, edition 2024, `rust-version = "1.98"`.
- `crates/stereo-bt-core` — platform-independent DSP, PCM conversion and device model
  (no Windows deps).
- `crates/stereo-bt-wasapi` — thin adapter over the `wasapi` crate (no own `unsafe`):
  `ComGuard`, `list_output_devices`, `LoopbackCapture`, `RenderStream`, `DeviceWatcher`.
  All streams 48 kHz f32 stereo, shared mode with Windows autoconvert.
- `crates/stereo-bt-engine` — `Session`: one capture thread → `FanOut` (one `rtrb` SPSC
  queue per output) → one render thread per output running `RenderPipeline`
  (`Playout` policy + `OutputChain`). Platform behind `Backend` trait; `WasapiBackend`
  on Windows, fake backend in `tests/session.rs`. Live settings via lock-free atomics
  (`OutputControls`). Manual hardware check: `cargo run -p stereo-bt-engine --example route`.
- Planned: supervisor (hot-plug restart, default-device mute, MMCSS), `app` (Slint
  `fluent-dark`).

## Sync model

- Every output keeps its queue at the same cushion (40 ms, `tuning.rs`); priming aligns to
  exactly the cushion, so base latency is equal for all outputs. The user delay is a
  separate `DelayLine` on top.
- Drift: trim back to cushion beyond +30 ms; re-prime on underrun. Resampling (rubato)
  is a later refinement.
- Never render to the loopback source (default device): feedback loop.
- WASAPI loopback is captured **before** endpoint volume/mute (verified on hardware
  2026-10-08), so muting the default endpoint keeps the source audio.

## WASAPI notes

- Every thread using WASAPI holds a `ComGuard` (MTA). GUI threads are STA → use a
  dedicated audio thread.
- Loopback delivers no packets while nothing plays; `LoopbackCapture::read` returns an
  empty slice on timeout.
- Hardware smoke tests are `#[ignore]`d: `cargo test -p stereo-bt-wasapi -- --ignored`.

## Conventions

- Workspace lints: `unsafe_code = "forbid"`, `missing_docs`, clippy `pedantic`,
  `unwrap_used`/`expect_used`/`panic` warn (allowed in tests via `clippy.toml`).
- Real-time path (`OutputChain`, `DelayLine`, render threads): no allocation, locks, I/O or
  logging after construction.
- Validated newtypes (`Delay`, `Volume`, `SampleRate`) at every boundary.
- Tests map to `PRODUCT.md` feature IDs (`f2_…`, `f3_…`); TDD red → green → refactor.
- Line endings LF (`.gitattributes`).

## Done means

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

All clean, no warnings.
