# PRODUCT.md — JOPL-StereoBT

## Pitch

Play system audio through multiple Bluetooth earbuds, headphones and speakers at once —
with per-device sync delay, true stereo split across two devices, and zero virtual drivers.
A native Windows 11 app written in Rust, inspired by
[WaveSync](https://indalsingh.dev/wavesync) ([source](https://github.com/Indal-Singh/WaveSync), MIT).

## Problem

Windows routes audio to exactly one output device. Two people sharing one laptop with two
Bluetooth headsets, or two cheap Bluetooth speakers used as a stereo pair, are not supported.
Existing workarounds (Stereo Mix, VB-Cable, Voicemeeter) need virtual drivers, are fiddly,
and do not compensate for the different latency of each Bluetooth device.

## Target users

- Two people watching/listening on one PC, each with their own headset.
- Users pairing two Bluetooth speakers as a left/right stereo set.
- Small rooms: PC speakers plus a Bluetooth speaker, kept in sync.

## Core features (v1)

| # | Feature | Notes |
|---|---------|-------|
| F1 | Multi-device output | Any active render endpoint: Bluetooth, USB, 3.5 mm, HDMI. |
| F2 | Stereo split | Per device channel mode: **Stereo**, **Left only**, **Right only**, **Mono mix**. Two devices = one stereo pair. |
| F3 | Per-device sync delay | 0–500 ms slider, 1 ms steps, plus fine ±1 ms nudge buttons. |
| F4 | Per-device volume | 0–100 %, independent of the Windows master volume. |
| F5 | One-click play/stop | "Play on multiple devices" / "Stop playing" button; also from the tray. |
| F6 | Hot-plug | Device removal does not stop others; reconnect resumes automatically. |
| F7 | Tray mode | Keeps streaming when the window is closed; tray menu for start/stop/quit. |
| F8 | Profiles | Remember device set, channel mode, delay and volume per device ID. |
| F9 | Clock-drift correction | Equal per-output queue cushion plus click-free adaptive resampling (±0.2 %, inaudible) that keeps every queue at the cushion; trimming only as a safety net. WaveSync has neither. |
| F10 | Follow system volume | All selected devices follow the Windows volume and mute of the default device (on by default); their own Windows volume is set to 100 % while streaming (restored afterwards); per-device Vol stays as a relative trim. |
| F11 | Make default | Switch the Windows default output device from a device card. |
| F12 | Assisted sync | Click track played into the default device (so it reaches every device through its real path); tap along per isolated device, get suggested delays, then fine-tune by ear with all devices clicking. |
| F13 | Muted-app warning | Warns when an app plays on the default device but is muted for it in the Windows volume mixer (its sound then reaches no device), with an Unmute button. |
| F14 | Measure with mic | Plays sweeps on one device at a time, finds them in a RAW microphone recording, and recommends the default device (the slowest by itself) and delays; verified within ~1 ms on hardware. |
| F15 | Small-speaker sound | Per device: bass cut (24 dB/oct high-pass, off or 40–300 Hz), virtual bass (harmonics of the cut bass), a speaker volume limit while following the system volume, and an always-on limiter at full scale. No added latency. |
| F16 | Space | For every device played through the app: stereo width (mid/side, 0–200 %) and a Freeverb-style room reverb (amount, room size) with different left and right tails, so a speaker pair sounds further apart. No added latency. |
| F17 | Battery level | Each device card shows the battery level Windows knows for it (Bluetooth devices that report one, usually via the hands-free profile), refreshed every minute; amber below 15 %. |
| F18 | Ping | A Ping button on each playing device plays a short double beep on it only, regardless of its volume, to tell identical speakers apart. |
| F19 | Level meter | Each playing device shows a peak level meter (-60 to 0 dB, falling 30 dB/s), amber near full scale. |

## Later (v2+)

- Per-app capture (process loopback, Windows 10 2004+) instead of whole-system audio.
- Auto-start with Windows.

## Non-goals

- No virtual audio driver, no kernel components.
- No macOS/Linux.
- No audio effects/EQ beyond volume and channel mapping.
- No Bluetooth pairing UI (use Windows Settings).

## UX

- Native Windows 11 Fluent look, **dark mode** (follow system theme later), Mica backdrop.
- One window with a navigation rail: **Devices** (a card per device with name, type icon,
  sync status, on/off switch, channels, delay slider and volume slider), **Sync** (when each
  device plays, measure with the microphone, sync by ear) and **Settings** (playback
  options, About). An info bar shows Active/Off and what is playing.
- Keyboard-first and accessible: full tab order, arrow keys on sliders, screen-reader names.

## Architecture

```
WASAPI loopback (default render endpoint, 48 kHz f32 stereo)
        │ capture thread
        ▼
  fan-out ──► SPSC ring ──► [delay line → channel map → volume → drift resampler] ──► WASAPI render (device A)
          ──► SPSC ring ──► [ ... ]                                                ──► WASAPI render (device B)
```

- **Real-time rule:** render threads do no heap allocation, no locks, no I/O, no logging.
  MMCSS "Pro Audio" thread priority.
- **Default device:** always the loopback source. Setting **"Keep default device playing"**
  (default **on**, the default device's checkbox): on = default device keeps playing
  undelayed and the other devices are delayed relative to it; off = default device is muted
  while streaming and only the selected devices play.
  Off (mute) is the better sync mode: every audible device is rendered by us, so any device can be
  delayed relative to any other. Loopback captures before endpoint mute (verified), so muting
  keeps the source. The previous mute state is restored on stop and after a crash.
  **Limitation:** mute mode needs a spare output device to act as the (muted) source, e.g. a
  monitor's HDMI audio. With only two physical outputs, use include-default mode: the default
  device plays natively and the other is delayed to match (this cannot split stereo across
  both, since the default device always plays the full mix).
- **Device events:** `IMMNotificationClient` → event channel → engine supervisor restarts
  only the affected pipeline.

### Crates (Cargo workspace)

| Crate | Responsibility | Depends on Windows? |
|-------|----------------|---------------------|
| `crates/stereo-bt-core` | DSP: delay line, channel mapping, volume, drift controller, config/profile model, validation. Pure Rust, fully unit/property tested. | No |
| `crates/stereo-bt-wasapi` | Capture/render streams, device enumeration, notifications. | Yes |
| `crates/stereo-bt-engine` | Pipeline orchestration, supervisor, state machine (Stopped/Starting/Running/Degraded). | Via trait only (testable with fakes) |
| `app` | UI, tray, settings persistence. Thin: UI events → engine commands. | Yes |

### Key dependencies

- `wasapi` (WASAPI wrapper) + `windows` (COM, `IMMNotificationClient`, MMCSS).
- `rtrb` (lock-free SPSC ring buffer), `rubato` (resampling).
- `thiserror` in libraries, `anyhow` only in `app`; `tracing` for logging (never on the RT path).
- `tray-icon`, `serde` + `toml` for profiles in `%APPDATA%`.

### UI toolkit — Slint (`fluent-dark` style)

Pure Rust, native rendering (no WebView), small binary, built-in Fluent widgets. Mica backdrop
via a small `windows` call. Rejected: Tauri (WebView2, not native) and WinUI 3 (immature Rust
XAML bindings).

Used under the **Slint Royalty-free Desktop License 2.0** (free for desktop apps). Obligations:

- Show the `AboutSlint` widget in the app's About section, **or** show the Slint badge on
  the public download page. We show it under About on the Settings page.
- Do not redistribute Slint on its own or expose Slint APIs from our app.
- Keep Slint's licence notices intact.

## Quality bar

- `#![forbid(unsafe_code)]` everywhere except the narrowly-scoped WASAPI/COM crate (audited,
  documented `unsafe` only where the `windows` crate requires it).
- TDD: core DSP covered by unit + property tests (e.g. "delay of N ms shifts output by exactly
  N × 48 samples", "Left-only never leaks right channel"); engine tested with fake devices.
- Zero compiler/clippy warnings, `cargo fmt` clean.
- Targets: < 1 % CPU, < 30 MB RAM, no audible glitches over a 2-hour session with two
  Bluetooth devices.

## Platform

Windows 11 x64 (Windows 10 22H2 best-effort). Distributed as a single portable
`jopl-stereo-bt.exe` (no installer); settings live in `%APPDATA%\JOPL-StereoBT`.

## Success criteria

1. Two Bluetooth headsets play the same video audio, perceived in sync after delay tuning.
2. Two Bluetooth speakers act as left/right stereo pair.
3. Unplugging/turning off one device never interrupts the other; reconnect resumes within 3 s.
4. No drift audible after 2 hours of continuous playback.

## Licence

Personal project, **MIT** — Copyright (c) 2026 JOPL, Triffit (see `LICENSE`). Third-party: Slint under its Royalty-free Desktop License 2.0
(see UI toolkit); other dependencies MIT/Apache-2.0 compatible.
