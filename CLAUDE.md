# CLAUDE.md — jopl-stereo-bt

Product proposal: see `PRODUCT.md`. Licence: MIT (`LICENSE`).

## Layout

- Cargo workspace, edition 2024, `rust-version = "1.98"`.
- `crates/stereo-bt-core` — platform-independent DSP, PCM conversion and device model
  (no Windows deps).
- `crates/stereo-bt-wasapi` — thin adapter over the `wasapi` crate: `ComGuard`,
  `list_output_devices`, `default_output_id`, `LoopbackCapture`, `RenderStream`,
  `DeviceWatcher`, plus `is_muted`/`set_muted`/`system_volume`, `set_default_output` and
  `MmcssGuard`, `muted_apps`/`unmute_app`. The **only** `unsafe` in the workspace lives in
  `endpoint_volume.rs`, `app_sessions.rs`, `mmcss.rs` and `default_device.rs` (module-level
  `allow`, every block has a `// SAFETY:` comment). `default_device.rs` declares the undocumented `IPolicyConfig` COM interface
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
  replies, device events debounced 300 ms, status snapshot). Mutes and volume changes are journaled *before* being made and restored on stop, drop and next
  start (`FileRestoreJournal`).
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
  `~1.18` because of it) for a dark title bar and the Mica backdrop; `Theme.window`
  is slightly translucent. No installer: the exe is portable.

## Slint notes

- `row` and `close` are reserved names in Slint; don't use them for properties/callbacks.
- User interaction replaces a control's value binding; components re-push model values in a
  `changed device => { ... }` handler. Element ids inside `if` blocks are not reachable from
  outside, so such controls live in their own component (`DeviceControls`).
- Look: Windows 11 Fluent dark (design: `mockups/windows11-2-pages.html`). Navigation rail
  with three pages: **Devices** (every setting per device), **Sync** (timing chart from the
  last mic measurement, Measure with the microphone, Sync by ear) and **Settings** (toggles +
  About). One button "Play on multiple devices" / "Stop playing"; the info bar says
  Active/Off. Tokens in `ui/theme.slint`, accessible controls in `ui/controls.slint`
  (`Slider` with arrow/PageUp/Home/End keys, `ToggleSwitch`, `Segmented`, `CommandButton`,
  `SubtleButton`, `InfoBar`, `NavItem`, `Card`). Std widgets only for `ScrollView` and
  `AboutSlint` (licence: About lives on the Settings page).
- The source (default) device's switch is the include-default toggle; `Settings::default()`
  has it **on** (default device keeps playing), muting is opt-in.

## Volume and default device

- Follow system volume (setting, default on): `Supervisor::maintain` reads the source's
  `SystemVolume` every 100 ms and sets `Session::set_master_gain` (dB → amplitude, mute → 0;
  our own mute of the source is ignored). Loopback is pre-volume, so this is the only way the
  followers track it. `OutputChain` ramps every gain change over one block (no clicks).
- While streaming and following, target devices' Windows volume is raised to 100 %
  (`Supervisor::reconcile_volumes`) so the system volume is the single control; originals are
  journaled first (`RestoreJournal`: `mute-journal.txt` + `volume-journal.txt` in the data
  folder) and restored on stop/quit/next start, retried for disconnected devices.
- Muted-app warning: every second `Supervisor::maintain` lists apps that are *playing* on the
  default device but muted for it in the Windows volume mixer (their audio reaches no device,
  since loopback is post-session-mute); the UI offers Unmute.
- "Make default" on a card → `Engine::make_default` → `Supervisor::make_default`
  (`IPolicyConfig::SetDefaultEndpoint` for console + multimedia roles), then reconcile.

## Assisted sync (F12)

- `ClickTrack` (engine) renders `ClickGenerator` clicks (core, 5 ms 2 kHz Hann burst, 1 s period)
  into the **source** device; loopback carries them to every output through its real
  pipeline. Emission time = write time + already-queued frames + offset in block.
- `Supervisor::isolate(Some(id))` silences all other outputs (`OutputControls` silenced flag);
  isolating a non-source device also mutes the source (journaled like any mute).
- App `assistant.rs` (pure state machine): steps = source first, then playing outputs; taps
  per step → `TapAnalysis` (median lag, MAD stray rejection, ≥5 taps) → `suggest_delay`
  (reference − device + current delay; flag when < 0: make the slowest device the default).
  Then fine-tune phase: all devices click, user adjusts delays by ear.

## Microphone measurement (F14)

- `MicCapture` (wasapi) opens the default mic in **RAW** mode (voice processing gated the
  chirps to silence otherwise) and skips Bluetooth hands-free mics (≤ 16 kHz).
- `Engine::start_calibration` runs `calibration.rs` on its own thread via `Client` commands:
  per device isolate → 1.4 s warm-up (BT wake-up gives bogus first readings) → 5 chirps
  (`TestSignal::Chirp`, 700 ms) → `MicTimeline::latencies_ms` (cross-correlation,
  `find_arrival` quality ≥ 12) → `robust_latency` (≥ 3 within ±10 ms).
- Mic packets are dated `now − (len + queued)`; the **earliest** implied start wins (jitter only
  makes packets look late). Capture loops must **drain** all queued packets per wake-up —
  reading one per wake-up fell behind and lost audio on hardware.
- Planning (core `auto_calibration.rs`): round 1 with the current default; if a target is later
  than the default even undelayed, round 2 with that target as default yields the app's
  overhead (same device native vs. through the pipeline), then the best default and delays.
- Manual check: `cargo run --release -p stereo-bt-engine --example calibrate [follow]`.

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
