use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use stereo_bt_core::{
    BassCut, ChannelMode, Delay, Percent, Space, SpeakerPosition, StereoWidth, VirtualBass, Volume,
};

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
    position: AtomicU8,
    bass_cut_hz: AtomicU16,
    virtual_bass_percent: AtomicU8,
    width_percent: AtomicU8,
    reverb_percent: AtomicU8,
    room_percent: AtomicU8,
    /// Incremented for every ping request; the render thread plays one per change.
    ping_requests: AtomicU32,
    /// Current peak level of the played audio, as `f32` bits.
    level: AtomicU32,
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
            position: AtomicU8::new(position_to_u8(config.position)),
            bass_cut_hz: AtomicU16::new(config.bass_cut.as_hz()),
            virtual_bass_percent: AtomicU8::new(config.virtual_bass.as_percent()),
            width_percent: AtomicU8::new(Space::default().width.as_percent()),
            reverb_percent: AtomicU8::new(Space::default().reverb.get()),
            room_percent: AtomicU8::new(Space::default().room_size.get()),
            ping_requests: AtomicU32::new(0),
            level: AtomicU32::new(0.0_f32.to_bits()),
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

    pub(crate) fn bass_cut(&self) -> BassCut {
        // Only validated values are ever stored.
        BassCut::from_hz(self.bass_cut_hz.load(Ordering::Relaxed)).unwrap_or(BassCut::OFF)
    }

    pub(crate) fn set_bass_cut(&self, cut: BassCut) {
        self.bass_cut_hz.store(cut.as_hz(), Ordering::Relaxed);
    }

    pub(crate) fn virtual_bass(&self) -> VirtualBass {
        VirtualBass::from_percent(self.virtual_bass_percent.load(Ordering::Relaxed))
            .unwrap_or(VirtualBass::OFF)
    }

    pub(crate) fn set_virtual_bass(&self, amount: VirtualBass) {
        self.virtual_bass_percent
            .store(amount.as_percent(), Ordering::Relaxed);
    }

    pub(crate) fn space(&self) -> Space {
        let defaults = Space::default();
        // Only validated values are ever stored.
        Space {
            width: StereoWidth::from_percent(self.width_percent.load(Ordering::Relaxed))
                .unwrap_or(defaults.width),
            reverb: Percent::new(self.reverb_percent.load(Ordering::Relaxed))
                .unwrap_or(defaults.reverb),
            room_size: Percent::new(self.room_percent.load(Ordering::Relaxed))
                .unwrap_or(defaults.room_size),
        }
    }

    pub(crate) fn set_space(&self, space: Space) {
        self.width_percent
            .store(space.width.as_percent(), Ordering::Relaxed);
        self.reverb_percent
            .store(space.reverb.get(), Ordering::Relaxed);
        self.room_percent
            .store(space.room_size.get(), Ordering::Relaxed);
    }

    pub(crate) fn request_ping(&self) {
        self.ping_requests.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn ping_requests(&self) -> u32 {
        self.ping_requests.load(Ordering::Relaxed)
    }

    /// The peak level of the played audio (linear).
    pub(crate) fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    pub(crate) fn set_level(&self, level: f32) {
        self.level.store(level.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn position(&self) -> SpeakerPosition {
        position_from_u8(self.position.load(Ordering::Relaxed))
    }

    pub(crate) fn set_position(&self, position: SpeakerPosition) {
        self.position
            .store(position_to_u8(position), Ordering::Relaxed);
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

const fn position_to_u8(position: SpeakerPosition) -> u8 {
    match position {
        SpeakerPosition::Front => 0,
        SpeakerPosition::Center => 1,
        SpeakerPosition::Rear => 2,
    }
}

const fn position_from_u8(value: u8) -> SpeakerPosition {
    match value {
        1 => SpeakerPosition::Center,
        2 => SpeakerPosition::Rear,
        _ => SpeakerPosition::Front,
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

    #[test]
    fn f20_every_position_round_trips() {
        let c = controls();
        for position in [
            SpeakerPosition::Front,
            SpeakerPosition::Center,
            SpeakerPosition::Rear,
        ] {
            c.set_position(position);
            assert_eq!(c.position(), position);
        }
    }

    #[test]
    fn f16_space_round_trips() {
        let c = controls();
        assert_eq!(c.space(), Space::default());
        let space = Space {
            width: StereoWidth::from_percent(170).unwrap(),
            reverb: Percent::new(25).unwrap(),
            room_size: Percent::new(80).unwrap(),
        };

        c.set_space(space);

        assert_eq!(c.space(), space);
    }

    #[test]
    fn f15_bass_settings_round_trip() {
        let c = controls();
        assert_eq!(c.bass_cut(), BassCut::OFF);
        c.set_bass_cut(BassCut::from_hz(150).unwrap());
        c.set_virtual_bass(VirtualBass::from_percent(60).unwrap());

        assert_eq!(c.bass_cut().hz(), Some(150));
        assert_eq!(c.virtual_bass().as_percent(), 60);
    }
}
