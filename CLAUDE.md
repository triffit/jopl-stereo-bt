# CLAUDE.md — jopl-stereo-bt

Product proposal: see `PRODUCT.md`. Licence: MIT (`LICENSE`).

## Layout

- Cargo workspace, edition 2024, `rust-version = "1.98"`.
- `crates/stereo-bt-core` — platform-independent DSP, PCM conversion and device model
  (no Windows deps).
- `crates/stereo-bt-wasapi` — thin adapter over the `wasapi` crate: `ComGuard`,
  `list_output_devices`, `default_output_id`, `LoopbackCapture`, `RenderStream`,
  `DeviceWatcher`, plus `is_muted`/`set_muted`/`system_volume`, `set_default_output` and
  `MmcssGuard`. The **only** `unsafe` in the workspace lives in `endpoint_volume.rs`,
  `mmcss.rs` and `default_device.rs` (module-level `allow`, every block has a `// SAFETY:`
  comment). `default_device.rs` declares the undocumented `IPolicyConfig` COM interface
  (all 12 methods, vtable order matters) via `windows_core::interface`. Workspace lint is `unsafe_code = "deny"`; core and engine
  `#![forbid(unsafe_code)]`.
  All streams 48 kHz f32 stereo, shared mode with Windows autoconvert.
- `crates/stereo-bt-engine` — `Session`: one capture thread → `FanOut` (one `rtrb` SPSC
  queue per output) → one render thread per output running `RenderPipeline`
  (`Playout` policy + `OutputChain`). Platform behind `Backend` trait; `WasapiBackend`
  on Windows, fake backend in `tests/session.rs`. Live settings via lock-free atomics
  (`OutputControls`). Manual hardware check: `cargo run -p stereo-bt-engine --example route`.
- Engine control: `Supervisor` (single-threaded state machine; every change goes through
  `reconcile()`: source = default device, mute it unless `include_default`, session = selected
  ∩ present − source). Only a source change restarts the session; devices are added/removed
  individually (`Session::add_output`/`remove_output`, new queues reach the capture thread via
  a channel) so others keep playing. `Supervisor::maintain(now)` (engine loop, every 100 ms)
  retries failed outputs/capture after `RETRY_DELAY` (3 s); errors are shown per device. Behind
  `Platform` + `MuteJournal` traits. `Engine` runs it on its own thread (commands with
  replies, device events debounced 300 ms, status snapshot). Mutes are journaled *before*
  muting and restored on stop, drop and next start (`FileMuteJournal`).
- `crates/stereo-bt-app` — binary `jopl-stereo-bt` (Slint 1.18, `fluent-dark` style via
  `build.rs`). Logic in `controller.rs` (behind `EngineControl` + `SettingsStore` traits,
  fully unit-tested), pure `view_model.rs`, persisted `settings.rs` (TOML in
  `%APPDATA%\JOPL-StereoBT\settings.toml`, per-device by endpoint id). `ui.rs` is glue only;
  generated Slint code lives in a lint-exempt `generated` module. UI edits are coalesced and
  pushed to the engine on the 100 ms timer; settings saved 1 s after the last edit and on exit.
- Slint licence (Royalty-free Desktop 2.0): the About popup **must** keep `AboutSlint`.
- Tray (F7, `tray.rs`, `tray-icon` crate): polled on the UI timer; closing the window while
  streaming hides to tray, closing while stopped (or without a tray) quits. Event loop runs via
  `run_event_loop_until_quit`. Icon drawn in code (`icon.rs`).
- Exe icon + version info: `build.rs` draws the icon with `src/icon.rs` (shared with the tray),
  encodes it with `src/ico.rs` into `OUT_DIR/app.ico` and embeds it via `winresource`.
- Window chrome: `ui::select_backend` uses Slint's `unstable-winit-030` hook (Slint pinned
  `~1.18` because of it) for a dark navy title bar and the Mica backdrop; `Theme.background`
  is slightly translucent. No installer: the exe is portable.

## Slint notes

- `row` and `close` are reserved names in Slint; don't use them for properties/callbacks.
- User interaction replaces a control's value binding; components re-push model values in a
  `changed device => { ... }` handler. Element ids inside `if` blocks are not reachable from
  outside, so such controls live in their own component (`DeviceControls`).
- Look follows WaveSync (dark navy cards, purple→blue gradient): tokens in `ui/theme.slint`,
  custom accessible controls in `ui/controls.slint` (`AccentSlider` with arrow/PageUp/Home/End
  keys, `AccentCheckBox`, `Segmented`, `Pill`, `OutlineButton`, `GradientButton`). Std
  widgets only for `ScrollView` and `AboutSlint`.
- The source (default) device's checkbox is the include-default toggle; `Settings::default()`
  has it **on** (default device keeps playing), muting is opt-in.

## Volume and default device

- Follow system volume (setting, default on): `Supervisor::maintain` reads the source's
  `SystemVolume` every 100 ms and sets `Session::set_master_gain` (dB → amplitude, mute → 0;
  our own mute of the source is ignored). Loopback is pre-volume, so this is the only way the
  followers track it. `OutputChain` ramps every gain change over one block (no clicks).
- "Make default" on a card → `Engine::make_default` → `Supervisor::make_default`
  (`IPolicyConfig::SetDefaultEndpoint` for console + multimedia roles), then reconcile.

## Sync model

- Every output keeps its queue at the same cushion (40 ms, `tuning.rs`); priming aligns to
  exactly the cushion, so base latency is equal for all outputs. The user delay is a
  separate `DelayLine` on top.
- Drift (F9): each `RenderPipeline` resamples with `DriftResampler` (4-point Catmull-Rom,
  constant 3-frame latency) at a ratio from `DriftControl` (PI on the smoothed queue fill
  error, ±0.2 % max, ~30 s settling), so the queue settles exactly at the cushion and no
  sync offset builds up. Trim beyond +30 ms and re-prime on underrun remain as safety nets.
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
