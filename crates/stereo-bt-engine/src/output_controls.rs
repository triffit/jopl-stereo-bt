use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use stereo_bt_core::{ChannelMode, Delay, Volume};

use crate::state_cell::StateCell;
use crate::{OutputConfig, StreamState};

/// Settings and counters shared lock-free between the control side and one
/// render thread. Every access is a single relaxed atomic: values are
/// independent, so no ordering between them is needed.
#[derive(Debug)]
pub(crate) struct OutputControls {
    delay_millis: AtomicU16,
    volume_percent: AtomicU8,
    channel_mode: AtomicU8,
    state: StateCell,
    underruns: AtomicU64,
    trims: AtomicU64,
    overruns: AtomicU64,
    stop: AtomicBool,
    /// Shared gain (e.g. the system volume) as `f32` bits.
    master_gain: AtomicU32,
    /// Silenced while another device is isolated for assisted sync.
    silenced: AtomicBool,
    /// Set once, when the render thread fails; never touched on the audio path.
    error: Mutex<Option<String>>,
}

impl OutputControls {
    pub(crate) fn new(config: &OutputConfig) -> Self {
        Self {
            delay_millis: AtomicU16::new(config.delay.as_millis()),
            volume_percent: AtomicU8::new(config.volume.as_percent()),
            channel_mode: AtomicU8::new(mode_to_u8(config.channel_mode)),
            state: StateCell::new(),
            underruns: AtomicU64::new(0),
            trims: AtomicU64::new(0),
            overruns: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            master_gain: AtomicU32::new(1.0_f32.to_bits()),
            silenced: AtomicBool::new(false),
            error: Mutex::new(None),
        }
    }

    pub(crate) fn delay(&self) -> Delay {
        // Only validated values are ever stored.
        Delay::from_millis(self.delay_millis.load(Ordering::Relaxed)).unwrap_or(Delay::ZERO)
    }

    pub(crate) fn set_delay(&self, delay: Delay) {
        self.delay_millis
            .store(delay.as_millis(), Ordering::Relaxed);
    }

    pub(crate) fn volume(&self) -> Volume {
        Volume::from_percent(self.volume_percent.load(Ordering::Relaxed)).unwrap_or(Volume::FULL)
    }

    pub(crate) fn set_volume(&self, volume: Volume) {
        self.volume_percent
            .store(volume.as_percent(), Ordering::Relaxed);
    }

    pub(crate) fn channel_mode(&self) -> ChannelMode {
        mode_from_u8(self.channel_mode.load(Ordering::Relaxed))
    }

    pub(crate) fn set_channel_mode(&self, mode: ChannelMode) {
        self.channel_mode.store(mode_to_u8(mode), Ordering::Relaxed);
    }

    pub(crate) fn state(&self) -> StreamState {
        self.state.get()
    }

    pub(crate) fn set_state(&self, state: StreamState) {
        self.state.set(state);
    }

    pub(crate) fn record_playout(&self, underruns: u64, trims: u64) {
        self.underruns.store(underruns, Ordering::Relaxed);
        self.trims.store(trims, Ordering::Relaxed);
    }

    pub(crate) fn add_overrun(&self) {
        self.overruns.fetch_add(1, Ordering::Relaxed);
    }

    /// The shared gain, or 0 while this output is silenced.
    pub(crate) fn master_gain(&self) -> f32 {
        if self.silenced.load(Ordering::Relaxed) {
            0.0
        } else {
            f32::from_bits(self.master_gain.load(Ordering::Relaxed))
        }
    }

    pub(crate) fn set_silenced(&self, silenced: bool) {
        self.silenced.store(silenced, Ordering::Relaxed);
    }

    pub(crate) fn set_master_gain(&self, amplitude: f32) {
        self.master_gain
            .store(amplitude.to_bits(), Ordering::Relaxed);
    }

    /// Asks this output's render thread to finish.
    pub(crate) fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub(crate) fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub(crate) fn set_error(&self, error: String) {
        *self.error.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
    }

    pub(crate) fn error(&self) -> Option<String> {
        self.error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    pub(crate) fn trims(&self) -> u64 {
        self.trims.load(Ordering::Relaxed)
    }

    pub(crate) fn overruns(&self) -> u64 {
        self.overruns.load(Ordering::Relaxed)
    }
}

const fn mode_to_u8(mode: ChannelMode) -> u8 {
    match mode {
        ChannelMode::Stereo => 0,
        ChannelMode::LeftOnly => 1,
        ChannelMode::RightOnly => 2,
        ChannelMode::MonoMix => 3,
    }
}

const fn mode_from_u8(value: u8) -> ChannelMode {
    match value {
        1 => ChannelMode::LeftOnly,
        2 => ChannelMode::RightOnly,
        3 => ChannelMode::MonoMix,
        _ => ChannelMode::Stereo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stereo_bt_core::DeviceId;

    fn controls() -> OutputControls {
        OutputControls::new(&OutputConfig::new(DeviceId::new("dev").unwrap()))
    }

    #[test]
    fn every_channel_mode_round_trips() {
        let c = controls();
        for mode in [
            ChannelMode::Stereo,
            ChannelMode::LeftOnly,
            ChannelMode::RightOnly,
            ChannelMode::MonoMix,
        ] {
            c.set_channel_mode(mode);
            assert_eq!(c.channel_mode(), mode);
        }
    }

    #[test]
    fn delay_and_volume_round_trip() {
        let c = controls();
        c.set_delay(Delay::from_millis(123).unwrap());
        c.set_volume(Volume::from_percent(42).unwrap());

        assert_eq!(c.delay().as_millis(), 123);
        assert_eq!(c.volume().as_percent(), 42);
    }
}
